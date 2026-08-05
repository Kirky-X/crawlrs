// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 引擎层 DTO 类型定义
//!
//! 包含 `EngineClient` 的请求/响应 DTO、配置类型和内部引擎接口类型。
//! 从 `engine_client.rs` 拆分而来（ARC-002），降低单文件复杂度。

use crate::common::CacheMode;
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

// HttpMethod 已提升至 `common::http_method`（CRITICAL-1 修复：消除
// 原 `infrastructure::oxcache::cache_mode`（现已提升至 `common::cache_mode`）对 `engines` 层的反向依赖）。
// 此处 `pub use` 重新导出，保持与 `ScrapeRequest` / `ScrapeOptions` 等
// 引擎类型的协同导入路径（符合代码库既有 `pub use crate::...` 惯例）。
pub use crate::common::HttpMethod;

/// Unified request structure for scraping operations.
///
/// This is the canonical request type for all scraping operations through EngineClient.
/// Callers should use this structure instead of interacting with engines directly.
#[derive(Debug, Clone)]
pub struct ScrapeRequest {
    /// The target URL to scrape
    pub url: String,
    /// Optional configuration for the scrape operation
    pub options: ScrapeOptions,
}

impl ScrapeRequest {
    /// Create a new scrape request with required URL and default options.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            options: ScrapeOptions::default(),
        }
    }

    /// Create a request with a URL and custom options builder.
    pub fn with_options(mut self, options: ScrapeOptions) -> Self {
        self.options = options;
        self
    }

    /// Configure the request to require JavaScript rendering.
    pub fn needs_js(mut self) -> Self {
        self.options.needs_js = true;
        self
    }

    /// Configure the request to require a screenshot.
    pub fn needs_screenshot(mut self) -> Self {
        self.options.needs_screenshot = true;
        self
    }

    /// Configure the request to use a mobile user agent.
    pub fn mobile(mut self) -> Self {
        self.options.mobile = true;
        self
    }

    /// Set a custom timeout for the request.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.options.timeout = duration;
        self
    }
}

/// session_id 最大长度（字节）— 防止 DoS（T056 安全审查 MEDIUM-2 修复）
///
/// 限制 ProxyPool::sticky 的 DashMap key 长度，防止恶意超长 session_id
/// 导致内存耗尽。128 字节对绝大多数会话标识场景足够。
pub const MAX_SESSION_ID_LEN: usize = 128;

/// 校验 session_id 是否合法（T056 安全审查 MEDIUM-2 修复）
///
/// # 校验规则
///
/// - 长度 <= [`MAX_SESSION_ID_LEN`] 字节
/// - 字符集为可打印 ASCII（0x20-0x7E），排除控制字符
///
/// # 防护
///
/// - 超长字符串 → 防止 DashMap key 内存耗尽（DoS）
/// - 控制字符/换行符 → 防止日志注入（CWE-117）
///
/// # 返回
///
/// - `true`：合法
/// - `false`：非法
#[must_use]
pub fn validate_session_id(session_id: &str) -> bool {
    if session_id.len() > MAX_SESSION_ID_LEN {
        return false;
    }
    session_id.bytes().all(|b| (0x20..=0x7E).contains(&b))
}

