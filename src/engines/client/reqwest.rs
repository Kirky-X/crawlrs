// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::config::settings::ProxyStrategy;
use crate::engines::client::handle::ClientHandle;
use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use crate::engines::provider::{ProxyCategory, ProxyProvider};
use crate::engines::validators;
use crate::utils::proxy::redact_proxy_url;
use crate::utils::ua_pool::UaPool;
use async_trait::async_trait;
use log::error;
use lru::LruCache;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 默认超时时间
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

/// 默认代理策略（无 settings 注入时使用）
const DEFAULT_PROXY_STRATEGY: ProxyStrategy = ProxyStrategy::RoundRobin;

/// ReqwestEngine 默认 MRT（5 秒，对应 `EngineTimeoutSettings::fetch_seconds`）。
///
/// design.md §14 / T060：HTTP fetch 引擎比浏览器引擎快，5 秒已足够覆盖正常请求；
/// 超时即切下一引擎（瀑布式）。生产环境应通过 [`ReqwestEngine::new_with_mrt`] 等
/// 构造函数从 `Settings.timeouts.engines.fetch_seconds` 注入，避免硬编码。
const DEFAULT_REQWEST_MRT_SECONDS: u64 = 5;

/// 抓取引擎
///
/// 基于reqwest实现的基本HTTP抓取引擎
///
/// 代理来源（T056/R-identity-003 + H1/H2/H3 修复）：
/// - **请求级代理**（`request.proxy`）：覆盖池，调用方指定特定代理
/// - **代理提供者**（`proxy_provider`）：按 `ProxyStrategy` 决定调度策略
///   - `RoundRobin`：`ProxyProvider::next(ProxyCategory::Html)`
///   - `Sticky`：`ProxyProvider::sticky(session_id)`（`request.session_id` 必填，否则 fallback 到 next）
/// - 两者皆无 → 直接使用注入的 `http_client`（无代理）
pub struct ReqwestEngine {
    /// HTTP 客户端（通过依赖注入，支持连接复用）
    http_client: Arc<reqwest::Client>,
    /// 代理提供者（H2 修复：依赖抽象 `ProxyProvider` trait，而非具体 `ProxyPool`）
    ///
    /// `None` 时不使用代理；`Some` 时按 `proxy_strategy` 调度策略取代理。
    proxy_provider: Option<Arc<dyn ProxyProvider>>,
    /// 代理调度策略（H1 修复：用于在 `next` / `sticky` 之间路由）
    proxy_strategy: ProxyStrategy,
    /// 引擎级请求超时（秒），用于 build_custom_client 构造临时 client（proxy/skip_tls 路径）
    /// 注入自 Settings.timeouts.engines.default_timeout_seconds（架构 MEDIUM：避免硬编码 30 秒）
    timeout_seconds: u64,
    /// 单引擎最大响应时间（MRT，design.md §14 / T060）。
    ///
    /// router 顺序 fallback 路径用 `min(remaining, mrt)` 包裹单引擎调用，
    /// 超 MRT 即切下一引擎。注入自 `Settings.timeouts.engines.fetch_seconds`（默认 5 秒）。
    mrt: Duration,
    /// UA 池（R-identity-001）：每次请求从池中选取一致的 UA + Accept-Language + sec-ch-ua
    ua_pool: UaPool,
    /// 代理 client 缓存（性能审查 HIGH-1 修复）
    ///
    /// 按 `proxy_url` 缓存已构建的 `reqwest::Client`，避免每次请求都重建 client
    /// 导致连接池丢失。`reqwest::Client::clone()` 内部 Arc 共享，克隆开销极低。
    ///
    /// 仅缓存 proxy_provider 路径下成功构建的 client（`is_fallback=false`）：
    /// - skip_tls 路径不缓存（请求级别配置，每次可能不同）
    /// - 请求级 proxy 不缓存（按需用，文档说"很少用"，且 URL 范围不可控）
    /// - fallback client 不缓存（避免缓存注入的 http_client 反复 clone）
    ///
    /// 缓存键为代理 URL（池数量有限，缓存大小有界）。
    /// PERF-007: LRU 缓存替代 DashMap，容量 64，parking_lot Mutex 短临界区保护。
    /// 代理池数量有限，LRU 淘汰确保缓存有界且热代理常驻。
    proxy_client_cache: parking_lot::Mutex<LruCache<String, reqwest::Client>>,
}

impl ReqwestEngine {
    /// 创建新的 ReqwestEngine 实例（无代理提供者）
    ///
    /// 使用 DEFAULT_TIMEOUT_SECONDS（30 秒）作为引擎级超时。
    /// 生产环境应使用 [`ReqwestEngine::new_with_timeout`] 从 Settings 注入超时，
    /// 并使用 [`ReqwestEngine::with_provider`] 注入代理提供者。
    pub fn new(http_client: Arc<reqwest::Client>) -> Self {
        Self::new_with_timeout(http_client, DEFAULT_TIMEOUT_SECONDS)
    }

    /// 创建带超时配置的 ReqwestEngine 实例（无代理提供者）
    ///
    /// 生产环境调用点应从 `settings.timeouts.engines.default_timeout_seconds` 注入超时，
    /// 避免硬编码 30 秒（架构 MEDIUM 2）。
    pub fn new_with_timeout(http_client: Arc<reqwest::Client>, timeout_seconds: u64) -> Self {
        Self::new_with_timeout_and_mrt(
            http_client,
            timeout_seconds,
            Duration::from_secs(DEFAULT_REQWEST_MRT_SECONDS),
        )
    }

    /// 创建带超时 + MRT 配置的 ReqwestEngine 实例（无代理提供者，T060/T061）。
    ///
    /// 生产环境应从：
    /// - `settings.timeouts.engines.default_timeout_seconds` 注入 `timeout_seconds`
    /// - `settings.timeouts.engines.fetch_seconds` 注入 `mrt`
    #[must_use]
    pub fn new_with_timeout_and_mrt(
        http_client: Arc<reqwest::Client>,
        timeout_seconds: u64,
        mrt: Duration,
    ) -> Self {
        Self {
            http_client,
            proxy_provider: None,
            proxy_strategy: DEFAULT_PROXY_STRATEGY,
            timeout_seconds,
            mrt,
            ua_pool: UaPool::default(),
            proxy_client_cache: parking_lot::Mutex::new(LruCache::new(
                NonZeroUsize::new(64).unwrap(),
            )),
        }
    }

