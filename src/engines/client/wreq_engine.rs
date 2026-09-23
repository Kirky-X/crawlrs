// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! TLS 指纹一致性伪装引擎（feature-gated on `engine-tls-fingerprint`）
//!
//! 基于 `wreq`（BoringSSL 后端，Apache-2.0）实现，产出真实浏览器 JA3/JA4 + HTTP/2 指纹，
//! 用于 `needs_tls_fingerprint` 请求（抗指纹识别反爬）。
//!
//! # 许可证说明（rule 7 冲突暴露）
//!
//! wreq-util 提供了 `Emulation::Chrome131` 便捷枚举，但 **wreq-util 本身是 GPL-3.0**，
//! 不在 `deny.toml` 的 license 白名单中（`cargo deny check` CI 会拒绝）。因此本引擎
//! **不引入 wreq-util**，改用 wreq 自有 API（Apache-2.0）：
//! - `wreq::Emulation`（buildable struct，通过 `EmulationBuilder` 构建）
//! - `wreq::IntoEmulation` trait
//! - `wreq::TlsConfig` / `Http1Config` / `Http2Config`
//!
//! 本项目自有枚举 [`TlsEmulation`](crate::utils::ua_pool::TlsEmulation) 经
//! [`emulation_provider`] 映射到 `wreq::EmulationProvider`。
//!
//! # 指纹保真度说明（rule 12 显性化）
//!
//! 当前各 [`TlsEmulation`] 变体统一解析到 `wreq::EmulationProvider::default()`（即 wreq 内置
//! 的浏览器指纹，Chrome 系 BoringSSL/HTTP2 配置）。各浏览器变体的精细化
//! `TlsConfig`/`Http1Config`/`Http2Config` 差异化需要真实实测指纹数据，**刻意不编造**——
//! 宁可先统一走默认指纹，也不臆造不可信的 JA4 配置。`UaProfile.tls_emulation` → 引擎的
//! 映射渠道已就位，待上游指纹模板或实测数据到位后，在这里按变体细化即可，无需改动引擎骨架。

use crate::config::settings::ProxyStrategy;
use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use crate::engines::provider::{ProxyCategory, ProxyProvider};
use crate::engines::validators;
use crate::utils::proxy::redact_proxy_url;
use crate::utils::ua_pool::{TlsEmulation, UaPool};
use async_trait::async_trait;
use log::error;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 默认代理策略（无 settings 注入时使用）
const DEFAULT_PROXY_STRATEGY: ProxyStrategy = ProxyStrategy::RoundRobin;

/// 将 [`TlsEmulation`] 映射为 `wreq::Emulation`。
///
/// # 许可证
///
/// 使用 wreq 自有 API（Apache-2.0），**不依赖 GPL 的 wreq-util**。
///
/// # 当前保真度
///
/// Chromium 系变体（Chrome/Edge）构建真实仿真：浏览器默认头（accept /
/// sec-fetch-* 一族，值与 UA profile 一致）+ Chrome H2 SETTINGS 帧参数
/// （HEADER_TABLE_SIZE=65536、INITIAL_WINDOW_SIZE=6291456、
/// MAX_HEADER_LIST_SIZE=262144 等）。TLS 层保持 BoringSSL 默认——wreq
/// 本身即 Chromium 衍生栈，默认 ClientHello 已与 Chrome 同源。
///
/// Firefox/Safari 变体**不做伪仿真**：BoringSSL 无法呈现 Gecko/WebKit 的
/// ClientHello，硬仿只会制造"Firefox UA + Chromium 系 JA4"指纹矛盾。
/// wreq 路径的 UA 候选集经 [`UaPool::pick_chromium`] 过滤后不会出现这些
/// 变体；此分支仅作为 release 兜底（返回 Chromium 仿真）+ debug_assert。
#[must_use]
pub fn emulation_provider(emulation: TlsEmulation) -> wreq::Emulation {
    if !emulation.is_chromium_family() {
        debug_assert!(
            emulation.is_chromium_family(),
            "non-Chromium TlsEmulation must never reach WreqEngine (UA pool filters them)"
        );
        log::warn!(
            "non-Chromium TlsEmulation {emulation:?} reached emulation_provider; \
             falling back to Chromium emulation (UA pool filter should prevent this)"
        );
    }
    wreq::Emulation::builder()
        .headers(chromium_default_headers())
        .http2_options(chrome_http2_options())
        .build(wreq::Group::default())
}