/// 页面加载后等待策略（T069，R-jsrender-004）
///
/// 三种模式：
/// - [`WaitFor::NetworkIdle`]：等待网络空闲（无新请求持续 500ms）
/// - [`WaitFor::Selector(String)`]：等待指定 CSS selector 出现在 DOM 中
/// - [`WaitFor::DomStable(Duration)`]：等待 DOM 稳定（无变化持续指定时长）
///
/// # 设计动机
///
/// 原有 `sync_wait_ms` 是固定 sleep，无论页面是否就绪都阻塞相同时间。
/// `WaitFor` 提供条件式等待：满足条件立即返回，超时返回错误，避免无谓阻塞。
/// 在 Playwright 引擎中替代 `sync_wait_ms` 的固定 sleep 逻辑。
///
/// # 实现
///
/// 枚举本身不依赖 `chromiumoxide`，可被非浏览器引擎代码持有。
/// `wait` 方法的实现在 `engines/wait.rs`（`engine-playwright` feature 门控），
/// 依赖 `chromiumoxide::Page`。
///
/// # 安全性
///
/// `Selector` 模式对 selector 字符串做 JS 字符串转义，防注入（CWE-94）。
/// `DomStable` 的 `stable_duration` 上限 60s，防恶意调用方设置过长导致 DoS。
#[derive(Debug, Clone, PartialEq, Default)]
pub enum WaitFor {
    /// 等待网络空闲（无新请求持续 500ms）
    ///
    /// chromiumoxide 的 `goto` 已等待 load 事件，此处额外等待确保异步请求完成。
    #[default]
    NetworkIdle,
    /// 等待指定 CSS selector 出现在 DOM 中
    ///
    /// selector 字符串会在 JS 中转义，防注入。
    Selector(String),
    /// 等待 DOM 稳定（无变化持续指定时长）
    ///
    /// 通过轮询 `document.body.innerHTML.length` 比较，若连续 `stable_duration`
    /// 内长度不变则视为稳定。`stable_duration` 上限 60s。
    DomStable(Duration),
}

/// Optional configuration for scrape operations.
#[derive(Debug, Clone)]
pub struct ScrapeOptions {
    /// HTTP method for the request
    pub method: HttpMethod,
    /// Whether JavaScript rendering is required (default: false)
    pub needs_js: bool,
    /// Whether screenshot capture is required (default: false)
    pub needs_screenshot: bool,
    /// Whether to use mobile user agent (default: false)
    pub mobile: bool,
    /// Request timeout duration (default: 30 seconds)
    pub timeout: Duration,
    /// Optional request body
    pub body: Option<String>,
    /// Sync wait duration in milliseconds after page load (default: 0)
    pub sync_wait_ms: u32,
    /// Page actions to perform (clicks, scrolls, etc.)
    pub actions: Vec<PageAction>,
    /// Screenshot configuration
    pub screenshot_config: Option<ScreenshotConfig>,
    /// Proxy URL (optional)
    pub proxy: Option<String>,
    /// Skip TLS verification (default: false)
    pub skip_tls_verification: bool,
    /// Custom HTTP headers (default: empty)
    pub headers: HashMap<String, String>,
    /// Enable TLS fingerprinting for anti-fingerprinting (default: false)
    pub needs_tls_fingerprint: bool,
    /// Force use of Fire Engine (CDP) for this request (default: false)
    pub use_fire_engine: bool,
    /// Block ad / tracker domains via CDP Fetch request interception (default: false)
    ///
    /// T033 / R-jsrender-003：当为 true 且使用浏览器引擎时，启用广告/追踪域名黑名单拦截，
    /// 命中 [`crate::engines::intercept::AD_DOMAIN_BLACKLIST`] 的请求将被 `Fetch.failRequest` 中止。
    pub block_ads: bool,
    /// Block media resources (image/media/font) via CDP Fetch interception (default: false)
    ///
    /// T033 / R-jsrender-003：当为 true 且使用浏览器引擎时，启用媒体资源类型拦截，
    /// CDP `ResourceType::{Image, Media, Font}` 的请求将被 `Fetch.failRequest` 中止。
    pub block_media: bool,
    /// 粘性会话 ID（H1 修复：用于 ProxyStrategy::Sticky 时调用 ProxyProvider::sticky）
    ///
    /// 调用方在需要粘性会话（同一会话固定走同一代理）时设置。
    /// `None` 时按 `ProxyStrategy::RoundRobin` 走 `ProxyProvider::next`。
    pub session_id: Option<String>,
    /// 缓存模式（T058/R-cache-002，design.md §13）
    ///
    /// 控制本次抓取的缓存读写行为。`None`（默认）等价于 `Some(CacheMode::Enabled)`，
    /// 由 `scrape_worker` 在读写 `CacheService` 前经 `CacheContext` 门控。
    ///
    /// 5 种模式详见 [`crate::common::CacheMode`]。
    pub cache_mode: Option<CacheMode>,
    /// 页面加载后等待策略（T069，R-jsrender-004，design.md §17）
    ///
    /// 仅浏览器引擎（Playwright）生效。`None`（默认）时 Playwright 使用
    /// [`WaitFor::NetworkIdle`]（与原 `sync_wait_ms` 默认 1 秒等待语义一致）。
    ///
    /// 设置后**替代** Playwright 中基于 `sync_wait_ms` 的固定 sleep 逻辑：
    /// 满足条件立即返回，超时返回 `EngineError::BrowserError`。
    ///
    /// `sync_wait_ms` 字段保留供非浏览器引擎（如 FlareSolverr）使用。
    pub wait_for: Option<WaitFor>,
}

