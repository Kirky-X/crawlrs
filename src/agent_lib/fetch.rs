// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! `fetch()`：拉取 URL 的 raw HTML → 正文提取 → Markdown。
//!
//! 实现要点：
//!
//! - **手动重定向循环**（`redirect(Policy::none())`），每跳（含重定向）先经
//!   [`super::EgressGuard`] 裁决，deny 立即返回 [`AgentLibError::EgressDenied`]
//!   且未发起连接（A004）。
//! - **SSRF 防护**：每跳用 [`SsrfValidator`] 做 DNS 校验 + 解析结果 pinning
//!   （`resolve_to_addrs`），与 `engines::client::reqwest` 的 SSRF 客户端同构。
//! - **`peer_addr`** 从响应 connection 元数据回填（`Response::remote_addr`），
//!   供接入方做 rebinding 日志对账。
//! - **`max_bytes`** 上限：content-length 预检 + `bytes_stream` 累积截断。
//! - **编码**：按 Content-Type charset 解码，回退 UTF-8 lossy。
//! - **Markdown 管道**：`ContentExtractionFacade`（trafilatura 主 / dom_smoothie
//!   回退）提取正文 → `HtmdMarkdownService` 转 Markdown；提取失败/为空时回退
//!   整页 HTML 转换；标题前置为 `# {title}`。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::LOCATION;
use reqwest::redirect::Policy;
use reqwest::Response;

use crate::domain::services::content_extractor::ContentExtractionFacade;
use crate::domain::services::markdown_service::{HtmdMarkdownService, MarkdownServiceTrait};
use crate::infrastructure::dns::create_ipv4_only_resolver;
use crate::infrastructure::security::ssrf::{SsrfValidator, ValidatedUrl};

use super::error::AgentLibError;
use super::EgressGuard;

/// 默认响应体上限（5MB）
pub const DEFAULT_MAX_BYTES: usize = 5 * 1024 * 1024;
/// 默认请求超时（20s）
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// 最大重定向次数
const MAX_REDIRECTS: u8 = 10;

/// 抓取请求选项。
#[derive(Clone)]
pub struct FetchOptions {
    /// 响应体字节上限（默认 5MB）
    pub max_bytes: usize,
    /// 请求超时（默认 20s）
    pub timeout: Duration,
    /// 逐跳出口裁决；`Some` 时每跳请求前询问，`None` 行为与平台现状一致
    pub egress: Option<Arc<dyn EgressGuard>>,
    /// 是否跟随重定向（默认 true；配 egress 时逐跳校验）
    pub follow_redirects: bool,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            timeout: DEFAULT_TIMEOUT,
            egress: None,
            follow_redirects: true,
        }
    }
}

/// 抓取结果。
#[derive(Debug, Clone)]
pub struct FetchedContent {
    /// Markdown 文本（含 `# {title}` 前置标题与正文）
    pub markdown: String,
    /// 页面标题（无则空字符串）
    pub title: String,
    /// 最终实际请求的 URL（重定向后）
    pub resolved_url: String,
    /// 实际连接地址（供 rebinding 日志对账）
    pub peer_addr: Option<String>,
}

