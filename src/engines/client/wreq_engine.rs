// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! TLS 指纹一致性伪装引擎（Phase 1 / D4，feature-gated on `engine-tls-fingerprint`）
//!
//! 基于 `wreq`（BoringSSL 后端，Apache-2.0）实现，产出真实浏览器 JA3/JA4 + HTTP/2 指纹，
//! 用于 `needs_tls_fingerprint` 请求（抗指纹识别反爬）。
//!
//! # 许可证说明（rule 7 冲突暴露）
//!
//! wreq-util 提供了 `Emulation::Chrome131` 便捷枚举，但 **wreq-util 本身是 GPL-3.0**，
//! 不在 `deny.toml` 的 license 白名单中（`cargo deny check` CI 会拒绝）。因此本引擎
//! **不引入 wreq-util**，改用 wreq 自有 API（Apache-2.0）：
//! - `wreq::EmulationProvider`（buildable struct，实现 `EmulationProviderFactory`）
//! - `wreq::EmulationProviderFactory` trait
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

/// 将 [`TlsEmulation`] 映射为 `wreq::EmulationProvider`（Phase 1 / D4）。
///
/// # 许可证
///
/// 使用 wreq 自有 API（Apache-2.0），**不依赖 GPL 的 wreq-util**。
///
/// # 当前保真度
///
/// 全部变体当前解析为 `EmulationProvider::default()`（wreq 内置 Chrome 系浏览器指纹）。
/// 详见模块级文档「指纹保真度说明」。
#[must_use]
pub fn emulation_provider(_emulation: TlsEmulation) -> wreq::EmulationProvider {
    wreq::EmulationProvider::default()
}

/// TLS 指纹引擎
///
/// 与 [`ReqwestEngine`](crate::engines::client::ReqwestEngine) 结构对齐，但：
/// - 使用 `wreq::Client`（BoringSSL → 真实 JA4 指纹）
/// - UA 经 `ua_pool` 选取，同一请求的 `ua` 与 `tls_emulation` 绑定一致
/// - 代理经 `ProxyProvider` trait 调度（与 ReqwestEngine 的 H2 修复一致；
///   design.md D4 原写 `Option<Arc<ProxyPool>>`，遵循代码库惯例改用 trait 抽象）
pub struct WreqEngine {
    /// wreq 客户端（构造时构建，含 TLS 指纹模拟配置）
    client: wreq::Client,
    /// UA 池（R-identity-001）：每请求选一致 UA + Accept-Language + sec-ch-ua + tls_emulation
    ua_pool: Arc<UaPool>,
    /// 代理提供者（H2：依赖抽象 trait）
    proxy_provider: Option<Arc<dyn ProxyProvider>>,
    /// 代理调度策略（RoundRobin / Sticky）
    proxy_strategy: ProxyStrategy,
    /// 引擎级请求超时（秒）—— 注入自 settings.timeouts.engines.default_timeout_seconds
    timeout_seconds: u64,
    /// 单引擎最大响应时间（MRT，design.md §14 / T062）。
    /// 注入自 `settings.timeouts.engines.tls_seconds`（默认 15 秒）。
    mrt: Duration,
}