impl Default for ScrapeOptions {
    fn default() -> Self {
        Self {
            method: HttpMethod::Get,
            needs_js: false,
            needs_screenshot: false,
            mobile: false,
            timeout: Duration::from_secs(30),
            body: None,
            sync_wait_ms: 0,
            actions: Vec::new(),
            screenshot_config: None,
            proxy: None,
            skip_tls_verification: false,
            headers: HashMap::new(),
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            block_ads: false,
            block_media: false,
            session_id: None,
            cache_mode: None,
            wait_for: None,
        }
    }
}

impl ScrapeOptions {
    /// Create options builder.
    pub fn builder() -> ScrapeOptionsBuilder {
        ScrapeOptionsBuilder::default()
    }
}

/// Builder for ScrapeOptions.
#[derive(Debug, Clone, Default)]
pub struct ScrapeOptionsBuilder(ScrapeOptions);

impl ScrapeOptionsBuilder {
    pub fn method(mut self, method: HttpMethod) -> Self {
        self.0.method = method;
        self
    }

    pub fn needs_js(mut self, enabled: bool) -> Self {
        self.0.needs_js = enabled;
        self
    }

    pub fn needs_screenshot(mut self, enabled: bool) -> Self {
        self.0.needs_screenshot = enabled;
        self
    }

    pub fn mobile(mut self, enabled: bool) -> Self {
        self.0.mobile = enabled;
        self
    }