/// 拉取 URL 并转换为 Markdown。
///
/// # 参数
///
/// - `url`: 目标 URL（仅支持 http/https）
/// - `opts`: 抓取选项（限长/超时/出口裁决/重定向）
///
/// # 错误
///
/// - `InvalidUrl`: URL 解析失败或 scheme 不支持
/// - `SsrfDenied`: SSRF 校验未通过
/// - `EgressDenied`: 逐跳出口裁决拒绝（未发起连接）
/// - `HttpStatus`: 最终响应非 2xx
/// - `MaxBytesExceeded`: 响应体超限
/// - `Timeout` / `Network` / `Extraction` / `Markdown`
pub async fn fetch(url: &str, opts: &FetchOptions) -> Result<FetchedContent, AgentLibError> {
    let parsed =
        url::Url::parse(url).map_err(|e| AgentLibError::InvalidUrl(format!("{}: {}", url, e)))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(AgentLibError::InvalidUrl(format!(
            "unsupported scheme '{}' for {}",
            parsed.scheme(),
            url
        )));
    }

    let validator = SsrfValidator::new();
    let mut current = parsed;
    let mut redirects: u8 = 0;
    let mut final_peer_addr: Option<String> = None;

    loop {
        // 每跳出口裁决（请求前判定，deny 未发起连接）。
        //
        // 设计 egress=Some 时 guard 是逐跳唯一权威（agentstem 将
        // EgressPolicy::validate_url 六道护栏包装为 guard，allow=validate_url==Ok），
        // crawlrs 不再独立执行 SSRF 拦截，否则 allowlist 注入的内部 URL 会被
        // crawlrs 二次拒绝（allowlist 经 EgressGuard 生效）。
        if let Some(guard) = &opts.egress {
            if !guard.allow(&current) {
                return Err(AgentLibError::EgressDenied {
                    url: current.to_string(),
                });
            }
        }

        // 构建本跳 client。
        // - egress=Some：guard 已裁决，无需 crawlrs 侧 SSRF；用普通 client（redirect=none）。
        // - egress=None：行为与平台现状一致——crawlrs 独立执行 SSRF 校验 + DNS pinning。
        let client = if opts.egress.is_some() {
            build_plain_client(opts.timeout)?
        } else {
            let validated = validator.validate(current.as_str()).await.map_err(|e| {
                AgentLibError::SsrfDenied {
                    url: current.to_string(),
                    // 脱敏：只保留错误类别，不回显解析出的内网 IP/主机名
                    reason: e.kind().to_string(),
                }
            })?;
            build_pinned_client(&validated, opts.timeout)?
        };

        // GET 请求
        let resp = client
            .get(current.clone())
            .send()
            .await
            .map_err(|e| map_reqwest_error(current.as_str(), e))?;

        // peer_addr 从 connection 元数据回填。
        // 逐跳覆盖：重定向后保留最终响应的 peer 地址（与 final_url 语义一致），
        // 而非首跳地址——首跳值会误导审计/日志中的对端记录。
        final_peer_addr = resp.remote_addr().map(|a| a.to_string());

        // 重定向处理
        if opts.follow_redirects && resp.status().is_redirection() {
            redirects += 1;
            if redirects > MAX_REDIRECTS {
                return Err(AgentLibError::TooManyRedirects {
                    max_redirects: MAX_REDIRECTS,
                });
            }
            let location = resp
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    AgentLibError::Network(format!(
                        "redirect {} missing Location header",
                        resp.status()
                    ))
                })?;
            current = current
                .join(location)
                .map_err(|e| AgentLibError::InvalidUrl(format!("bad redirect: {}", e)))?;
            continue;
        }

        // 非 2xx 状态码
        if !resp.status().is_success() {
            return Err(AgentLibError::HttpStatus {
                status: resp.status().as_u16(),
                url: current.to_string(),
            });
        }

        // 读取响应体（max_bytes 限制 + 编码解码）
        let html = read_body_limited(resp, opts.max_bytes).await?;

        // 正文提取 + Markdown 管道
        return convert_to_markdown(&html, &current, final_peer_addr).await;
    }
}

/// 构建普通 HTTP client（egress=Some 路径，redirect=none 手动处理）。
fn build_plain_client(timeout: Duration) -> Result<reqwest::Client, AgentLibError> {
    reqwest::Client::builder()
        .user_agent(crate::utils::http_client::DEFAULT_USER_AGENT)
        .timeout(timeout)
        .cookie_store(true)
        .local_address(Some(Ipv4Addr::UNSPECIFIED.into()))
        .dns_resolver(create_ipv4_only_resolver())
        .redirect(Policy::none())
        .build()
        .map_err(|e| AgentLibError::Network(format!("failed to build client: {}", e)))
}