    /// 创建带代理提供者配置的 ReqwestEngine 实例（H2 修复：依赖 `ProxyProvider` trait）
    ///
    /// 默认使用 `ProxyStrategy::RoundRobin`。如需 sticky，请使用 [`Self::with_provider_and_strategy`]。
    /// 使用 DEFAULT_TIMEOUT_SECONDS（30 秒）作为引擎级超时。
    #[must_use]
    pub fn with_provider(
        http_client: Arc<reqwest::Client>,
        proxy_provider: Arc<dyn ProxyProvider>,
    ) -> Self {
        Self::with_provider_strategy_and_timeout(
            http_client,
            proxy_provider,
            DEFAULT_PROXY_STRATEGY,
            DEFAULT_TIMEOUT_SECONDS,
        )
    }

    /// 创建带代理提供者 + 策略 + 超时配置的 ReqwestEngine 实例（H1/H2 修复）
    ///
    /// 生产环境调用点应从：
    /// - `settings.proxy.strategy` 注入 `proxy_strategy`
    /// - `settings.timeouts.engines.default_timeout_seconds` 注入超时
    ///
    /// 避免硬编码（架构 MEDIUM 2）。
    #[must_use]
    pub fn with_provider_strategy_and_timeout(
        http_client: Arc<reqwest::Client>,
        proxy_provider: Arc<dyn ProxyProvider>,
        proxy_strategy: ProxyStrategy,
        timeout_seconds: u64,
    ) -> Self {
        Self::with_provider_strategy_timeout_and_mrt(
            http_client,
            proxy_provider,
            proxy_strategy,
            timeout_seconds,
            Duration::from_secs(DEFAULT_REQWEST_MRT_SECONDS),
        )
    }

    /// 创建带代理提供者 + 策略 + 超时 + MRT 配置的 ReqwestEngine 实例（T060/T061）。
    ///
    /// 生产环境调用点应从：
    /// - `settings.proxy.strategy` 注入 `proxy_strategy`
    /// - `settings.timeouts.engines.default_timeout_seconds` 注入 `timeout_seconds`
    /// - `settings.timeouts.engines.fetch_seconds` 注入 `mrt`
    #[must_use]
    pub fn with_provider_strategy_timeout_and_mrt(
        http_client: Arc<reqwest::Client>,
        proxy_provider: Arc<dyn ProxyProvider>,
        proxy_strategy: ProxyStrategy,
        timeout_seconds: u64,
        mrt: Duration,
    ) -> Self {
        Self {
            http_client,
            proxy_provider: Some(proxy_provider),
            proxy_strategy,
            timeout_seconds,
            mrt,
            ua_pool: UaPool::default(),
            proxy_client_cache: parking_lot::Mutex::new(LruCache::new(
                NonZeroUsize::new(64).unwrap(),
            )),
        }
    }

    /// 获取 UA 池引用（用于测试验证 R-identity-001）
    #[must_use]
    pub fn ua_pool(&self) -> &UaPool {
        &self.ua_pool
    }

    /// 引擎是否配置了代理提供者（用于测试验证 H2）
    #[must_use]
    pub fn has_proxy_provider(&self) -> bool {
        self.proxy_provider.is_some()
    }

    /// 获取代理调度策略（用于测试验证 H1）
    #[must_use]
    pub fn proxy_strategy(&self) -> ProxyStrategy {
        self.proxy_strategy
    }

    /// 获取引擎级 MRT（用于测试验证 T060）。
    ///
    /// 返回构造时注入的 `mrt`（默认 5 秒，对应 `fetch_seconds`）。
    #[must_use]
    pub fn mrt(&self) -> Duration {
        self.mrt
    }

    /// 构建自定义 reqwest::Client（统一处理 proxy + skip_tls）
    ///
    /// 与 init_http_client 保持一致：强制 IPv4 + dns_resolver（架构 HIGH：代理分支缺 dns_resolver）。
    /// - `proxy_url`: 可选代理 URL（None 或空字符串表示不使用代理）
    /// - `skip_tls`: true 时启用 `danger_accept_invalid_certs(true)`（仅开发环境，生产环境由
    ///   `ScrapeOptions::builder().skip_tls_verification(true)` 在 APP_ENVIRONMENT=production 时拒绝）
    /// - `timeout_seconds`: 请求超时（秒），从 Settings 注入避免硬编码
    /// - `fallback`: 构建失败时回退到的注入 http_client
    ///
    /// # 返回值
    ///
    /// `(reqwest::Client, bool)` —— 第二个 bool 是 `is_fallback`：
    /// - `false`：client 构建成功
    /// - `true`：构建失败，已回退到 `fallback`（M4 修复：失败显性化，规则12）
    ///
    /// M4 修复：失败时日志从 `warn` 升级为 `error`，并返回 `is_fallback=true` 标志
    /// 让调用方感知失败（不藏默认值背后）。
    fn build_custom_client(
        proxy_url: Option<&str>,
        skip_tls: bool,
        fallback: &Arc<reqwest::Client>,
        timeout_seconds: u64,
    ) -> (reqwest::Client, bool) {
        // 强制 IPv4 + dns_resolver：与 init_http_client 保持一致
        // 避免代理路径下 DNS 解析仍走系统默认 getaddrinfo 返回 IPv6
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .cookie_store(true)
            .local_address(Some(std::net::Ipv4Addr::UNSPECIFIED.into()))
            .dns_resolver(crate::infrastructure::dns::create_ipv4_only_resolver());

        if skip_tls {
            builder = builder.danger_accept_invalid_certs(true);
        }

        let effective_proxy = proxy_url.map(|s| s.trim()).filter(|s| !s.is_empty());

        match effective_proxy {
            Some(url) => {
                match reqwest::Proxy::http(url) {
                    Ok(proxy) => match builder.proxy(proxy).build() {
                        Ok(client) => {
                            // 安全：代理 URL 可能含 user:pass@host 凭据，日志输出必须脱敏
                            // （CWE-532 防护，T056 安全审查 CRITICAL-1 修复）
                            log::debug!(
                                "Using HTTP proxy: {} (skip_tls={}, timeout={}s)",
                                redact_proxy_url(url),
                                skip_tls,
                                timeout_seconds
                            );
                            (client, false)
                        }
                        Err(e) => {
                            // M4 修复：warn → error（规则12：失败必须显性化）
                            // 安全：url 脱敏后输出（CWE-532 防护）
                            error!(
                            "Failed to build proxy client (url={}, skip_tls={}, timeout={}s): {}, \
                             falling back to injected http_client",
                            redact_proxy_url(url), skip_tls, timeout_seconds, e
                        );
                            ((**fallback).clone(), true)
                        }
                    },
                    Err(e) => {
                        // M4 修复：warn → error
                        // 安全：url 脱敏后输出（CWE-532 防护）
                        error!(
                        "Failed to configure HTTP proxy (url={}): {}, falling back to injected http_client",
                        redact_proxy_url(url), e
                    );
                        ((**fallback).clone(), true)
                    }
                }
            }
            None => match builder.build() {
                Ok(client) => {
                    if skip_tls {
                        log::debug!(
                            "Using client with skip_tls=true (no proxy, timeout={}s)",
                            timeout_seconds
                        );
                    }
                    (client, false)
                }
                Err(e) => {
                    // M4 修复：warn → error
                    error!(
                        "Failed to build client (skip_tls={}, timeout={}s): {}, \
                         falling back to injected http_client",
                        skip_tls, timeout_seconds, e
                    );
                    ((**fallback).clone(), true)
                }
            },
        }
    }