    pub fn timeout(mut self, duration: Duration) -> Self {
        self.0.timeout = duration;
        self
    }

    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.0.body = Some(body.into());
        self
    }

    pub fn sync_wait_ms(mut self, ms: u32) -> Self {
        self.0.sync_wait_ms = ms;
        self
    }

    pub fn proxy(mut self, proxy: impl Into<String>) -> Self {
        self.0.proxy = Some(proxy.into());
        self
    }

    /// Configure whether to skip TLS certificate verification.
    ///
    /// # Security Warning
    ///
    /// Skipping TLS verification is **FORBIDDEN** in production environments.
    /// This option is only available in development/test environments for testing purposes.
    ///
    /// In production, attempting to skip TLS verification will:
    /// - Log a security warning
    /// - Ignore the skip request (TLS verification remains enabled)
    ///
    /// # Arguments
    ///
    /// * `skip` - Whether to skip TLS verification (ignored in production)
    ///
    /// # Returns
    ///
    /// Returns the builder with TLS verification settings applied.
    pub fn skip_tls_verification(mut self, skip: bool) -> Self {
        if skip {
            // Check environment - use both APP_ENVIRONMENT and CRAWLRS_ENV for compatibility
            let env = std::env::var("APP_ENVIRONMENT")
                .or_else(|_| std::env::var("CRAWLRS_ENV"))
                .unwrap_or_else(|_| "development".to_string());

            let is_production =
                env.eq_ignore_ascii_case("production") || env.eq_ignore_ascii_case("prod");

            if is_production {
                // SECURITY: Reject TLS verification skip in production
                warn!(
                    target: "security",
                    "SECURITY ALERT: Attempt to skip TLS verification in production environment '{}' - DENIED. \
                     TLS verification will remain enabled to prevent man-in-the-middle attacks.",
                    env
                );
                // Return without modifying the setting - TLS verification stays enabled
                return self;
            }

            // Allow skip in non-production environments with warning
            warn!(
                target: "security",
                "TLS certificate verification disabled in '{}' environment. \
                 This should ONLY be used for testing purposes. \
                 NEVER disable TLS verification in production as it enables man-in-the-middle attacks.",
                env
            );
        }
        self.0.skip_tls_verification = skip;
        self
    }

    pub fn headers(mut self, headers: HashMap<String, String>) -> Self {
        self.0.headers = headers;
        self
    }

    pub fn needs_tls_fingerprint(mut self, enabled: bool) -> Self {
        self.0.needs_tls_fingerprint = enabled;
        self
    }

    pub fn use_fire_engine(mut self, enabled: bool) -> Self {
        self.0.use_fire_engine = enabled;
        self
    }

    pub fn screenshot_config(mut self, config: ScreenshotConfig) -> Self {
        self.0.screenshot_config = Some(config);
        self
    }

    /// Enable/disable ad & tracker domain blocking via CDP Fetch interception (T033, R-jsrender-003).
    pub fn block_ads(mut self, enabled: bool) -> Self {
        self.0.block_ads = enabled;
        self
    }

    /// Enable/disable media resource (image/media/font) blocking via CDP Fetch interception (T033).
    pub fn block_media(mut self, enabled: bool) -> Self {
        self.0.block_media = enabled;
        self
    }

    /// 设置粘性会话 ID（H1 修复：用于 ProxyStrategy::Sticky）
    ///
    /// 调用方在需要粘性会话（同一会话固定走同一代理）时设置。
    /// `None` 时按 `ProxyStrategy::RoundRobin` 走 `ProxyProvider::next`。
    ///
    /// # 安全校验（T056 安全审查 MEDIUM-2 修复）
    ///
    /// - 长度上限：[`MAX_SESSION_ID_LEN`] 字节（128）
    /// - 字符集：可打印 ASCII（0x20-0x7E），排除控制字符
    ///
    /// 非法输入（超长或含控制字符）会被拒绝并记录 warn 日志，`session_id` 保持 `None`。
    /// 防止 DoS（超长字符串填充 sticky binding map）和日志注入（CWE-117）。
    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        let sid = session_id.into();
        if validate_session_id(&sid) {
            self.0.session_id = Some(sid);
        } else {
            warn!(
                "session_id rejected (length={}, max={}, must be printable ASCII); \
                 falling back to None (RoundRobin)",
                sid.len(),
                MAX_SESSION_ID_LEN
            );
        }
        self
    }

    pub fn build(self) -> ScrapeOptions {
        self.0
    }

    /// 设置缓存模式（T058/R-cache-002，design.md §13）
    ///
    /// `None`（默认）等价于 `Some(CacheMode::Enabled)`。
    /// 详见 [`crate::common::CacheMode`]。
    pub fn cache_mode(mut self, mode: impl Into<Option<CacheMode>>) -> Self {
        self.0.cache_mode = mode.into();
        self
    }

    /// 设置页面加载后等待策略（T069，R-jsrender-004，design.md §17）
    ///
    /// 仅浏览器引擎（Playwright）生效。`None` 时使用 [`WaitFor::NetworkIdle`]。
    /// 详见 [`WaitFor`]。
    #[must_use]
    pub fn wait_for(mut self, wait: impl Into<Option<WaitFor>>) -> Self {
        self.0.wait_for = wait.into();
        self
    }
}

/// Page action to perform during scraping.
#[derive(Debug, Clone)]
pub enum PageAction {
    /// Wait for specified milliseconds
    Wait { milliseconds: u64 },
    /// Click element by CSS selector
    Click { selector: String },
    /// Scroll in direction
    Scroll { direction: ScrollDirection },
    /// Input text into element
    Input { selector: String, text: String },
}

/// Scroll direction for PageAction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollDirection {
    #[default]
    Down,
    Up,
    Bottom,
    Top,
}