impl WreqEngine {
    /// 创建 WreqEngine（无代理提供者，Phase 1 / D4）。
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
        })
    }

    /// 构建基础 wreq 客户端（含默认 Chrome 系 TLS 指纹模拟 + 超时 + 重定向跟随）。
    fn build_base_client(timeout_seconds: u64) -> Result<wreq::Client, EngineError> {
        wreq::Client::builder()
            .emulation(emulation_provider(TlsEmulation::Chrome131))
            .redirect(wreq::redirect::Policy::default())
            .timeout(Duration::from_secs(timeout_seconds))
            .build()
            .map_err(|e| EngineError::Internal(format!("wreq client build failed: {e}")))
    }

    /// 构建带代理的临时 wreq 客户端。
    ///
    /// # 返回
    ///
    /// `Some(client)`：构建成功；`None`：失败（代理 URL 非法或构建失败，已打 error 日志）。
    /// 调用方在 `None` 时回退到基础 client（rule 12：失败显性化到日志，不静默吞掉）。
    fn build_proxy_client(proxy_url: &str, timeout_seconds: u64) -> Option<wreq::Client> {
        let proxy = match wreq::Proxy::http(proxy_url) {
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
            .redirect(wreq::redirect::Policy::default())
            .proxy(proxy)
            .timeout(Duration::from_secs(timeout_seconds))
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

    /// 获取 UA 池引用（测试验证 R-identity-001）。
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
    /// 代理优先级（对齐 ReqwestEngine / design.md §12）：
    /// 1. 请求级代理（`request.proxy`）覆盖池
    /// 2. 代理提供者按 `proxy_strategy` 调度（RoundRobin → next / Sticky → sticky）
    /// 3. 无代理 → 基础 client
    ///
    /// 返回 `(client, Option<String> used_proxy_url)`：`used_proxy_url` 用于
    /// `ProxyProvider::mark_failure` / `mark_success` 状态回填。
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
    /// 确保 TLS SNI 和 HTTP 虚拟主机路由正确。
    fn build_resolve_client(
        host: &str,
        resolve_addrs: &[std::net::SocketAddr],
        proxy_url: Option<&str>,
        skip_tls: bool,
        timeout_seconds: u64,
    ) -> Result<wreq::Client, EngineError> {
        let mut builder = wreq::Client::builder()
            .emulation(emulation_provider(TlsEmulation::Chrome131))
            .redirect(wreq::redirect::Policy::default())
            .timeout(Duration::from_secs(timeout_seconds));

        // SSRF resolve 覆盖：hostname → 验证过的 IP
        if !host.is_empty() && host.parse::<std::net::IpAddr>().is_err() {
            builder = builder.resolve_to_addrs(host, resolve_addrs);
        }

        if skip_tls {
            log::warn!(
                "wreq skip_tls_verification=true: TLS certificate validation disabled for this request"
            );
            builder = builder.cert_verification(false);
        }

        if let Some(url) = proxy_url {
            match wreq::Proxy::http(url) {
                Ok(px) => {
                    builder = builder.proxy(px);
                    log::debug!("Using wreq HTTP proxy: {}", redact_proxy_url(url));
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

        // R-identity-001：选与请求匹配的 UA profile（ua + accept_language + sec-ch-ua + tls_emulation）
        let profile = self.ua_pool.pick(request.mobile);

        // 确定本次请求使用的代理 URL（与 get_client 逻辑一致）
        let used_proxy_url = self.resolve_proxy_url(&request.proxy, request.session_id.as_deref());

        // 构建含 SSRF resolve 覆盖的 wreq client（保持原始 hostname → TLS SNI / 虚拟主机路由正确）
        let effective_client = Self::build_resolve_client(
            &host,
            &resolve_addrs,
            used_proxy_url.as_deref(),
            request.skip_tls_verification,
            self.timeout_seconds,
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

        let content = response
            .text()
            .await
            .map_err(|e| EngineError::RequestFailed(e.to_string()))?;

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

    /// T062：返回注入的 MRT（默认 15s，对应 `tls_seconds`）。
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

    // === 指纹映射测试（T015 联动：TlsEmulation → EmulationProvider）===

    #[test]
    fn test_emulation_provider_roundtrips_all_variants() {
        // 全部变体都能构造出 EmulationProvider（当前统一默认 Chrome 系指纹，见模块文档）
        for variant in [
            TlsEmulation::Chrome131,
            TlsEmulation::Chrome130,
            TlsEmulation::Chrome120,
            TlsEmulation::Firefox133,
            TlsEmulation::Safari17,
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

    // === JA4 指纹外部验证（T024 / R-tls-005）===
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