    /// 获取 HTTP 客户端句柄（H3 修复：返回 `ClientHandle`，封装代理 URL 状态回填）
    ///
    /// 代理优先级（design.md §12，T056 + H1/H2/H3 修复）：
    /// 1. **请求级代理**（`request.proxy`）：覆盖提供者
    /// 2. **代理提供者**（`proxy_provider`）：按 `proxy_strategy` 调度
    ///    - `RoundRobin` → `ProxyProvider::next(ProxyCategory::Html)`
    ///    - `Sticky` + `session_id` 存在 → `ProxyProvider::sticky(session_id)`
    ///    - `Sticky` + `session_id` 为 None → fallback 到 `next`，并 warn 日志
    /// 3. **无代理**：直接使用注入的 `http_client`
    ///
    /// `skip_tls_verification=true` 时必须构建临时 client（无法覆盖已有 client 的 TLS 设置），
    /// 并输出 warn 日志（安全审计需要 — TLS 验证被显式跳过）。
    ///
    /// L2 修复：用 early return 拍平原 3 层嵌套（provider → match → if let）。
    /// M4 修复：`build_custom_client` 返回 `(client, is_fallback)` 元组，传递给 `ClientHandle::new`。
    fn get_client(
        &self,
        proxy: &Option<String>,
        skip_tls: bool,
        session_id: Option<&str>,
    ) -> ClientHandle {
        // skip_tls_verification=true：构建临时 client with danger_accept_invalid_certs
        // 生产环境 ScrapeOptions::builder() 已拒绝该选项，这里只处理开发环境
        if skip_tls {
            log::warn!(
                "skip_tls_verification=true: TLS certificate validation disabled for this request"
            );
            let proxy_url = proxy.as_ref().map(|s| s.as_str());
            let (client, is_fallback) =
                Self::build_custom_client(proxy_url, true, &self.http_client, self.timeout_seconds);
            let used = proxy.as_ref().and_then(|s| {
                let t = s.trim();
                if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                }
            });
            return ClientHandle::new(client, used, is_fallback);
        }

        // 请求级代理优先（覆盖提供者）
        let request_proxy = proxy.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty());

        if let Some(url) = request_proxy {
            // 请求级代理：每次构建（不缓存，请求级代理很少用）
            let (client, is_fallback) = Self::build_custom_client(
                Some(url),
                false,
                &self.http_client,
                self.timeout_seconds,
            );
            return ClientHandle::new(client, Some(url.to_string()), is_fallback);
        }

        // 从代理提供者按策略调度（H1：按 ProxyStrategy 路由 next / sticky）
        // L2 修复：用 early return 拍平嵌套，避免 provider → match → if let 三层
        let Some(provider) = &self.proxy_provider else {
            // 无代理提供者 → 直接使用注入的 http_client
            return ClientHandle::new(self.http_client.as_ref().clone(), None, false);
        };

        let picked = match self.proxy_strategy {
            ProxyStrategy::RoundRobin => provider.next(ProxyCategory::Html),
            ProxyStrategy::Sticky => match session_id {
                Some(sid) if !sid.is_empty() => provider.sticky(sid),
                // Sticky 策略但 session_id 缺失：fallback 到 next 并 warn
                _ => {
                    log::warn!(
                        "proxy_strategy=Sticky but session_id is missing/empty; \
                         falling back to RoundRobin (next)"
                    );
                    provider.next(ProxyCategory::Html)
                }
            },
        };

        let Some(url) = picked else {
            // 池为空或全冷却 → fallback 到 http_client
            log::debug!(
                "proxy_provider returned None (empty pool or all proxies in cooldown), \
                 using injected http_client"
            );
            return ClientHandle::new(self.http_client.as_ref().clone(), None, false);
        };

        // 性能审查 HIGH-1 修复：proxy_provider 路径缓存 reqwest::Client
        //
        // 原实现每次请求都 build_custom_client，导致：
        // - 每次请求都新建 reqwest::Client（含 cookie store + dns resolver + TLS context）
        // - 连接池无法复用，每次都重新 TCP/TLS 握手
        // - 在高并发爬取场景下严重影响吞吐量
        //
        // 修复后：按 proxy_url 缓存成功构建的 client，reqwest::Client::clone() 仅增加 Arc 引用计数
        // 缓存大小有界（池中代理 URL 数量有限），不会无限增长。
        //
        // 仅缓存 is_fallback=false 的 client；fallback 路径直接用注入的 http_client（避免缓存 fallback）
        let (client, is_fallback) = {
            let mut cache = self.proxy_client_cache.lock();
            if let Some(cached) = cache.get(&url) {
                // 缓存命中：clone 即可（reqwest::Client::clone 内部 Arc 共享）
                (cached.clone(), false)
            } else {
                drop(cache); // 释放锁后再构建 client
                             // 缓存未命中：构建新 client
                let (client, is_fallback) = Self::build_custom_client(
                    Some(&url),
                    false,
                    &self.http_client,
                    self.timeout_seconds,
                );
                // 仅缓存成功构建的 client（fallback 不缓存）
                if !is_fallback {
                    self.proxy_client_cache
                        .lock()
                        .put(url.clone(), client.clone());
                }
                (client, is_fallback)
            }
        };
        ClientHandle::new(client, Some(url), is_fallback)
    }
}