/// 构建 DNS pinning 的 HTTP client（仅本函数使用，redirect=none 手动处理）。
fn build_pinned_client(
    validated: &ValidatedUrl,
    timeout: Duration,
) -> Result<reqwest::Client, AgentLibError> {
    let host = validated.parsed_url.host_str().unwrap_or("").to_string();
    let port = validated.port;
    let resolve_addrs: Vec<SocketAddr> = validated
        .resolved_ips
        .iter()
        .map(|ip| SocketAddr::new(*ip, port))
        .collect();
    if resolve_addrs.is_empty() {
        return Err(AgentLibError::Internal("SSRF: no resolved IPs".to_string()));
    }

    let mut builder = reqwest::Client::builder()
        .user_agent(crate::utils::http_client::DEFAULT_USER_AGENT)
        .timeout(timeout)
        .cookie_store(true)
        .local_address(Some(Ipv4Addr::UNSPECIFIED.into()))
        .dns_resolver(create_ipv4_only_resolver())
        .redirect(Policy::none());

    if !host.is_empty() && host.parse::<IpAddr>().is_err() {
        builder = builder.resolve_to_addrs(&host, &resolve_addrs);
    }

    builder
        .build()
        .map_err(|e| AgentLibError::Network(format!("failed to build client: {}", e)))
}

/// 读取响应体，受 `max_bytes` 限制。
///
/// 委托给共享实现 [`crate::utils::http_client::read_body_limited`]（content-length
/// 预检 + `bytes_stream` 累积截断 + charset 解码），把中立错误映射为 [`AgentLibError`]，
/// 消除与引擎/搜索客户端的重复实现。
async fn read_body_limited(resp: Response, max_bytes: usize) -> Result<String, AgentLibError> {
    crate::utils::http_client::read_body_limited(resp, max_bytes)
        .await
        .map_err(|e| match e {
            crate::utils::http_client::BodyReadError::LimitExceeded { max_bytes } => {
                AgentLibError::MaxBytesExceeded { max_bytes }
            }
            crate::utils::http_client::BodyReadError::Network(e) => {
                AgentLibError::Network(e.to_string())
            }
        })
}

/// 正文提取 → Markdown 转换（标题前置 + 失败回退整页）。
async fn convert_to_markdown(
    html: &str,
    current: &url::Url,
    peer_addr: Option<String>,
) -> Result<FetchedContent, AgentLibError> {
    let facade = ContentExtractionFacade::new(None);
    let markdown_service = HtmdMarkdownService::new();

    let extracted = facade.extract(html, current.as_str()).await;

    let (title, body) = match extracted {
        Ok(c) if !c.text.trim().is_empty() => {
            let title = c.title.clone().unwrap_or_default();
            let md = markdown_service
                .to_markdown(&c.text, false)
                .map_err(|e| AgentLibError::Markdown(e.to_string()))?;
            (title, md)
        }
        _ => {
            // 提取失败/为空 → 回退整页 HTML 转换
            let md = markdown_service
                .to_markdown(html, false)
                .map_err(|e| AgentLibError::Markdown(e.to_string()))?;
            (String::new(), md)
        }
    };

    let markdown = if title.is_empty() {
        body
    } else {
        format!("# {}\n\n{}", title, body)
    };

    Ok(FetchedContent {
        markdown,
        title,
        resolved_url: current.to_string(),
        peer_addr,
    })
}