/// Screenshot configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotConfig {
    /// Capture full page (default: true)
    pub full_page: bool,
    /// CSS selector for element-specific screenshot
    pub selector: Option<String>,
    /// Image quality 1-100 (for JPEG, default: 80)
    pub quality: Option<u8>,
    /// Image format (default: "jpeg")
    pub format: Option<String>,
}

impl Default for ScreenshotConfig {
    fn default() -> Self {
        Self {
            full_page: true,
            selector: None,
            quality: Some(80),
            format: Some("jpeg".to_string()),
        }
    }
}

/// Unified response structure for scraping operations.
///
/// This is the canonical response type returned by EngineClient.
///
/// T059/R-cache-002：实现 `Serialize`/`Deserialize` 以支持 `scrape_worker`
/// 缓存门控——抓取成功后序列化为 JSON 字符串写入 `CacheService`，
/// 读缓存命中时反序列化还原为 `ScrapeResponse` 直返，跳过实际抓取。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeResponse {
    /// HTTP status code
    pub status_code: u16,
    /// Response content (HTML or extracted text)
    pub content: String,
    /// Base64-encoded screenshot (if requested)
    pub screenshot: Option<String>,
    /// Response content type
    pub content_type: String,
    /// Response headers
    pub headers: HashMap<String, String>,
    /// Time taken to complete request in milliseconds
    pub response_time_ms: u64,
    /// Final URL after any redirects
    pub final_url: Option<String>,
    /// Markdown 转换结果（T041/R-content-001）
    ///
    /// 仅当请求 `formats` 含 `"markdown"` 且 `markdown` 特性启用时由
    /// `scrape_worker` 填充；其余情况为 `None`，对老调用方透明。
    pub markdown: Option<String>,
}

impl ScrapeResponse {
    /// Create a new response.
    pub fn new(
        status_code: u16,
        content: impl Into<String>,
        content_type: impl Into<String>,
    ) -> Self {
        Self {
            status_code,
            content: content.into(),
            screenshot: None,
            content_type: content_type.into(),
            headers: HashMap::new(),
            response_time_ms: 0,
            final_url: None,
            markdown: None,
        }
    }

    /// Check if the request was successful (2xx status code).
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }
}

// === Internal Request/Response Types for Router ===
// These are used internally by EngineRouter and engines

/// Internal request type for engine operations
#[derive(Debug, Clone)]
pub struct InternalScrapeRequest {
    pub url: String,
    pub method: HttpMethod,
    pub headers: HashMap<String, String>,
    pub timeout: Duration,
    pub needs_js: bool,
    pub needs_screenshot: bool,
    pub screenshot_config: Option<InternalScreenshotConfig>,
    pub mobile: bool,
    pub proxy: Option<String>,
    pub skip_tls_verification: bool,
    pub needs_tls_fingerprint: bool,
    pub use_fire_engine: bool,
    pub actions: Vec<InternalPageAction>,
    pub body: Option<String>,
    pub sync_wait_ms: u32,
    /// T033 / R-jsrender-003：广告/追踪域名拦截开关（仅浏览器引擎生效）
    pub block_ads: bool,
    /// T033 / R-jsrender-003：媒体资源类型拦截开关（仅浏览器引擎生效）
    pub block_media: bool,
    /// 粘性会话 ID（H1 修复：用于 ProxyStrategy::Sticky 时调用 ProxyProvider::sticky）
    ///
    /// 通过 `ScrapeRequest::to_internal` 从 `ScrapeOptions.session_id` 桥接而来。
    pub session_id: Option<String>,
    /// 页面加载后等待策略（T069，R-jsrender-004，design.md §17）
    ///
    /// 通过 `ScrapeRequest::to_internal` 从 `ScrapeOptions.wait_for` 桥接而来。
    /// 仅浏览器引擎（Playwright）消费；`None` 时 Playwright 使用 [`WaitFor::NetworkIdle`]。
    pub wait_for: Option<WaitFor>,
}