#[async_trait]
impl ScraperEngine for ReqwestEngine {
    /// 执行HTTP抓取
    ///
    /// # 参数
    ///
    /// * `request` - 抓取请求
    ///
    /// # 返回值
    ///
    /// * `Ok(InternalScrapeResponse)` - 抓取响应
    /// * `Err(EngineError)` - 抓取过程中出现的错误
    async fn scrape(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
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
        // SSRF: pin hostname → validated IPs via reqwest resolve override.
        // Keep the original URL intact so TLS SNI and virtual host routing work correctly.
        // SSRF 关闭（开发/测试开关）时 resolved_ips 为空 → 跳过 pin，正常 DNS 解析。
        let resolve_addrs: Vec<std::net::SocketAddr> = validated_url
            .resolved_ips
            .iter()
            .map(|ip| std::net::SocketAddr::new(*ip, port))
            .collect();
        if resolve_addrs.is_empty()
            && !crate::infrastructure::security::ssrf::ssrf_protection_disabled()
        {
            return Err(EngineError::Other("SSRF: no resolved IPs".to_string()));
        }
        let need_tls_bypass = request.skip_tls_verification;
        let handle = self.get_client(
            &request.proxy,
            request.skip_tls_verification,
            request.session_id.as_deref(),
        );

        // SSRF DNS pin: build client with resolve override from scratch.
        // Keeps the original hostname for TLS SNI and HTTP virtual host routing,
        // while connecting to SSRF-validated IPs.
        let ssrf_client = if !host.is_empty() && !host.parse::<std::net::IpAddr>().is_ok() {
            let mut b = reqwest::Client::builder()
                .timeout(Duration::from_secs(self.timeout_seconds))
                .cookie_store(true)
                .local_address(Some(std::net::Ipv4Addr::UNSPECIFIED.into()))
                .dns_resolver(crate::infrastructure::dns::create_ipv4_only_resolver())
                .resolve_to_addrs(&host, &resolve_addrs);
            if need_tls_bypass {
                b = b.danger_accept_invalid_certs(true);
            }
            if let Some(p) = &handle.used_proxy_url {
                if let Ok(px) = reqwest::Proxy::http(p) {
                    b = b.proxy(px);
                }
            }
            b.build().map_err(|e| {
                EngineError::Other(format!("Failed to build SSRF resolve client: {}", e))
            })?
        } else {
            // Host is an IP literal (no DNS to override) or empty — use base client
            let temp_client: Option<reqwest::Client> =
                if need_tls_bypass {
                    let mut b = reqwest::Client::builder()
                        .timeout(Duration::from_secs(self.timeout_seconds))
                        .cookie_store(true)
                        .local_address(Some(std::net::Ipv4Addr::UNSPECIFIED.into()))
                        .danger_accept_invalid_certs(true);
                    if let Some(p) = &handle.used_proxy_url {
                        if let Ok(px) = reqwest::Proxy::http(p) {
                            b = b.proxy(px);
                        }
                    }
                    Some(b.build().map_err(|e| {
                        EngineError::Other(format!("Failed build temp client: {}", e))
                    })?)
                } else {
                    None
                };
            temp_client.unwrap_or_else(|| handle.client.clone())
        };
        let effective_client = &ssrf_client;

        // Build headers
        let mut headers = HeaderMap::new();
        for (k, v) in &request.headers {
            if let (Ok(header_name), Ok(header_value)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                headers.insert(header_name, header_value);
            }
        }

        // R-identity-001: 从 UaPool 取一致的 UA + Accept-Language + sec-ch-ua profile
        // 替换原固定 DEFAULT_USER_AGENT / 固定移动 UA 分支；
        // DEFAULT_USER_AGENT 仍保留在 http_client.rs 作为 client 级默认（UaPool 不可用时的回退）
        let profile = self.ua_pool.pick(request.mobile);

        // Create request builder.
        //
        // Use a real browser User-Agent from UaPool instead of a self-identifying
        // `crawlrs/*` UA: major search engines (Baidu, Sogou, Bing) reject
        // bot-identified requests with a 227-byte JS-redirect error page
        // instead of returning actual search results.
        let request_url = validated_url.parsed_url.as_str();
        let mut request_builder = match request.method {
            crate::engines::engine_client::HttpMethod::Get => effective_client.get(request_url),
            crate::engines::engine_client::HttpMethod::Post => effective_client.post(request_url),
        };

        // 应用 UA 绑定 headers（User-Agent + Accept-Language + sec-ch-ua）
        // - User-Agent: 所有 profile 必设
        // - Accept-Language: 所有 profile 必设
        // - sec-ch-ua: 仅 Chromium-based 浏览器（Firefox/Safari 为空串，跳过）
        // 用户自定义 headers（request.headers）在后文 insert，可覆盖这些默认值
        request_builder = request_builder
            .header("User-Agent", profile.ua)
            .header("Accept-Language", profile.accept_language);
        if !profile.sec_ch_ua.is_empty() {
            request_builder = request_builder.header("sec-ch-ua", profile.sec_ch_ua);
        }

        // Add custom headers
        request_builder = request_builder.headers(headers);

        if let Some(body) = &request.body {
            request_builder = request_builder.body(body.clone());
        }

        // Set timeout
        request_builder = request_builder.timeout(request.timeout);

        let start = Instant::now();
        let response_result = request_builder.send().await;

        let response = match response_result {
            Ok(resp) => resp,
            Err(e) if e.is_timeout() => {
                // H3: 通过 ClientHandle.report_failure 回填 ProxyProvider（无需感知 used_proxy_url）
                if let Some(provider) = &self.proxy_provider {
                    handle.report_failure(provider.as_ref());
                }
                return Err(EngineError::Timeout(request.timeout));
            }
            Err(e) => {
                // H3: 通过 ClientHandle.report_failure 回填 ProxyProvider（无需感知 used_proxy_url）
                if let Some(provider) = &self.proxy_provider {
                    handle.report_failure(provider.as_ref());
                }
                // 打印完整错误链（含 source）以便诊断根因
                let mut chain = Vec::new();
                let mut current: Option<&dyn std::error::Error> = Some(&e);
                while let Some(err) = current {
                    chain.push(err.to_string());
                    current = err.source();
                }
                error!(
                    "reqwest send failed for {}: error chain: {}",
                    request.url,
                    chain.join(" -> ")
                );
                return Err(EngineError::RequestFailed(e.to_string()));
            }
        };

        let status_code = response.status().as_u16();

        // H3: 2xx 成功 → 通过 ClientHandle.report_success 回填 ProxyProvider 恢复代理健康
        // 4xx/5xx 视为业务层失败，但代理本身可用，不 report_failure（避免误伤好代理）
        if (200..300).contains(&status_code) {
            if let Some(provider) = &self.proxy_provider {
                handle.report_success(provider.as_ref());
            }
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html")
            .to_string();

        // Ensure content_type is not empty
        let content_type = if content_type.trim().is_empty() {
            "text/html".to_string()
        } else {
            content_type
        };

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

        // 同步等待
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

    /// 计算对请求的支持分数
    ///
    /// # 参数
    ///
    /// * `request` - 抓取请求
    ///
    /// # 返回值
    ///
    /// 支持分数（0-100），不支持JS和截图的请求返回100分
    fn support_score(&self, request: &InternalScrapeRequest) -> u8 {
        if request.needs_js || request.needs_screenshot {
            return 10; // Low priority for unsupported features
        }
        // Phase 1 / D4：needs_tls_fingerprint 请求应由专用 TLS 指纹引擎（WreqEngine）处理，
        // reqwest（rustls 后端）无法伪装 JA4 指纹，返回 10（低分），让 router 优先选 WreqEngine。
        if request.needs_tls_fingerprint {
            return 10;
        }
        100 // Highest priority (fastest)
    }

    /// 获取引擎名称
    ///
    /// # 返回值
    ///
    /// 引擎名称
    fn name(&self) -> &'static str {
        "reqwest"
    }

    /// T060：覆写 MRT，返回构造时注入的 `mrt`（默认 5 秒）。
    ///
    /// router 顺序 fallback 路径用 `min(remaining, self.mrt)` 包裹单引擎调用，
    /// 超 MRT 即切下一引擎（瀑布式）。
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

    fn create_test_client() -> Arc<reqwest::Client> {
        Arc::new(
            reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
        )
    }

    fn create_basic_request(url: &str) -> InternalScrapeRequest {
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

    fn create_request_with_js(url: &str) -> InternalScrapeRequest {
        InternalScrapeRequest {
            url: url.to_string(),
            method: HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: true,
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

    fn create_request_with_screenshot(url: &str) -> InternalScrapeRequest {
        InternalScrapeRequest {
            url: url.to_string(),
            method: HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: true,
            screenshot_config: Some(InternalScreenshotConfig {
                full_page: true,
                selector: None,
                quality: Some(80),
                format: Some("jpeg".to_string()),
            }),
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

    // === ReqwestEngine creation tests ===

    #[test]
    fn test_reqwest_engine_new() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        assert_eq!(engine.name(), "reqwest");
        assert!(!engine.has_proxy_provider());
    }

    #[test]
    fn test_reqwest_engine_with_provider() {
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://a:8080".to_string(), "http://b:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider(client, pool);
        assert_eq!(engine.name(), "reqwest");
        assert!(engine.has_proxy_provider());
    }

    #[test]
    fn test_reqwest_engine_with_provider_strategy_and_timeout() {
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://a:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider_strategy_and_timeout(
            client,
            pool,
            ProxyStrategy::RoundRobin,
            45,
        );
        assert_eq!(engine.name(), "reqwest");
        assert!(engine.has_proxy_provider());
        assert_eq!(engine.proxy_strategy(), ProxyStrategy::RoundRobin);
        assert_eq!(engine.timeout_seconds, 45);
    }

    // === name() tests ===

    #[test]
    fn test_name_returns_reqwest() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        assert_eq!(engine.name(), "reqwest");
    }

    #[test]
    fn test_name_returns_reqwest_with_provider() {
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://proxy:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider(client, pool);
        assert_eq!(engine.name(), "reqwest");
    }

    // === support_score tests ===

    #[test]
    fn test_support_score_basic_get_request() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_basic_request("https://example.com");
        // Basic GET without JS/screenshot should get 100
        assert_eq!(engine.support_score(&request), 100);
    }

    #[test]
    fn test_support_score_basic_post_request() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = InternalScrapeRequest {
            url: "https://example.com".to_string(),
            method: HttpMethod::Post,
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
            body: Some("data".to_string()),
            sync_wait_ms: 0,
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
            needs_mllm: false,
        };
        assert_eq!(engine.support_score(&request), 100);
    }

    #[test]
    fn test_support_score_needs_js_returns_low() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_request_with_js("https://example.com");
        // JS requests should get low score (10) since reqwest can't render JS
        assert_eq!(engine.support_score(&request), 10);
    }