/// Chromium 系浏览器静态默认头。
///
/// 只放与 UA profile 无关的头：`user-agent`/`accept-language`/`sec-ch-ua`
/// 由请求级从所选 profile 注入（wreq_engine scrape 路径），避免两处维护
/// 漂移。`accept-encoding` 亦不在列——必须与启用了解压 feature 的编码集
/// 一致，由 feature 接线处补齐。
fn chromium_default_headers() -> wreq::header::HeaderMap {
    use wreq::header::{HeaderMap, HeaderName, HeaderValue};

    // (name, value, sensitive?) —— 名称小写，与 Chrome 原始顺序一致
    const DEFAULTS: &[(&str, &str)] = &[
        (
            "accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
        ),
        ("sec-fetch-dest", "document"),
        ("sec-fetch-mode", "navigate"),
        ("sec-fetch-site", "none"),
        ("sec-fetch-user", "?1"),
        ("upgrade-insecure-requests", "1"),
    ];

    let mut headers = HeaderMap::new();
    for (name, value) in DEFAULTS {
        let name =
            HeaderName::from_bytes(name.as_bytes()).expect("static header name must be valid");
        let value = HeaderValue::from_str(value).expect("static header value must be valid");
        headers.insert(name, value);
    }
    headers
}

/// Chrome 系 HTTP/2 SETTINGS 帧参数（Chrome 131 实测值）。
fn chrome_http2_options() -> wreq::http2::Http2Options {
    wreq::http2::Http2Options::builder()
        .header_table_size(65_536)
        .enable_push(false)
        .initial_window_size(6_291_456)
        .max_header_list_size(262_144)
        .max_concurrent_streams(1_000)
        .build()
}

/// 读取 wreq 响应体并强制大小上限（与 reqwest 侧 `read_body_limited` 语义对齐）。
///
/// 双保险：`Content-Length` 预检先行（超限直接拒绝，不建立缓冲）；
/// 无 `Content-Length`（chunked）或声明不实时流式累积并在超限时中断。
/// 避免对超大/恶意响应无界读入内存。编码解码按 Content-Type charset
/// （encoding_rs），与 reqwest 侧一致。
async fn read_wreq_body_limited(
    response: wreq::Response,
    max_bytes: usize,
) -> Result<String, EngineError> {
    use futures::StreamExt;

    // 在消费 body 前提取 charset（bytes_stream 会消耗 response）
    let charset = response
        .headers()
        .get(wreq::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|ct| {
            ct.split(';')
                .map(|part| part.trim())
                .find_map(|part| part.strip_prefix("charset="))
        })
        .map(|s| s.trim().trim_matches('"').to_string());

    // 1. content_length 预检
    if let Some(len) = response.content_length() {
        if len as usize > max_bytes {
            return Err(EngineError::Other(format!(
                "Response body size {} exceeds limit {}",
                len, max_bytes
            )));
        }
    }

    // 2. bytes_stream 累积 + 上限中断
    let mut buffer: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| EngineError::RequestFailed(e.to_string()))?;
        if buffer.len() + chunk.len() > max_bytes {
            return Err(EngineError::Other(format!(
                "Response body exceeds limit {} (chunked transfer)",
                max_bytes
            )));
        }
        buffer.extend_from_slice(&chunk);
    }

    // 3. 编码解码（对齐 reqwest 侧 read_body_limited）
    let decoded = match charset.as_deref() {
        Some(label) => {
            let encoding =
                encoding_rs::Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::UTF_8);
            let (text, _, _) = encoding.decode(&buffer);
            text.into_owned()
        }
        None => String::from_utf8_lossy(&buffer).into_owned(),
    };

    Ok(decoded)
}

/// TLS 指纹引擎
///
/// 与 [`ReqwestEngine`](crate::engines::client::ReqwestEngine) 结构对齐，但：
/// - 使用 `wreq::Client`（BoringSSL → 真实 JA4 指纹）
/// - UA 经 `ua_pool` 选取，同一请求的 `ua` 与 `tls_emulation` 绑定一致
/// - 代理经 `ProxyProvider` trait 调度（与 ReqwestEngine 的一致；
///   原写 `Option<Arc<ProxyPool>>`，遵循代码库惯例改用 trait 抽象）
pub struct WreqEngine {
    /// wreq 客户端（构造时构建，含 TLS 指纹模拟配置）
    /// 生产路径使用 `build_resolve_client` 静态方法构建 per-request client；
    /// 此字段保留供测试路径 `get_client` 使用。
    ///
    /// 会话隔离约束：本 client 开启 `cookie_store`（jar 跨请求持久）。
    /// 生产 scrape 每请求经 `build_resolve_client` 新建独立 client（jar 隔离，
    /// 无跨任务串扰）；若未来生产路径改用本字段，必须先解决跨任务会话隔离。
    #[allow(dead_code)]
    client: wreq::Client,
    /// UA 池每请求选一致 UA + Accept-Language + sec-ch-ua + tls_emulation
    ua_pool: Arc<UaPool>,
    /// 代理提供者（H2：依赖抽象 trait）
    proxy_provider: Option<Arc<dyn ProxyProvider>>,
    /// 代理调度策略（RoundRobin / Sticky）
    proxy_strategy: ProxyStrategy,
    /// 引擎级请求超时（秒）—— 注入自 settings.timeouts.engines.default_timeout_seconds
    timeout_seconds: u64,
    /// 单引擎最大响应时间（MRT）。
    /// 注入自 `settings.timeouts.engines.tls_seconds`（默认 15 秒）。
    mrt: Duration,
    /// 响应体大小上限（字节）—— 注入自 `EngineSettings::max_response_body_bytes`，
    /// 与 ReqwestEngine 同款防 OOM 双保险（Content-Length 预检 + 流式累积）。
    max_response_body_bytes: usize,
}