/// Internal screenshot configuration
#[derive(Debug, Clone)]
pub struct InternalScreenshotConfig {
    pub full_page: bool,
    pub selector: Option<String>,
    pub quality: Option<u8>,
    pub format: Option<String>,
}

/// Internal page action for engine operations
#[derive(Debug, Clone)]
pub enum InternalPageAction {
    Wait { milliseconds: u64 },
    Click { selector: String },
    Scroll { direction: String },
    Input { selector: String, text: String },
    Screenshot { full_page: bool },
}

/// Internal response type for engine operations
#[derive(Debug, Clone)]
pub struct InternalScrapeResponse {
    pub status_code: u16,
    pub content: String,
    pub screenshot: Option<String>,
    pub content_type: String,
    pub headers: HashMap<String, String>,
    pub response_time_ms: u64,
}

/// Convert from public ScrapeRequest to internal format
impl ScrapeRequest {
    #[inline]
    pub(crate) fn to_internal(&self) -> InternalScrapeRequest {
        let options = &self.options;

        let actions: Vec<InternalPageAction> = options
            .actions
            .iter()
            .map(|action| match action {
                PageAction::Wait { milliseconds } => InternalPageAction::Wait {
                    milliseconds: *milliseconds,
                },
                PageAction::Click { selector } => InternalPageAction::Click {
                    selector: selector.clone(),
                },
                PageAction::Scroll { direction } => {
                    let direction_str = match direction {
                        ScrollDirection::Down => "down",
                        ScrollDirection::Up => "up",
                        ScrollDirection::Bottom => "bottom",
                        ScrollDirection::Top => "top",
                    };
                    InternalPageAction::Scroll {
                        direction: direction_str.to_string(),
                    }
                }
                PageAction::Input { selector, text } => InternalPageAction::Input {
                    selector: selector.clone(),
                    text: text.clone(),
                },
            })
            .collect();

        let screenshot_config =
            options
                .screenshot_config
                .as_ref()
                .map(|config| InternalScreenshotConfig {
                    full_page: config.full_page,
                    selector: config.selector.clone(),
                    quality: config.quality,
                    format: config.format.clone(),
                });

        // T056 安全审查 MEDIUM-2 修复：to_internal 二次校验 session_id
        // 防止用户绕过 builder 直接构造 ScrapeOptions 注入非法 session_id
        // （超长字符串 DoS / 控制字符日志注入 CWE-117）
        let session_id = match &options.session_id {
            Some(sid) if validate_session_id(sid) => Some(sid.clone()),
            Some(sid) => {
                warn!(
                    "session_id rejected in to_internal (length={}, max={}, \
                     must be printable ASCII); falling back to None (RoundRobin)",
                    sid.len(),
                    MAX_SESSION_ID_LEN
                );
                None
            }
            None => None,
        };

        InternalScrapeRequest {
            url: self.url.clone(),
            method: options.method,
            headers: options.headers.clone(),
            timeout: options.timeout,
            needs_js: options.needs_js,
            needs_screenshot: options.needs_screenshot,
            screenshot_config,
            mobile: options.mobile,
            proxy: options.proxy.clone(),
            skip_tls_verification: options.skip_tls_verification,
            needs_tls_fingerprint: options.needs_tls_fingerprint,
            use_fire_engine: options.use_fire_engine,
            actions,
            body: options.body.clone(),
            sync_wait_ms: options.sync_wait_ms,
            block_ads: options.block_ads,
            block_media: options.block_media,
            session_id,
            wait_for: options.wait_for.clone(),
        }
    }
}

/// Convert from internal ScrapeResponse to public format
impl InternalScrapeResponse {
    #[inline]
    pub fn to_public(&self, original_url: &str) -> ScrapeResponse {
        ScrapeResponse {
            status_code: self.status_code,
            content: self.content.clone(),
            screenshot: self.screenshot.clone(),
            content_type: self.content_type.clone(),
            headers: self.headers.clone(),
            response_time_ms: self.response_time_ms,
            final_url: Some(original_url.to_string()),
            markdown: None,
        }
    }
}