    #[test]
    fn test_support_score_needs_screenshot_returns_low() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_request_with_screenshot("https://example.com");
        // Screenshot requests should get low score (10)
        assert_eq!(engine.support_score(&request), 10);
    }

    #[test]
    fn test_support_score_needs_js_and_screenshot_returns_low() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = InternalScrapeRequest {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: true,
            needs_screenshot: true,
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
        };
        assert_eq!(engine.support_score(&request), 10);
    }

    #[test]
    fn test_support_score_mobile_without_js() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = InternalScrapeRequest {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: true,
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
        };
        // Mobile without JS should still get 100
        assert_eq!(engine.support_score(&request), 100);
    }

    #[test]
    fn test_support_score_with_provider() {
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://proxy:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider(client, pool);
        let request = create_basic_request("https://example.com");
        // ProxyProvider shouldn't affect support score
        assert_eq!(engine.support_score(&request), 100);
    }

    // === support_score: needs_tls_fingerprint (Phase 1 / D4, T013-T014) ===

    #[test]
    fn test_support_score_needs_tls_fingerprint_returns_low() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // needs_tls_fingerprint=true → reqwest 无法伪装 JA4 指纹 → 10（低分，让位 WreqEngine）
        let request = InternalScrapeRequest {
            needs_tls_fingerprint: true,
            ..create_basic_request("https://example.com")
        };
        assert_eq!(engine.support_score(&request), 10);
    }

    #[test]
    fn test_support_score_no_tls_fingerprint_returns_full() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // needs_tls_fingerprint=false（默认）→ 100（最高优先）
        let request = create_basic_request("https://example.com");
        assert_eq!(engine.support_score(&request), 100);
    }

    // === scrape SSRF rejection tests ===

    #[tokio::test]
    async fn test_scrape_rejects_localhost() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_basic_request("http://localhost");
        let result = engine.scrape(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::Other(msg) => {
                assert!(msg.contains("SSRF protection"));
            }
            other => panic!("Expected Other with SSRF, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_scrape_rejects_127_0_0_1() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_basic_request("http://127.0.0.1");
        let result = engine.scrape(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_rejects_private_ip_192_168() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_basic_request("http://192.168.1.1");
        let result = engine.scrape(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_rejects_private_ip_10_0() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_basic_request("http://10.0.0.1");
        let result = engine.scrape(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_rejects_private_ip_172_16() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_basic_request("http://172.16.0.1");
        let result = engine.scrape(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_rejects_file_scheme() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_basic_request("file:///etc/passwd");
        let result = engine.scrape(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_rejects_ftp_scheme() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_basic_request("ftp://example.com");
        let result = engine.scrape(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_rejects_metadata_endpoint() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_basic_request("http://169.254.169.254");
        let result = engine.scrape(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_rejects_0000() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_basic_request("http://0.0.0.0");
        let result = engine.scrape(&request).await;
        assert!(result.is_err());
    }

    // === scrape with various request configurations (SSRF rejection) ===

    #[tokio::test]
    async fn test_scrape_post_request_rejects_ssrf() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = InternalScrapeRequest {
            url: "http://localhost".to_string(),
            method: HttpMethod::Post,
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
            body: Some("data".to_string()),
            sync_wait_ms: 0,
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
            needs_mllm: false,
        };
        let result = engine.scrape(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_mobile_request_rejects_ssrf() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = InternalScrapeRequest {
            url: "http://localhost".to_string(),
            method: HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: true,
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
        };
        let result = engine.scrape(&request).await;
        assert!(result.is_err());
    }

    // === scrape with invalid URL format ===

    #[tokio::test]
    async fn test_scrape_invalid_url_format() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_basic_request("not-a-valid-url");
        let result = engine.scrape(&request).await;
        // Should fail (either SSRF or request error)
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_empty_url() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let request = create_basic_request("");
        let result = engine.scrape(&request).await;
        assert!(result.is_err());
    }

    // === Default trait test for HttpMethod ===

    #[test]
    fn test_http_method_default_is_get() {
        assert_eq!(HttpMethod::default(), HttpMethod::Get);
    }

    // === Test logger for covering log::debug!/log::warn! in get_client ===

    use log::{LevelFilter, Log, Metadata, Record};
    use std::sync::Once;

    static LOGGER_INIT: Once = Once::new();

    struct CapturingLogger;

    impl Log for CapturingLogger {
        fn enabled(&self, metadata: &Metadata) -> bool {
            metadata.level() <= log::Level::Debug
        }
        fn log(&self, _record: &Record) {}
        fn flush(&self) {}
    }

    fn ensure_debug_logger() {
        LOGGER_INIT.call_once(|| {
            static CAPTURING_LOGGER: CapturingLogger = CapturingLogger;
            let _ = log::set_logger(&CAPTURING_LOGGER);
            log::set_max_level(LevelFilter::Debug);
        });
    }

    // === get_client private method tests (H3: 返回 ClientHandle) ===
    // get_client is a private method, but accessible via `use super::*` in tests.
    // These tests only build clients without sending HTTP requests.

    #[test]
    fn test_get_client_with_no_proxy_returns_injected_client_and_no_url() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // No proxy + no provider → ClientHandle.used_proxy_url = None
        let handle = engine.get_client(&None, false, None);
        assert!(
            handle.used_proxy_url().is_none(),
            "no proxy + no provider → used_proxy_url must be None"
        );
        assert!(!handle.has_proxy());
        // M4 修复：无代理路径非 fallback
        assert!(!handle.is_fallback(), "no proxy path must not be fallback");
    }

    #[test]
    fn test_get_client_with_request_level_proxy_returns_proxy_url() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // Request-level proxy (valid HTTP) → ClientHandle.used_proxy_url = Some(url)
        let handle = engine.get_client(
            &Some("http://proxy.example.com:8080".to_string()),
            false,
            None,
        );
        assert_eq!(
            handle.used_proxy_url(),
            Some("http://proxy.example.com:8080"),
            "request-level proxy must be returned as used_proxy_url"
        );
        assert!(handle.has_proxy());
        // M4 修复：有效代理路径非 fallback
        assert!(
            !handle.is_fallback(),
            "valid proxy path must not be fallback"
        );
    }

    #[test]
    fn test_get_client_with_invalid_request_proxy_still_returns_url() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // 即使 reqwest::Proxy::http 失败 fallback 到 injected client，used_proxy_url 仍是请求级代理
        // 因为调用方语义上"尝试使用了这个代理"。report_failure 仍会调用（provider 为 None 时跳过）
        let handle = engine.get_client(&Some("://invalid".to_string()), false, None);
        // "://invalid" trim 后非空 → 视为请求级代理，used_proxy_url = Some("://invalid")
        assert_eq!(handle.used_proxy_url(), Some("://invalid"));
        // M4 修复：无效代理 → build_custom_client 失败 → is_fallback=true
        assert!(
            handle.is_fallback(),
            "invalid proxy must set is_fallback=true (M4: 失败显性化)"
        );
    }

    #[test]
    fn test_get_client_with_empty_request_proxy_returns_no_url() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // 空字符串 → 视为无请求级代理
        let handle = engine.get_client(&Some("".to_string()), false, None);
        assert!(
            handle.used_proxy_url().is_none(),
            "empty proxy string → used_proxy_url must be None"
        );
        assert!(
            !handle.is_fallback(),
            "empty proxy → no proxy path, not fallback"
        );
    }

    #[test]
    fn test_get_client_with_provider_returns_pool_url() {
        ensure_debug_logger();
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://pool-proxy:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider(client, pool);
        // 无请求级代理 → 从 provider 取 → ClientHandle.used_proxy_url = Some(pool_url)
        let handle = engine.get_client(&None, false, None);
        assert_eq!(
            handle.used_proxy_url(),
            Some("http://pool-proxy:8080"),
            "provider must provide used_proxy_url for report_failure / report_success"
        );
        assert!(
            !handle.is_fallback(),
            "valid pool proxy must not be fallback"
        );
    }

    #[test]
    fn test_get_client_request_proxy_overrides_provider() {
        ensure_debug_logger();
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://pool-proxy:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider(client, pool);
        // 请求级代理 > 池代理
        let handle = engine.get_client(&Some("http://request-proxy:9090".to_string()), false, None);
        assert_eq!(
            handle.used_proxy_url(),
            Some("http://request-proxy:9090"),
            "request-level proxy must override provider"
        );
        assert!(
            !handle.is_fallback(),
            "valid request proxy must not be fallback"
        );
    }

    #[test]
    fn test_get_client_empty_pool_returns_injected_client() {
        ensure_debug_logger();
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            Vec::<String>::new(),
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider(client, pool);
        // 空池 → fallback 到注入 client，used_proxy_url = None
        let handle = engine.get_client(&None, false, None);
        assert!(
            handle.used_proxy_url().is_none(),
            "empty pool → used_proxy_url must be None"
        );
        // 空池不是 build_custom_client 失败，是 provider 返回 None 的正常路径
        assert!(
            !handle.is_fallback(),
            "empty pool is normal path, not build failure"
        );
    }

    #[test]
    fn test_get_client_pool_all_cooldown_returns_injected_client() {
        ensure_debug_logger();
        let client = create_test_client();
        let pool = Arc::new(ProxyPool::from_urls(
            vec!["http://pool-proxy:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        pool.mark_failure("http://pool-proxy:8080");
        let pool_dyn: Arc<dyn ProxyProvider> = pool;
        let engine = ReqwestEngine::with_provider(client, pool_dyn);
        // 全冷却 → fallback 到注入 client，used_proxy_url = None（不 report_failure）
        let handle = engine.get_client(&None, false, None);
        assert!(
            handle.used_proxy_url().is_none(),
            "all cooldown → fallback to injected client, used_proxy_url must be None"
        );
        assert!(
            !handle.is_fallback(),
            "all cooldown is normal path, not build failure"
        );
    }

    #[test]
    fn test_get_client_rr_cycles_through_pool() {
        ensure_debug_logger();
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec![
                "http://a:8080".to_string(),
                "http://b:8080".to_string(),
                "http://c:8080".to_string(),
            ],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider(client, pool);
        // RR 应轮询所有代理：3 次调用应覆盖 a/b/c（顺序由 rr 计数器决定）
        let urls: Vec<String> = (0..3)
            .map(|_| {
                engine
                    .get_client(&None, false, None)
                    .used_proxy_url()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert!(urls.contains(&"http://a:8080".to_string()));
        assert!(urls.contains(&"http://b:8080".to_string()));
        assert!(urls.contains(&"http://c:8080".to_string()));
    }

    #[test]
    fn test_get_client_sticky_strategy_uses_sticky_when_session_id_present() {
        ensure_debug_logger();
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://a:8080".to_string(), "http://b:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider_strategy_and_timeout(
            client,
            pool,
            ProxyStrategy::Sticky,
            30,
        );
        // Sticky + session_id="sess-1" → 两次调用应返回同一代理（粘性）
        let url1 = engine
            .get_client(&None, false, Some("sess-1"))
            .used_proxy_url()
            .unwrap()
            .to_string();
        let url2 = engine
            .get_client(&None, false, Some("sess-1"))
            .used_proxy_url()
            .unwrap()
            .to_string();
        assert_eq!(
            url1, url2,
            "sticky strategy must return same url within session"
        );
    }

    #[test]
    fn test_get_client_sticky_strategy_falls_back_to_rr_when_session_id_missing() {
        ensure_debug_logger();
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://a:8080".to_string(), "http://b:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider_strategy_and_timeout(
            client,
            pool,
            ProxyStrategy::Sticky,
            30,
        );
        // Sticky + session_id=None → fallback 到 next（RR）
        let handle = engine.get_client(&None, false, None);
        assert!(
            handle.used_proxy_url().is_some(),
            "sticky without session_id should fall back to next and return a proxy url"
        );
    }

    #[test]
    fn test_get_client_sticky_strategy_falls_back_to_rr_when_session_id_empty() {
        ensure_debug_logger();
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://a:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider_strategy_and_timeout(
            client,
            pool,
            ProxyStrategy::Sticky,
            30,
        );
        // Sticky + session_id="" → fallback 到 next（RR）
        let handle = engine.get_client(&None, false, Some(""));
        assert!(
            handle.used_proxy_url().is_some(),
            "sticky with empty session_id should fall back to next"
        );
    }

    #[test]
    fn test_get_client_with_valid_https_proxy() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // Valid HTTPS proxy URL
        let handle = engine.get_client(
            &Some("https://proxy.example.com:8443".to_string()),
            false,
            None,
        );
        assert_eq!(
            handle.used_proxy_url(),
            Some("https://proxy.example.com:8443")
        );
    }

    #[test]
    fn test_get_client_with_skip_tls_verification_builds_custom_client() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // skip_tls_verification=true → 构建临时 client with danger_accept_invalid_certs
        // 验证不 panic + 输出 warn 日志
        let handle = engine.get_client(&None, true, None);
        assert!(
            handle.used_proxy_url().is_none(),
            "skip_tls + no proxy → used_proxy_url None"
        );
    }

    #[test]
    fn test_get_client_with_skip_tls_and_proxy_builds_custom_client() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // skip_tls_verification=true + proxy → 构建临时 client with both options
        let handle = engine.get_client(&Some("http://proxy:8080".to_string()), true, None);
        assert_eq!(handle.used_proxy_url(), Some("http://proxy:8080"));
    }

    // === timeout 注入测试（架构 MEDIUM 2） ===

    #[test]
    fn test_new_with_timeout_sets_timeout_seconds() {
        let client = create_test_client();
        let engine = ReqwestEngine::new_with_timeout(client, 60);
        // 验证 timeout_seconds 字段被正确注入（通过 build_custom_client 路径间接验证）
        // get_client 不 panic 即说明 timeout_seconds 已正确传递
        let _ = engine.get_client(&None, false, None);
        assert_eq!(engine.timeout_seconds, 60);
    }

    #[test]
    fn test_with_provider_strategy_and_timeout_sets_timeout_seconds() {
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://proxy.example.com:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider_strategy_and_timeout(
            client,
            pool,
            ProxyStrategy::RoundRobin,
            45,
        );
        // provider 路径下，timeout_seconds 应为注入值 45
        assert_eq!(engine.timeout_seconds, 45);
        // 验证 proxy client 也使用注入的 timeout 构建（不 panic）
        let _ = engine.get_client(&None, false, None);
    }

    #[test]
    fn test_new_defaults_to_30_seconds() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // new() 默认使用 DEFAULT_TIMEOUT_SECONDS（30 秒）
        assert_eq!(engine.timeout_seconds, 30);
    }

    #[test]
    fn test_with_provider_defaults_to_30_seconds() {
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://proxy:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider(client, pool);
        // with_provider() 默认使用 DEFAULT_TIMEOUT_SECONDS（30 秒）
        assert_eq!(engine.timeout_seconds, 30);
    }

    #[test]
    fn test_get_client_with_injected_timeout_and_skip_tls_no_panic() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::new_with_timeout(client, 120);
        // skip_tls + 注入 timeout=120 → build_custom_client 用 120 秒构建临时 client
        // 验证不 panic + warn 日志输出
        let _ = engine.get_client(&Some("http://proxy:8080".to_string()), true, None);
    }

    // === T021 / R-identity-001: UaPool 集成测试 ===

    #[test]
    fn test_reqwest_engine_has_non_empty_ua_pool() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let pool = engine.ua_pool();
        assert!(
            pool.count(false) >= 20,
            "desktop pool must have >= 20 profiles"
        );
        assert!(
            pool.count(true) >= 20,
            "mobile pool must have >= 20 profiles"
        );
    }

    #[test]
    fn test_reqwest_engine_pick_ua_returns_varied_profiles() {
        // R-identity-001: 多次选取应返回不同 UA（随机性，避免固定 UA 被反爬识别）
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let pool = engine.ua_pool();
        let mut uas = std::collections::HashSet::new();
        for _ in 0..50 {
            uas.insert(pool.pick(false).ua);
        }
        assert!(
            uas.len() >= 2,
            "multiple picks should return varied UAs, got only {} unique in 50 picks",
            uas.len()
        );
    }

    #[test]
    fn test_reqwest_engine_pick_ua_mobile_vs_desktop() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let pool = engine.ua_pool();
        // mobile=true 应返回移动 profile
        for _ in 0..10 {
            let p = pool.pick(true);
            assert!(p.mobile, "pick(true) must return mobile profile");
        }
        // mobile=false 应返回桌面 profile
        for _ in 0..10 {
            let p = pool.pick(false);
            assert!(!p.mobile, "pick(false) must return desktop profile");
        }
    }

    #[test]
    fn test_reqwest_engine_pick_seeded_is_stable() {
        // R-identity-001: 同 seed 必须稳定返回同一 profile（重试时轮换 UA 的基础）
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let pool = engine.ua_pool();
        let seed = 7_u64;
        let p1 = pool.pick_seeded(seed, false);
        let p2 = pool.pick_seeded(seed, false);
        let p3 = pool.pick_seeded(seed, true);
        let p4 = pool.pick_seeded(seed, true);
        assert_eq!(p1.ua, p2.ua, "desktop: same seed must return same UA");
        assert_eq!(p3.ua, p4.ua, "mobile: same seed must return same UA");
        // desktop 和 mobile 同 seed 应返回不同 UA（来自不同分组）
        assert_ne!(p1.ua, p3.ua, "desktop and mobile must differ for same seed");
    }

    #[test]
    fn test_reqwest_engine_ua_profile_header_consistency() {
        // R-identity-001: profile 的 UA / Accept-Language / sec-ch-ua 必须一致绑定
        // - Chromium-based UA → sec-ch-ua 非空
        // - Firefox/Safari UA → sec-ch-ua 为空
        // - Accept-Language 永远非空
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let pool = engine.ua_pool();
        for p in pool.desktop.iter().chain(pool.mobile.iter()) {
            assert!(!p.ua.is_empty(), "UA must be non-empty");
            assert!(
                !p.accept_language.is_empty(),
                "Accept-Language must be non-empty"
            );
            let is_chromium = p.ua.contains("Chrome")
                || p.ua.contains("CriOS")
                || p.ua.contains("Edg")
                || p.ua.contains("SamsungBrowser");
            if is_chromium {
                assert!(
                    !p.sec_ch_ua.is_empty(),
                    "Chromium UA must have non-empty sec_ch_ua: {}",
                    p.ua
                );
            } else {
                assert!(
                    p.sec_ch_ua.is_empty(),
                    "Non-Chromium UA must have empty sec_ch_ua: {}",
                    p.ua
                );
            }
        }
    }

    #[test]
    fn test_reqwest_engine_ua_not_default_user_agent() {
        // R-identity-001: 引擎的 UA pool 应包含多个 UA，不全部等于 DEFAULT_USER_AGENT
        // （验证确实替换了固定 UA）
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        let pool = engine.ua_pool();
        let default_ua =
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
        let non_default_count = pool.desktop.iter().filter(|p| p.ua != default_ua).count();
        assert!(
            non_default_count >= 20,
            "most desktop UAs should differ from DEFAULT_USER_AGENT, got {} non-default",
            non_default_count
        );
    }

    #[test]
    fn test_reqwest_engine_with_provider_has_ua_pool() {
        // 验证所有构造路径都初始化 ua_pool
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://proxy:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider(client, pool);
        assert!(engine.ua_pool().count(false) >= 20);
        assert!(engine.ua_pool().count(true) >= 20);
    }

    #[test]
    fn test_reqwest_engine_with_timeout_has_ua_pool() {
        let client = create_test_client();
        let engine = ReqwestEngine::new_with_timeout(client, 60);
        assert!(engine.ua_pool().count(false) >= 20);
        assert!(engine.ua_pool().count(true) >= 20);
    }

    #[test]
    fn test_reqwest_engine_with_proxy_pool_and_timeout_has_ua_pool() {
        let client = create_test_client();
        let pool: Arc<dyn ProxyProvider> = Arc::new(ProxyPool::from_urls(
            vec!["http://proxy:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ));
        let engine = ReqwestEngine::with_provider_strategy_and_timeout(
            client,
            pool,
            ProxyStrategy::RoundRobin,
            45,
        );
        assert!(engine.ua_pool().count(false) >= 20);
        assert!(engine.ua_pool().count(true) >= 20);
    }
}