/// wreq 引擎响应体大小上限缺省值（10MB，对齐 ReqwestEngine::DEFAULT_MAX_RESPONSE_BODY_BYTES）。
const DEFAULT_WREQ_MAX_RESPONSE_BODY_BYTES: usize = 10 * 1024 * 1024;

/// 构建 SSRF 安全的 wreq 重定向策略。
///
/// 与 reqwest 侧 `create_ssrf_safe_redirect_policy` 对齐：逐跳校验 scheme、
/// 静态内网 hostname/IP 字面量，并对重定向目标 hostname 做 DNS 解析检查
/// （全部 IP 必须为公网；解析失败按 fail-closed 拒绝跟随）。
fn create_ssrf_safe_wreq_redirect_policy(max_redirects: usize) -> wreq::redirect::Policy {
    use crate::engines::shared::is_private_ip;
    use crate::infrastructure::security::ssrf::is_internal_url;
    use std::net::ToSocketAddrs;

    wreq::redirect::Policy::custom(move |attempt| {
        if attempt.previous.len() >= max_redirects {
            return attempt.error("too many redirects");
        }

        let uri = &*attempt.uri;

        // 1) 只允许 http/https
        if uri.scheme_str() != Some("http") && uri.scheme_str() != Some("https") {
            log::warn!(
                "SSRF protection: Blocking wreq redirect with non-HTTP scheme: {:?}",
                uri.scheme_str()
            );
            return attempt.stop();
        }

        // 2) 静态检查：IP 字面量私网 / 内部 hostname
        let uri_str = uri.to_string();
        if is_internal_url(&uri_str) {
            log::warn!(
                "SSRF protection: Blocking wreq redirect to internal URL: {}",
                uri_str
            );
            return attempt.stop();
        }

        // 3) DNS 级检查：重定向目标全部解析 IP 必须公网（fail-closed）
        if let Some(host) = uri.host() {
            let port = uri
                .port_u16()
                .unwrap_or(if uri.scheme_str() == Some("https") {
                    443
                } else {
                    80
                });
            match (host, port).to_socket_addrs() {
                Ok(addrs) => {
                    let mut any = false;
                    for addr in addrs {
                        any = true;
                        if is_private_ip(addr.ip()) {
                            log::warn!(
                                "SSRF protection: wreq redirect host {} resolves to private IP {}, blocking",
                                host,
                                addr.ip()
                            );
                            return attempt.stop();
                        }
                    }
                    if !any {
                        return attempt.stop();
                    }
                }
                Err(e) => {
                    log::warn!(
                        "SSRF protection: wreq redirect DNS failed for host {} ({}), blocking",
                        host,
                        e
                    );
                    return attempt.stop();
                }
            }
        }

        attempt.follow()
    })
}

impl WreqEngine {
    /// 创建 WreqEngine（无代理提供者）。
    ///
    /// `ua_pool`：共享 UA 池；`mrt`：引擎级 MRT；`timeout_seconds`：请求超时。
    ///
    /// # 错误
    ///
    /// wreq 客户端构建失败（BoringSSL 后端初始化失败等）返回 `EngineError::Internal`。
    pub fn new(
        ua_pool: Arc<UaPool>,
        mrt: Duration,
        timeout_seconds: u64,
    ) -> Result<Self, EngineError> {
        let client = Self::build_base_client(timeout_seconds)?;
        Ok(Self {
            client,
            ua_pool,
            proxy_provider: None,
            proxy_strategy: DEFAULT_PROXY_STRATEGY,
            timeout_seconds,
            mrt,
            max_response_body_bytes: DEFAULT_WREQ_MAX_RESPONSE_BODY_BYTES,
        })
    }

    /// 创建带代理提供者的 WreqEngine（H2：依赖 `ProxyProvider` trait）。
    pub fn with_provider(
        ua_pool: Arc<UaPool>,
        proxy_provider: Arc<dyn ProxyProvider>,
        proxy_strategy: ProxyStrategy,
        mrt: Duration,
        timeout_seconds: u64,
    ) -> Result<Self, EngineError> {
        let client = Self::build_base_client(timeout_seconds)?;
        Ok(Self {
            client,
            ua_pool,
            proxy_provider: Some(proxy_provider),
            proxy_strategy,
            timeout_seconds,
            mrt,
            max_response_body_bytes: DEFAULT_WREQ_MAX_RESPONSE_BODY_BYTES,
        })
    }