/// 将 reqwest 错误映射为 [`AgentLibError`]。
fn map_reqwest_error(url: &str, e: reqwest::Error) -> AgentLibError {
    if e.is_timeout() {
        AgentLibError::Timeout(format!("{}: {}", url, e))
    } else if e.is_connect() {
        AgentLibError::Network(format!("connect failed for {}: {}", url, e))
    } else {
        AgentLibError::Network(format!("{}: {}", url, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    /// 测试用 guard：按 URL 谓词逐跳裁决。
    struct PredicateGuard<F: Fn(&Url) -> bool + Send + Sync> {
        f: F,
    }

    impl<F: Fn(&Url) -> bool + Send + Sync> EgressGuard for PredicateGuard<F> {
        fn allow(&self, url: &Url) -> bool {
            (self.f)(url)
        }
    }

    fn allow_all() -> Arc<dyn EgressGuard> {
        Arc::new(PredicateGuard { f: |_| true })
    }

    fn deny_all() -> Arc<dyn EgressGuard> {
        Arc::new(PredicateGuard { f: |_| false })
    }

    fn deny_path(path: &'static str) -> Arc<dyn EgressGuard> {
        Arc::new(PredicateGuard {
            f: move |u: &Url| u.path() != path,
        })
    }

    const TEST_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
  <title>Test Page Title</title>
</head>
<body>
  <article>
    <h1>Main Heading</h1>
    <p>This is the first paragraph of meaningful content for extraction.</p>
    <p>This is the second paragraph with more detail.</p>
  </article>
</body>
</html>"#;

    #[tokio::test]
    async fn fetch_invalid_url_returns_error_not_panic() {
        let err = fetch("not a url", &FetchOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(err, AgentLibError::InvalidUrl(_)));
    }

    #[tokio::test]
    async fn fetch_unsupported_scheme_returns_invalid_url() {
        let err = fetch("file:///etc/passwd", &FetchOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(err, AgentLibError::InvalidUrl(_)));
    }

    #[tokio::test]
    async fn fetch_known_html_produces_markdown() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/article"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(TEST_HTML, "text/html"))
            .mount(&server)
            .await;

        let opts = FetchOptions {
            egress: Some(allow_all()),
            ..Default::default()
        };
        let content = fetch(&format!("{}/article", server.uri()), &opts)
            .await
            .expect("fetch should succeed");
        assert!(
            !content.markdown.trim().is_empty(),
            "markdown should be non-empty"
        );
        assert!(
            content.markdown.contains("Main Heading"),
            "markdown should contain the title text: {}",
            content.markdown
        );
    }

    #[tokio::test]
    async fn fetch_peer_addr_backfilled() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/page"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(TEST_HTML, "text/html"))
            .mount(&server)
            .await;

        let opts = FetchOptions {
            egress: Some(allow_all()),
            ..Default::default()
        };
        let content = fetch(&format!("{}/page", server.uri()), &opts)
            .await
            .expect("fetch should succeed");
        assert!(
            content.peer_addr.is_some(),
            "peer_addr should be backfilled from connection metadata"
        );
        assert!(content.peer_addr.as_deref().unwrap().contains(':'));
    }

    #[tokio::test]
    async fn fetch_egress_deny_returns_egress_denied_before_connection() {
        // guard deny：应在请求前终止（未发起连接），返回 EgressDenied。
        // mock 的 expect(0) 断言第二跳未真正发起请求。
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/never"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(TEST_HTML, "text/html"))
            .expect(0)
            .mount(&server)
            .await;

        let opts = FetchOptions {
            egress: Some(deny_all()),
            ..Default::default()
        };
        let err = fetch(&format!("{}/never", server.uri()), &opts)
            .await
            .unwrap_err();
        assert!(matches!(err, AgentLibError::EgressDenied { .. }));
    }

    #[tokio::test]
    async fn fetch_302_redirect_to_denied_target_is_rejected_on_second_hop() {
        let server = wiremock::MockServer::start().await;
        // 第一跳允许，302 指向 /blocked 路径，guard 拒绝第二跳
        wiremock::Mock::given(wiremock::matchers::path("/start"))
            .respond_with(
                wiremock::ResponseTemplate::new(302).insert_header("Location", "/blocked"),
            )
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::path("/blocked"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(TEST_HTML, "text/html"))
            .expect(0) // 第二跳不应真正发起请求
            .mount(&server)
            .await;

        let opts = FetchOptions {
            egress: Some(deny_path("/blocked")),
            ..Default::default()
        };
        let err = fetch(&format!("{}/start", server.uri()), &opts)
            .await
            .unwrap_err();
        assert!(matches!(err, AgentLibError::EgressDenied { .. }));
    }

    #[tokio::test]
    async fn fetch_302_redirect_followed_to_final_content() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/redirect"))
            .respond_with(wiremock::ResponseTemplate::new(302).insert_header("Location", "/final"))
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::path("/final"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(TEST_HTML, "text/html"))
            .mount(&server)
            .await;

        let opts = FetchOptions {
            egress: Some(allow_all()),
            ..Default::default()
        };
        let content = fetch(&format!("{}/redirect", server.uri()), &opts)
            .await
            .expect("redirect should be followed");
        assert!(content.resolved_url.ends_with("/final"));
        assert!(content.markdown.contains("Main Heading"));
    }

    #[tokio::test]
    async fn fetch_follow_redirects_false_returns_http_status() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/redirect"))
            .respond_with(wiremock::ResponseTemplate::new(302).insert_header("Location", "/final"))
            .mount(&server)
            .await;

        let opts = FetchOptions {
            egress: Some(allow_all()),
            follow_redirects: false,
            ..Default::default()
        };
        let err = fetch(&format!("{}/redirect", server.uri()), &opts)
            .await
            .unwrap_err();
        assert!(matches!(err, AgentLibError::HttpStatus { status: 302, .. }));
    }

    #[tokio::test]
    async fn fetch_max_bytes_exceeded() {
        let server = wiremock::MockServer::start().await;
        let big_body = "x".repeat(8192);
        wiremock::Mock::given(wiremock::matchers::path("/big"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_raw(big_body.as_str(), "text/plain"),
            )
            .mount(&server)
            .await;

        let opts = FetchOptions {
            egress: Some(allow_all()),
            max_bytes: 1024,
            ..Default::default()
        };
        let err = fetch(&format!("{}/big", server.uri()), &opts)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AgentLibError::MaxBytesExceeded { max_bytes: 1024 }
        ));
    }

    #[tokio::test]
    async fn fetch_http_error_status() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/notfound"))
            .respond_with(wiremock::ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let opts = FetchOptions {
            egress: Some(allow_all()),
            ..Default::default()
        };
        let err = fetch(&format!("{}/notfound", server.uri()), &opts)
            .await
            .unwrap_err();
        assert!(matches!(err, AgentLibError::HttpStatus { status: 404, .. }));
    }

    #[tokio::test]
    async fn fetch_egress_none_runs_crawlrs_ssrf_validation() {
        // egress=None 时 crawlrs 独立执行 SSRF 校验（与平台 validate_url 行为一致）。
        // 使用明确被拦截的私网/回环地址，不依赖 wiremock 回环行为——
        // 校验必须在发起连接前拒绝，无需真实对端。
        for url in ["http://10.0.0.1/secret", "http://127.0.0.1:9/page"] {
            let err = fetch(url, &FetchOptions::default()).await.unwrap_err();
            assert!(
                matches!(err, AgentLibError::SsrfDenied { .. }),
                "private/loopback target {} must yield SsrfDenied",
                url
            );
        }
    }

    #[tokio::test]
    async fn fetch_too_many_redirects() {
        let server = wiremock::MockServer::start().await;
        // 自环重定向：/loop -> /loop
        wiremock::Mock::given(wiremock::matchers::path("/loop"))
            .respond_with(wiremock::ResponseTemplate::new(302).insert_header("Location", "/loop"))
            .mount(&server)
            .await;

        let opts = FetchOptions {
            egress: Some(allow_all()),
            ..Default::default()
        };
        let err = fetch(&format!("{}/loop", server.uri()), &opts)
            .await
            .unwrap_err();
        assert!(matches!(err, AgentLibError::TooManyRedirects { .. }));
    }

    #[tokio::test]
    async fn fetch_timeout_returns_timeout_error() {
        let server = wiremock::MockServer::start().await;
        // 服务端延迟 5s，客户端超时 50ms → 必然触发超时
        wiremock::Mock::given(wiremock::matchers::path("/slow"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_secs(5))
                    .set_body_raw(TEST_HTML, "text/html"),
            )
            .mount(&server)
            .await;

        let opts = FetchOptions {
            egress: Some(allow_all()),
            timeout: Duration::from_millis(50),
            ..Default::default()
        };
        let err = fetch(&format!("{}/slow", server.uri()), &opts)
            .await
            .unwrap_err();
        assert!(matches!(err, AgentLibError::Timeout(_)), "got {:?}", err);
    }
}