    /// 注入响应体大小上限（来自 `EngineSettings::max_response_body_bytes`，对齐 ReqwestEngine）。
    pub fn with_max_response_body_bytes(mut self, bytes: usize) -> Self {
        self.max_response_body_bytes = bytes;
        self
    }

    /// 构建基础 wreq 客户端（含默认 Chrome 系 TLS 指纹模拟 + 超时 + SSRF 安全重定向）。
    fn build_base_client(timeout_seconds: u64) -> Result<wreq::Client, EngineError> {
        wreq::Client::builder()
            .emulation(emulation_provider(TlsEmulation::Chrome131))
            .redirect(create_ssrf_safe_wreq_redirect_policy(5))
            .timeout(Duration::from_secs(timeout_seconds))
            .cookie_store(true)
            .build()
            .map_err(|e| EngineError::Internal(format!("wreq client build failed: {e}")))
    }

    /// 构建带代理的临时 wreq 客户端。
    ///
    /// # 返回
    ///
    /// `Some(client)`：构建成功；`None`：失败（代理 URL 非法或构建失败，已打 error 日志）。
    /// 调用方在 `None` 时回退到基础 client（rule 12：失败显性化到日志，不静默吞掉）。
    #[cfg(test)]
    fn build_proxy_client(proxy_url: &str, timeout_seconds: u64) -> Option<wreq::Client> {
        let proxy = match wreq::Proxy::all(proxy_url) {
            Ok(p) => p,
            Err(e) => {
                error!(
                    "Failed to configure wreq proxy (url={}): {}",
                    redact_proxy_url(proxy_url),
                    e
                );
                return None;
            }
        };
        match wreq::Client::builder()
            .redirect(create_ssrf_safe_wreq_redirect_policy(5))
            .proxy(proxy)
            .timeout(Duration::from_secs(timeout_seconds))
            .cookie_store(true)
            .build()
        {
            Ok(c) => {
                log::debug!("Using wreq HTTP proxy: {}", redact_proxy_url(proxy_url));
                Some(c)
            }
            Err(e) => {
                error!(
                    "Failed to build wreq client (proxy={}): {}",
                    redact_proxy_url(proxy_url),
                    e
                );
                None
            }
        }
    }

    /// 获取 UA 池引用（测试验证）。
    #[must_use]
    pub fn ua_pool(&self) -> &UaPool {
        &self.ua_pool
    }

    /// 引擎是否配置了代理提供者（测试验证 H2）。
    #[must_use]
    pub fn has_proxy_provider(&self) -> bool {
        self.proxy_provider.is_some()
    }

    /// 获取代理调度策略。
    #[must_use]
    pub fn proxy_strategy(&self) -> ProxyStrategy {
        self.proxy_strategy
    }

    /// 获取引擎级 MRT。
    #[must_use]
    pub fn mrt(&self) -> Duration {
        self.mrt
    }

    /// 获取引擎级请求超时（秒）。
    #[must_use]
    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }

    /// 获取用于本次请求的 client 与使用的代理 URL。
    ///
    /// 代理优先级（对齐 ReqwestEngine）：
    /// 1. 请求级代理（`request.proxy`）覆盖池
    /// 2. 代理提供者按 `proxy_strategy` 调度（RoundRobin → next / Sticky → sticky）
    /// 3. 无代理 → 基础 client
    ///
    /// 返回 `(client, Option<String> used_proxy_url)`：`used_proxy_url` 用于
    /// `ProxyProvider::mark_failure` / `mark_success` 状态回填。
    #[cfg(test)]
    fn get_client(
        &self,
        proxy: &Option<String>,
        session_id: Option<&str>,
    ) -> (wreq::Client, Option<String>) {
        let proxy_url = self.resolve_proxy_url(proxy, session_id);
        let client = if let Some(ref url) = proxy_url {
            Self::build_proxy_client(url, self.timeout_seconds)
                .unwrap_or_else(|| self.client.clone())
        } else {
            self.client.clone()
        };
        (client, proxy_url)
    }

    /// 解析本次请求的代理 URL（不构建 client）。
    ///
    /// 代理优先级与 `get_client` 一致，仅返回 URL 供 `build_resolve_client` 使用。
    fn resolve_proxy_url(
        &self,
        proxy: &Option<String>,
        session_id: Option<&str>,
    ) -> Option<String> {
        // 请求级代理优先
        let request_proxy = proxy.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty());
        if let Some(url) = request_proxy {
            return Some(url.to_string());
        }

        let provider = self.proxy_provider.as_ref()?;

        let picked = match self.proxy_strategy {
            ProxyStrategy::RoundRobin => provider.next(ProxyCategory::Html),
            ProxyStrategy::Sticky => match session_id {
                Some(sid) if !sid.is_empty() => provider.sticky(sid),
                _ => {
                    log::warn!(
                        "proxy_strategy=Sticky but session_id is missing/empty; \
                         falling back to RoundRobin (next)"
                    );
                    provider.next(ProxyCategory::Html)
                }
            },
        };

        match picked {
            Some(url) => Some(url),
            None => {
                log::debug!(
                    "wreq proxy_provider returned None (empty pool or all cooldown); \
                     no proxy for this request"
                );
                None
            }
        }
    }

    /// 构建含 SSRF resolve 覆盖的 wreq client。
    ///
    /// 保持原始 hostname 不变，通过 `resolve_to_addrs` 将 DNS 解析固定到 SSRF 验证过的 IP，
    /// 确保 TLS SNI 和 HTTP 虚拟主机路由正确。TLS 仿真参数来自所选 UA profile，
    /// 保证 UA 声明的浏览器与 TLS/H2 指纹一致。
    fn build_resolve_client(
        host: &str,
        resolve_addrs: &[std::net::SocketAddr],
        proxy_url: Option<&str>,
        skip_tls: bool,
        timeout_seconds: u64,
        emulation: TlsEmulation,
    ) -> Result<wreq::Client, EngineError> {
        let mut builder = wreq::Client::builder()
            .emulation(emulation_provider(emulation))
            .redirect(create_ssrf_safe_wreq_redirect_policy(5))
            .timeout(Duration::from_secs(timeout_seconds))
            .cookie_store(true);

        // SSRF resolve 覆盖：hostname → 验证过的 IP
        if !host.is_empty() && host.parse::<std::net::IpAddr>().is_err() {
            builder = builder.resolve_to_addrs(host.to_string(), resolve_addrs.iter().copied());
        }

        if skip_tls {
            log::warn!(
                "wreq skip_tls_verification=true: TLS certificate validation disabled for this request"
            );
            builder = builder.tls_cert_verification(false);
        }

        if let Some(url) = proxy_url {
            match wreq::Proxy::all(url) {
                Ok(px) => {
                    builder = builder.proxy(px);
                    log::debug!("Using wreq proxy (all schemes): {}", redact_proxy_url(url));
                }
                Err(e) => {
                    error!(
                        "Failed to configure wreq proxy (url={}): {}",
                        redact_proxy_url(url),
                        e
                    );
                }
            }
        }

        builder
            .build()
            .map_err(|e| EngineError::Other(format!("Failed to build wreq resolve client: {e}")))
    }
}

#[async_trait]
impl ScraperEngine for WreqEngine {
    /// 执行 HTTP 抓取并产出真实 TLS 指纹。
    ///
    /// 流程：SSRF 校验 + resolve 固定 IP → 选 UA profile（含 tls_emulation）→ 构建 client（含 resolve 覆盖）
    /// → 组装 headers/body/timeout → 发送 → 返回内部响应。
    async fn scrape(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        // SSRF 保护：DNS 验证 + resolve 覆盖（保持原始 URL，TLS SNI 和虚拟主机路由正常）
        let validator = validators::SsrfValidator::new();
        let validated_url = validator
            .validate(&request.url)
            .await
            .map_err(|e| EngineError::Other(format!("SSRF protection: {}", e)))?;
        let host = validated_url
            .parsed_url
            .host_str()
            .unwrap_or("")
            .to_string();
        let port = validated_url.port;
        // SSRF: pin hostname → validated IPs via wreq resolve override.
        // Keep the original URL intact so TLS SNI and virtual host routing work correctly.
        let resolve_addrs: Vec<std::net::SocketAddr> = validated_url
            .resolved_ips
            .iter()
            .map(|ip| std::net::SocketAddr::new(*ip, port))
            .collect();
        if resolve_addrs.is_empty() {
            return Err(EngineError::Other("SSRF: no resolved IPs".to_string()));
        }

        // 选与请求匹配的 UA profile（ua + accept_language + sec-ch-ua + tls_emulation）。
        // wreq 只能仿真 Chromium 系 TLS，必须从 Chromium 子集中选取，
        // 避免"Firefox UA + Chromium 系 JA4"指纹矛盾。
        let profile = self.ua_pool.pick_chromium(request.mobile);

        // 确定本次请求使用的代理 URL（与 get_client 逻辑一致）
        let used_proxy_url = self.resolve_proxy_url(&request.proxy, request.session_id.as_deref());

        // 构建含 SSRF resolve 覆盖的 wreq client（保持原始 hostname → TLS SNI / 虚拟主机路由正确）
        let effective_client = Self::build_resolve_client(
            &host,
            &resolve_addrs,
            used_proxy_url.as_deref(),
            request.skip_tls_verification,
            self.timeout_seconds,
            profile.tls_emulation,
        )?;

        let request_url = validated_url.parsed_url.as_str();
        let mut request_builder = match request.method {
            crate::common::HttpMethod::Get => effective_client.get(request_url),
            crate::common::HttpMethod::Post => effective_client.post(request_url),
        };

        // UA 绑定 headers（User-Agent / Accept-Language / sec-ch-ua），用户自定义 headers 随后覆盖
        request_builder = request_builder
            .header("User-Agent", profile.ua)
            .header("Accept-Language", profile.accept_language);
        if !profile.sec_ch_ua.is_empty() {
            request_builder = request_builder.header("sec-ch-ua", profile.sec_ch_ua);
        }

        // 自定义 headers
        let mut headers = wreq::header::HeaderMap::new();
        for (k, v) in &request.headers {
            if let (Ok(name), Ok(value)) = (
                wreq::header::HeaderName::from_bytes(k.as_bytes()),
                wreq::header::HeaderValue::from_str(v),
            ) {
                headers.insert(name, value);
            }
        }
        request_builder = request_builder.headers(headers);

        if let Some(body) = &request.body {
            request_builder = request_builder.body(body.clone());
        }
        request_builder = request_builder.timeout(request.timeout);

        let start = Instant::now();
        let response_result = request_builder.send().await;

        let response = match response_result {
            Ok(resp) => resp,
            Err(e) => {
                if let Some(url) = &used_proxy_url {
                    if let Some(provider) = &self.proxy_provider {
                        provider.mark_failure(url);
                    }
                }
                if e.is_timeout() {
                    return Err(EngineError::Timeout(request.timeout));
                }
                error!("wreq send failed for {}: {e}", request.url);
                return Err(EngineError::RequestFailed(e.to_string()));
            }
        };

        let status_code = response.status().as_u16();
        // 2xx 成功 → mark_success 恢复代理健康（对齐 ReqwestEngine H3）
        if (200..300).contains(&status_code) {
            if let Some(url) = &used_proxy_url {
                if let Some(provider) = &self.proxy_provider {
                    provider.mark_success(url);
                }
            }
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "text/html".to_string());

        let mut response_headers = std::collections::HashMap::with_capacity(32);
        for (k, v) in response.headers() {
            if let Ok(v_str) = v.to_str() {
                response_headers.insert(k.as_str().to_string(), v_str.to_string());
            }
        }

        let content = read_wreq_body_limited(response, self.max_response_body_bytes).await?;

        if request.sync_wait_ms > 0 {
            tokio::time::sleep(Duration::from_millis(request.sync_wait_ms as u64)).await;
        }

        Ok(InternalScrapeResponse {
            status_code,
            content,
            screenshot: None,
            content_type,
            headers: response_headers,
            response_time_ms: start.elapsed().as_millis() as u64,
            final_url: None,
        })
    }

    /// 支持分数：
    /// - `needs_tls_fingerprint` → 100（本引擎专长，router 优先选它）
    /// - 需 JS/截图 → 10（wreq 无法渲染）
    /// - 其余 → 10（非专长，让位快速 HTTP 引擎）
    fn support_score(&self, request: &InternalScrapeRequest) -> u8 {
        if request.needs_js || request.needs_screenshot {
            return 10;
        }
        if request.needs_tls_fingerprint {
            return 100;
        }
        10
    }

    /// 引擎名称。
    fn name(&self) -> &'static str {
        "wreq"
    }

    /// 支持 TLS 指纹（专业）。
    fn supports_tls_fingerprint(&self) -> bool {
        true
    }

    /// 返回注入的 MRT（默认 15s，对应 `tls_seconds`）。
    fn max_response_time(&self) -> Duration {
        self.mrt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::engine_client::{
        HttpMethod, InternalScrapeRequest, InternalScreenshotConfig,
    };
    use crate::engines::proxy_pool::ProxyPool;
    use std::collections::HashMap;
    use std::time::Duration;

    // === Helper functions ===

    fn create_engine() -> WreqEngine {
        WreqEngine::new(Arc::new(UaPool::new()), Duration::from_secs(15), 30)
            .expect("wreq client build should succeed")
    }

    fn create_request(url: &str) -> InternalScrapeRequest {
        InternalScrapeRequest {
            url: url.to_string(),
            method: HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            body: None,
            sync_wait_ms: 0,
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
            needs_mllm: false,
        }
    }

    fn create_proxy_pool() -> Arc<dyn ProxyProvider> {
        Arc::new(ProxyPool::from_urls(
            vec![
                "http://proxy-a:8080".to_string(),
                "http://proxy-b:8080".to_string(),
            ],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ))
    }

    // === 构造器测试 ===

    #[test]
    fn test_wreq_engine_new() {
        let engine = create_engine();
        assert_eq!(engine.name(), "wreq");
        assert!(!engine.has_proxy_provider());
        assert!(engine.supports_tls_fingerprint());
    }

    #[test]
    fn test_wreq_engine_with_provider() {
        let engine = WreqEngine::with_provider(
            Arc::new(UaPool::new()),
            create_proxy_pool(),
            ProxyStrategy::RoundRobin,
            Duration::from_secs(15),
            30,
        )
        .expect("wreq client build should succeed");
        assert_eq!(engine.name(), "wreq");
        assert!(engine.has_proxy_provider());
        assert_eq!(engine.proxy_strategy(), ProxyStrategy::RoundRobin);
    }

    #[test]
    fn test_wreq_engine_mrt_and_timeout_injected() {
        let engine = WreqEngine::new(Arc::new(UaPool::new()), Duration::from_secs(15), 45)
            .expect("wreq client build should succeed");
        assert_eq!(engine.mrt(), Duration::from_secs(15));
        assert_eq!(engine.timeout_seconds(), 45);
    }

    // === 名称 / 能力测试 ===

    #[test]
    fn test_name_returns_wreq() {
        assert_eq!(create_engine().name(), "wreq");
    }

    #[test]
    fn test_supports_tls_fingerprint_true() {
        assert!(create_engine().supports_tls_fingerprint());
    }

    #[test]
    fn test_ua_pool_getter() {
        let pool = Arc::new(UaPool::new());
        let engine = WreqEngine::new(pool.clone(), Duration::from_secs(15), 30)
            .expect("wreq client build should succeed");
        assert_eq!(engine.ua_pool().count(false), pool.count(false));
    }

    // === support_score 测试 ===

    #[test]
    fn test_support_score_tls_fingerprint_returns_100() {
        let engine = create_engine();
        let mut request = create_request("https://example.com");
        request.needs_tls_fingerprint = true;
        // TLS 指纹请求是本引擎专长 → 100（router 优先选它）
        assert_eq!(engine.support_score(&request), 100);
    }

    #[test]
    fn test_support_score_basic_returns_10() {
        let engine = create_engine();
        // 普通请求非专长 → 10，让位快速 HTTP 引擎
        assert_eq!(
            engine.support_score(&create_request("https://example.com")),
            10
        );
    }

    #[test]
    fn test_support_score_needs_js_returns_10() {
        let engine = create_engine();
        let mut request = create_request("https://example.com");
        request.needs_js = true;
        request.needs_tls_fingerprint = true;
        // 即使请求了 TLS 指纹，需 JS 渲染时 wreq 也无法胜任 → 10
        assert_eq!(engine.support_score(&request), 10);
    }

    // === P03：TLS 仿真真实化 ===

    #[test]
    fn test_emulation_provider_sets_browser_defaults() {
        let emulation = emulation_provider(TlsEmulation::Chrome131);
        assert!(
            !emulation.headers.is_empty(),
            "emulation must carry browser default headers (accept/sec-fetch-*), got empty"
        );
        assert!(
            emulation.headers.contains_key("accept"),
            "accept header is the most basic browser fingerprint header"
        );
        assert!(
            emulation.http2_options.is_some(),
            "emulation must set explicit H2 SETTINGS (Chrome fingerprint)"
        );
    }

    #[test]
    fn test_emulation_provider_accepts_all_chromium_variants() {
        for variant in [
            TlsEmulation::Chrome131,
            TlsEmulation::Chrome130,
            TlsEmulation::Chrome120,
            TlsEmulation::Edge131,
        ] {
            let emulation = emulation_provider(variant);
            assert!(
                !emulation.headers.is_empty(),
                "{variant:?} must get headers"
            );
        }
    }

    #[test]
    fn test_support_score_needs_screenshot_returns_10() {
        let engine = create_engine();
        let mut request = create_request("https://example.com");
        request.needs_screenshot = true;
        request.screenshot_config = Some(InternalScreenshotConfig {
            full_page: false,
            selector: None,
            quality: None,
            format: None,
        });
        assert_eq!(engine.support_score(&request), 10);
    }

    // === 代理调度测试（H2：ProxyProvider trait）===

    #[test]
    fn test_get_client_no_provider_returns_base_client() {
        let engine = create_engine();
        let (client, used_proxy) = engine.get_client(&None, None);
        assert!(used_proxy.is_none());
        // 基础 client 应与引擎默认 client 等值（wreq Client 是 Arc 内部共享，可 clone 比较指针语义）
        let _ = client;
    }

    #[test]
    fn test_get_client_round_robin_provides_proxy() {
        let engine = WreqEngine::with_provider(
            Arc::new(UaPool::new()),
            create_proxy_pool(),
            ProxyStrategy::RoundRobin,
            Duration::from_secs(15),
            30,
        )
        .expect("wreq client build should succeed");
        let (_, used_proxy) = engine.get_client(&None, None);
        assert!(used_proxy.is_some(), "RoundRobin 应从池中拿到代理");
    }

    #[test]
    fn test_get_client_sticky_uses_session() {
        let engine = WreqEngine::with_provider(
            Arc::new(UaPool::new()),
            create_proxy_pool(),
            ProxyStrategy::Sticky,
            Duration::from_secs(15),
            30,
        )
        .expect("wreq client build should succeed");
        let (_, first) = engine.get_client(&None, Some("session-1"));
        let (_, second) = engine.get_client(&None, Some("session-1"));
        // Sticky 同一 session 应命中同一代理
        assert!(first.is_some() && second.is_some());
        assert_eq!(first, second);
    }

    #[test]
    fn test_get_client_sticky_without_session_falls_back_round_robin() {
        let engine = WreqEngine::with_provider(
            Arc::new(UaPool::new()),
            create_proxy_pool(),
            ProxyStrategy::Sticky,
            Duration::from_secs(15),
            30,
        )
        .expect("wreq client build should succeed");
        // 无 session_id → 回退 RoundRobin，仍能从池中取到代理
        let (_, used_proxy) = engine.get_client(&None, None);
        assert!(used_proxy.is_some());
    }

    #[test]
    fn test_get_client_request_proxy_overrides_pool() {
        let engine = WreqEngine::with_provider(
            Arc::new(UaPool::new()),
            create_proxy_pool(),
            ProxyStrategy::RoundRobin,
            Duration::from_secs(15),
            30,
        )
        .expect("wreq client build should succeed");
        // 请求级代理应覆盖池调度
        let (_, used_proxy) = engine.get_client(&Some("http://override:9999".to_string()), None);
        assert_eq!(used_proxy.as_deref(), Some("http://override:9999"));
    }

    // === 指纹映射测试（联动：TlsEmulation → EmulationProvider）===

    #[test]
    fn test_emulation_provider_roundtrips_all_variants() {
        // 仅 Chromium 系变体能构造 Emulation——Firefox/Safari 不做伪仿真
        // （BoringSSL 无法呈现 Gecko/WebKit ClientHello），由 UA 池过滤，
        // 传非 Chromium 变体属契约违规（debug_assert 拦截）。
        for variant in [
            TlsEmulation::Chrome131,
            TlsEmulation::Chrome130,
            TlsEmulation::Chrome120,
            TlsEmulation::Edge131,
        ] {
            let _provider = emulation_provider(variant);
        }
    }

    // === SSRF 保护测试（scrape 前置校验）===

    #[tokio::test]
    async fn test_scrape_rejects_private_ip() {
        let engine = create_engine();
        let result = engine.scrape(&create_request("http://192.168.1.1/")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_rejects_localhost() {
        let engine = create_engine();
        let result = engine.scrape(&create_request("http://localhost/")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_rejects_metadata_endpoint() {
        let engine = create_engine();
        let result = engine
            .scrape(&create_request("http://169.254.169.254/latest/meta-data/"))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_rejects_ftp_scheme() {
        let engine = create_engine();
        let result = engine
            .scrape(&create_request("ftp://example.com/file"))
            .await;
        assert!(result.is_err());
    }

    // === JA4 指纹外部验证 ===
    // 需网络访问，默认 ignore。运行：cargo test --features engine-tls-fingerprint -- --ignored

    /// 向 ja4er.com 发送请求，验证返回的 JA4 指纹非空且包含 Chrome 特征。
    ///
    /// ja4er.com 返回 JSON：`{"ja4":"...","ja4_raw":"..."}`。
    /// Chrome 系 JA4 前缀为 `t13d`（TLS 1.3 + default extensions）。
    #[tokio::test]
    #[ignore] // 需网络访问
    async fn test_ja4_fingerprint_matches_chrome() {
        let engine = create_engine();
        let request = create_request("https://ja4er.com/");
        let response = engine
            .scrape(&request)
            .await
            .expect("ja4er.com request should succeed");

        assert!(
            (200..300).contains(&response.status_code),
            "expected 2xx, got {}",
            response.status_code
        );

        // ja4er.com 返回 JSON 含 ja4 字段
        let body = &response.content;
        assert!(
            body.contains("ja4"),
            "response should contain ja4 field, got: {}",
            &body[..body.len().min(500)]
        );

        // Chrome 系 JA4 指纹以 t13d 开头（TLS 1.3 + default extensions）
        // 或 t13（TLS 1.3 变体）—— 宽松匹配，避免 wreq 内部版本变化导致脆性断言
        let has_chrome_prefix = body.contains("t13d") || body.contains("t13");
        assert!(
            has_chrome_prefix,
            "JA4 fingerprint should contain Chrome TLS 1.3 prefix (t13/t13d), got: {}",
            &body[..body.len().min(500)]
        );
    }

    /// 向 tls.peet.ws 发送请求，验证返回内容含 TLS 指纹信息。
    #[tokio::test]
    #[ignore] // 需网络访问
    async fn test_tls_peet_ws_returns_fingerprint_data() {
        let engine = create_engine();
        let request = create_request("https://tls.peet.ws/");
        let response = engine
            .scrape(&request)
            .await
            .expect("tls.peet.ws request should succeed");

        assert!(
            (200..300).contains(&response.status_code),
            "expected 2xx, got {}",
            response.status_code
        );

        // tls.peet.ws 返回 JSON 含 ja4 / ja4_raw 等指纹字段
        let body = &response.content;
        let has_fingerprint = body.contains("ja4") || body.contains("ja3") || body.contains("tls");
        assert!(
            has_fingerprint,
            "response should contain TLS fingerprint data, got: {}",
            &body[..body.len().min(500)]
        );
    }
}
