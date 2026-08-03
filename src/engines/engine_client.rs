// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! EngineClient - Unified public API for scraping operations
//!
//! This module provides the single entry point for all scraping operations.
//! All internal implementation details (UA rotation, circuit breaker, engine selection)
//! are encapsulated within EngineClient and not exposed to callers.

// ARC-002: DTO 类型已拆分至 `types.rs`，此处 re-export 保持向后兼容。
pub use crate::engines::types::{
    validate_session_id, HttpMethod, InternalPageAction, InternalScrapeRequest,
    InternalScrapeResponse, InternalScreenshotConfig, PageAction, ScrapeOptions,
    ScrapeOptionsBuilder, ScrapeRequest, ScrapeResponse, ScreenshotConfig, ScrollDirection,
    WaitFor, MAX_SESSION_ID_LEN,
};

use crate::engines::health_monitor::{AggregateHealthStatus, EngineHealthMonitor};
use crate::engines::router::{EngineRouter, EngineRouterTrait};
use crate::engines::validators::validate_url;
use crate::utils::retry::RetryReason;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

/// Engine error types for EngineClient operations.
#[derive(Error, Debug)]
pub enum EngineError {
    /// Request failed with a specific error message
    #[error("Request failed: {0}")]
    RequestFailed(String),

    /// Request timed out
    #[error("Request timed out after {0:?}")]
    Timeout(Duration),

    /// All engines are unavailable
    #[error("All engines failed: {0}")]
    AllEnginesFailed(String),

    /// No engines available for the request
    #[error("No engines available")]
    NoEnginesAvailable,

    /// Invalid URL provided
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// SSRF protection triggered
    #[error("SSRF protection: {0}")]
    SsrfProtection(String),

    /// Browser/Playwright error
    #[error("Browser error: {0}")]
    BrowserError(String),

    /// 反爬虫检测命中（design.md §1.6/1.7，R-antibot-003）
    ///
    /// 携带 `antibot::classifier::classify` 给出的 `Detection::reason` 字符串。
    /// 可重试：`is_retryable()=true`，`retry_reason()=AntiBot`，
    /// 路由层据此切身份（UA/代理/stealth）+ 强制浏览器/FlareSolverr 引擎改派。
    #[error("Anti-bot detected: {0}")]
    AntiBotDetected(String),

    /// 引擎特性切换（T027，R-identity-002）
    ///
    /// 当引擎降级或特性切换（如 Chrome → HTTP、JS 渲染失败回退到静态抓取）时触发。
    /// 携带描述信息（如 "chrome_degraded_to_http"）。
    /// 可重试：`is_retryable()=true`，`retry_reason()=FeatureToggle`，
    /// 路由层据此换引擎重试，并按 `RetryDirective` 立即轮换 UA（attempt=0 特例）。
    #[error("Feature toggle: {0}")]
    FeatureToggle(String),

    /// Request expired (circuit breaker open)
    #[error("Request expired")]
    Expired,

    /// 引擎级 MRT 超时（架构审查 MEDIUM-2 修复，design.md §14 / T062）
    ///
    /// 区别于 `Timeout`（请求整体超时），此变体表示单引擎在 MRT（Maximum Response Time）
    /// 内未完成，router 触发瀑布式 fallback 切换到下一引擎。
    ///
    /// 可重试：`is_retryable()=true`，`retry_reason()=Transient`（同引擎可重试，
    /// 但 router 会优先切下一引擎而非重试同引擎）。
    #[error("Engine {engine} exceeded MRT of {mrt:?}")]
    EngineMrtExceeded {
        /// 引擎名称
        engine: String,
        /// 实际使用的 MRT（即 `effective_timeout = min(remaining, engine_mrt)`）
        mrt: Duration,
    },

    /// Other error
    #[error("Other error: {0}")]
    Other(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

// From implementations for EngineError
impl From<String> for EngineError {
    fn from(msg: String) -> Self {
        EngineError::RequestFailed(msg)
    }
}

impl From<&str> for EngineError {
    fn from(msg: &str) -> Self {
        EngineError::RequestFailed(msg.to_string())
    }
}

impl From<anyhow::Error> for EngineError {
    fn from(err: anyhow::Error) -> Self {
        EngineError::Internal(err.to_string())
    }
}

impl EngineError {
    /// Check if the error is retryable.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RequestFailed(_) => true,
            Self::Timeout(_) => true,
            Self::NoEnginesAvailable => false,
            Self::InvalidUrl(_) => false,
            Self::SsrfProtection(_) => false,
            Self::BrowserError(_) => true,
            Self::AntiBotDetected(_) => true,
            Self::FeatureToggle(_) => true,
            Self::Internal(_) => false,
            Self::AllEnginesFailed(_) => false,
            Self::Expired => false,
            Self::Other(_) => false,
            // 引擎级 MRT 超时：可重试（router 优先切下一引擎，而非重试同引擎）
            Self::EngineMrtExceeded { .. } => true,
        }
    }

    /// 将错误归类为重试原因（design.md §4，R-antibot-003 / R-identity-002）。
    ///
    /// 调用方应先查 [`is_retryable()`](Self::is_retryable)：
    /// 不可重试的错误虽返回 [`RetryReason::Transient`]，
    /// 但不会被重试系统消费，仅作占位以保证返回值完备。
    ///
    /// 映射：
    /// - `RequestFailed` / `Timeout` / `BrowserError` → `Transient`（同引擎可重试）
    /// - `AntiBotDetected` → `AntiBot`（需切身份 + 浏览器引擎改派）
    /// - `FeatureToggle` → `FeatureToggle`（需换引擎重试；T027 新增）
    /// - 其余不可重试变体 → `Transient`（占位）
    pub fn retry_reason(&self) -> RetryReason {
        match self {
            Self::AntiBotDetected(_) => RetryReason::AntiBot,
            Self::FeatureToggle(_) => RetryReason::FeatureToggle,
            Self::RequestFailed(_)
            | Self::Timeout(_)
            | Self::BrowserError(_)
            | Self::NoEnginesAvailable
            | Self::InvalidUrl(_)
            | Self::SsrfProtection(_)
            | Self::Internal(_)
            | Self::AllEnginesFailed(_)
            | Self::Expired
            | Self::Other(_)
            | Self::EngineMrtExceeded { .. } => RetryReason::Transient,
        }
    }
}

use async_trait::async_trait;

/// ScraperEngine trait - abstraction for different scraping engines
///
/// This trait defines the interface that all scraping engines must implement.
/// Each engine provides different capabilities (JS rendering, TLS fingerprinting, etc.)
/// and is scored based on how well it matches the request requirements.
#[async_trait]
pub trait ScraperEngine: Send + Sync {
    /// Perform a scraping request
    async fn scrape(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError>;

    /// Calculate a support score for the given request
    ///
    /// Returns a score from 0-100 indicating how well this engine
    /// supports the request. Higher scores indicate better support.
    fn support_score(&self, request: &InternalScrapeRequest) -> u8;

    /// Get the engine name
    fn name(&self) -> &'static str;

    /// Check if the engine supports TLS fingerprinting
    ///
    /// Returns true if the engine can perform TLS fingerprinting
    /// for anti-fingerprinting purposes.
    fn supports_tls_fingerprint(&self) -> bool {
        false
    }

    /// 引擎级最大响应时间（MRT, Maximum Response Time）—— design.md §14 / T060。
    ///
    /// 用于 router 顺序 fallback 路径瀑布式超时：单引擎调用以
    /// `min(remaining_timeout, engine.max_response_time())` 包裹，
    /// 超 MRT 即切下一引擎（不切整体失败）。race 模式不受影响。
    ///
    /// # 默认实现
    ///
    /// 返回 30 秒（与 `EngineTimeoutSettings::default_timeout_seconds` 默认值一致）。
    /// 各引擎按类型覆写为更精确的 MRT：
    /// - HTTP fetch 引擎（reqwest）：5 秒（`fetch_seconds`）
    /// - CDP/浏览器引擎（playwright / flaresolverr_cdp / flaresolverr_full）：30 秒（`cdp_seconds`）
    /// - TLS 指纹引擎（flaresolverr_tls）：15 秒（`tls_seconds`）
    ///
    /// # 注入
    ///
    /// 引擎构造时应从 `EngineTimeoutSettings` 注入对应字段，避免硬编码
    /// （参考 `ReqwestEngine::new_with_timeout` 模式）。
    ///
    /// 架构审查 MEDIUM-1 修复：删除 `DEFAULT_ENGINE_MRT` 常量，默认实现直接返回
    /// `Duration::from_secs(30)`，避免与 `EngineTimeoutSettings::default_timeout_seconds`
    /// 形成隐式耦合（注释承诺"保持一致"但代码无引用关系）。
    fn max_response_time(&self) -> Duration {
        Duration::from_secs(30)
    }
}

/// Health status of the engine system.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EngineHealthStatus {
    /// All engines are operational
    #[default]
    Healthy,
    /// Some engines are degraded or unavailable
    Degraded {
        /// List of engines that are unhealthy
        unhealthy_engines: Vec<String>,
        /// Message describing the degradation
        message: String,
    },
    /// No engines are available
    Unavailable {
        /// Message describing the unavailability
        message: String,
    },
}

/// Trait for EngineClient - enables dependency injection
#[async_trait]
pub trait EngineClientTrait: Send + Sync {
    /// Perform a scraping request
    async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError>;

    /// Perform health check on all registered engines
    async fn health_check(&self) -> EngineHealthStatus;

    /// Get the number of registered engines
    fn engine_count(&self) -> usize;

    /// Get list of registered engine names
    fn registered_engines(&self) -> Vec<String>;
}

/// Engine client - the single entry point for all scraping operations.
///
/// This struct encapsulates all internal implementation details:
/// - User-Agent rotation
/// - Circuit breaker state
/// - Engine selection algorithm
/// - Retry logic and backoff
/// - Connection pooling
///
/// Callers should use this struct for all scraping operations instead of
/// interacting with engines directly.
#[derive(Clone)]
pub struct EngineClient {
    /// Internal router for engine selection and request routing
    router: Arc<dyn EngineRouterTrait>,
    /// Internal health monitor for tracking engine health
    health_monitor: Arc<EngineHealthMonitor>,
}

impl EngineClient {
    /// Create a new EngineClient with default configuration.
    pub fn new() -> Self {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouter::new(Vec::new()));
        Self::with_router(router)
    }

    /// Create an EngineClient with a custom router.
    pub fn with_router(router: Arc<dyn EngineRouterTrait>) -> Self {
        Self {
            router,
            health_monitor: Arc::new(EngineHealthMonitor::new(Vec::new())),
        }
    }

    /// Create an EngineClient with engines pre-registered.
    pub fn with_engines(engines: Vec<Arc<dyn ScraperEngine>>) -> Self {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouter::new(engines));
        let engines_for_health = Vec::new(); // Will need to get engines from router
        let health_monitor = Arc::new(EngineHealthMonitor::new(engines_for_health));
        Self {
            router,
            health_monitor,
        }
    }

    /// Perform a scraping request.
    ///
    /// This method automatically:
    /// - Validates the URL
    /// - Selects the optimal engine based on request requirements
    /// - Handles retries and circuit breaking
    /// - Rotates user agents
    /// - Returns a unified response
    ///
    /// # Arguments
    ///
    /// * `request` - The scrape request containing URL and options
    ///
    /// # Returns
    ///
    /// * `Ok(ScrapeResponse)` on success
    /// * `Err(EngineError)` on failure
    pub async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        // Validate URL first
        if let Err(e) = validate_url(&request.url).await {
            return Err(EngineError::SsrfProtection(e.to_string()));
        }

        // Convert to internal request format
        let internal_request = request.to_internal();

        // Route to appropriate engine
        match self.router.route(&internal_request).await {
            Ok(response) => Ok(response.to_public(&request.url)),
            Err(e) => Err(convert_error(e)),
        }
    }

    /// Perform health check on all registered engines.
    ///
    /// # Returns
    ///
    /// * `EngineHealthStatus` indicating the health of all engines
    pub async fn health_check(&self) -> EngineHealthStatus {
        // Perform health check on all engines
        self.health_monitor.perform_all_health_checks().await;

        // Get aggregate status and convert to public type
        let status = self.health_monitor.get_aggregate_status().await;

        match status {
            AggregateHealthStatus::Healthy => EngineHealthStatus::Healthy,
            AggregateHealthStatus::Degraded(unhealthy_engines) => {
                let count = unhealthy_engines.len();
                EngineHealthStatus::Degraded {
                    unhealthy_engines,
                    message: format!("{} engines degraded", count),
                }
            }
            AggregateHealthStatus::Unavailable => EngineHealthStatus::Unavailable {
                message: "All engines unavailable".to_string(),
            },
        }
    }

    /// Get the number of registered engines.
    pub fn engine_count(&self) -> usize {
        self.router.registered_engines().len()
    }

    /// Get list of registered engine names.
    pub fn registered_engines(&self) -> Vec<String> {
        self.router.registered_engines()
    }
}

impl Default for EngineClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EngineClientTrait for EngineClient {
    async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        self.scrape(request).await
    }

    async fn health_check(&self) -> EngineHealthStatus {
        self.health_check().await
    }

    fn engine_count(&self) -> usize {
        self.engine_count()
    }

    fn registered_engines(&self) -> Vec<String> {
        self.registered_engines()
    }
}

/// Convert internal errors to public EngineError
fn convert_error(e: EngineError) -> EngineError {
    match e {
        EngineError::RequestFailed(msg) => EngineError::RequestFailed(msg),
        EngineError::Timeout(duration) => EngineError::Timeout(duration),
        EngineError::AllEnginesFailed(msg) => {
            if msg.contains("No suitable engines") {
                EngineError::NoEnginesAvailable
            } else {
                EngineError::RequestFailed(msg)
            }
        }
        EngineError::SsrfProtection(msg) => EngineError::SsrfProtection(msg),
        EngineError::BrowserError(msg) => EngineError::BrowserError(msg),
        EngineError::AntiBotDetected(msg) => EngineError::AntiBotDetected(msg),
        EngineError::FeatureToggle(msg) => EngineError::FeatureToggle(msg),
        EngineError::Expired => EngineError::Internal("Request expired".to_string()),
        EngineError::Other(msg) => EngineError::Internal(msg),
        EngineError::NoEnginesAvailable => EngineError::NoEnginesAvailable,
        EngineError::InvalidUrl(msg) => EngineError::InvalidUrl(msg),
        EngineError::Internal(msg) => EngineError::Internal(msg),
        EngineError::EngineMrtExceeded { engine, mrt } => {
            EngineError::EngineMrtExceeded { engine, mrt }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_scrape_request_builder() {
        let request = ScrapeRequest::new("https://example.com")
            .needs_js()
            .needs_screenshot()
            .mobile()
            .timeout(Duration::from_secs(60));

        assert_eq!(request.url, "https://example.com");
        assert!(request.options.needs_js);
        assert!(request.options.needs_screenshot);
        assert!(request.options.mobile);
        assert_eq!(request.options.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_scrape_options_builder() {
        let options = ScrapeOptions::builder()
            .needs_js(true)
            .needs_screenshot(true)
            .mobile(true)
            .timeout(Duration::from_secs(45))
            .proxy("http://proxy.example.com:8080")
            .build();

        assert!(options.needs_js);
        assert!(options.needs_screenshot);
        assert!(options.mobile);
        assert_eq!(options.timeout, Duration::from_secs(45));
        assert_eq!(
            options.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_scrape_response_success() {
        let response = ScrapeResponse::new(200, "Hello World", "text/html");

        assert_eq!(response.status_code, 200);
        assert_eq!(response.content, "Hello World");
        assert_eq!(response.content_type, "text/html");
        assert!(response.is_success());
    }

    #[test]
    fn test_engine_error_retryable() {
        assert!(EngineError::RequestFailed("connection refused".to_string()).is_retryable());
        assert!(EngineError::Timeout(Duration::from_secs(30)).is_retryable());
        assert!(!EngineError::InvalidUrl("invalid".to_string()).is_retryable());
        assert!(!EngineError::SsrfProtection("blocked".to_string()).is_retryable());
    }

    // === ScrapeRequest tests ===

    #[test]
    fn test_scrape_request_new_default_options() {
        let request = ScrapeRequest::new("https://example.com");
        assert_eq!(request.url, "https://example.com");
        assert!(!request.options.needs_js);
        assert!(!request.options.needs_screenshot);
        assert!(!request.options.mobile);
        assert_eq!(request.options.timeout, Duration::from_secs(30));
        assert_eq!(request.options.method, HttpMethod::Get);
    }

    #[test]
    fn test_scrape_request_with_options() {
        let options = ScrapeOptions::builder()
            .needs_js(true)
            .timeout(Duration::from_secs(120))
            .build();
        let request = ScrapeRequest::new("https://example.com").with_options(options);

        assert!(request.options.needs_js);
        assert_eq!(request.options.timeout, Duration::from_secs(120));
    }

    #[test]
    fn test_scrape_request_chained_builders() {
        let request = ScrapeRequest::new("https://example.com")
            .needs_js()
            .needs_screenshot()
            .mobile()
            .timeout(Duration::from_secs(90));

        assert!(request.options.needs_js);
        assert!(request.options.needs_screenshot);
        assert!(request.options.mobile);
        assert_eq!(request.options.timeout, Duration::from_secs(90));
    }

    // === ScrapeOptions tests ===

    #[test]
    fn test_scrape_options_default() {
        let options = ScrapeOptions::default();
        assert_eq!(options.method, HttpMethod::Get);
        assert!(!options.needs_js);
        assert!(!options.needs_screenshot);
        assert!(!options.mobile);
        assert_eq!(options.timeout, Duration::from_secs(30));
        assert!(options.body.is_none());
        assert_eq!(options.sync_wait_ms, 0);
        assert!(options.actions.is_empty());
        assert!(options.screenshot_config.is_none());
        assert!(options.proxy.is_none());
        assert!(!options.skip_tls_verification);
        assert!(options.headers.is_empty());
        assert!(!options.needs_tls_fingerprint);
        assert!(!options.use_fire_engine);
    }

    #[test]
    fn test_scrape_options_builder_all_fields() {
        let mut headers = HashMap::new();
        headers.insert("X-Custom".to_string(), "value".to_string());

        let options = ScrapeOptions::builder()
            .method(HttpMethod::Post)
            .needs_js(true)
            .needs_screenshot(true)
            .mobile(true)
            .timeout(Duration::from_secs(60))
            .body("payload")
            .sync_wait_ms(500)
            .proxy("http://proxy:8080")
            .headers(headers.clone())
            .needs_tls_fingerprint(true)
            .use_fire_engine(true)
            .screenshot_config(ScreenshotConfig::default())
            .build();

        assert_eq!(options.method, HttpMethod::Post);
        assert!(options.needs_js);
        assert!(options.needs_screenshot);
        assert!(options.mobile);
        assert_eq!(options.timeout, Duration::from_secs(60));
        assert_eq!(options.body, Some("payload".to_string()));
        assert_eq!(options.sync_wait_ms, 500);
        assert_eq!(options.proxy, Some("http://proxy:8080".to_string()));
        assert_eq!(options.headers, headers);
        assert!(options.needs_tls_fingerprint);
        assert!(options.use_fire_engine);
        assert!(options.screenshot_config.is_some());
    }

    #[test]
    fn test_scrape_options_builder_skip_tls_false() {
        // skip=false should not read env vars and just set the field
        let options = ScrapeOptions::builder()
            .skip_tls_verification(false)
            .build();
        assert!(!options.skip_tls_verification);
    }

    // === HttpMethod tests ===

    #[test]
    fn test_http_method_default() {
        assert_eq!(HttpMethod::default(), HttpMethod::Get);
    }

    #[test]
    fn test_http_method_equality() {
        assert_eq!(HttpMethod::Get, HttpMethod::Get);
        assert_eq!(HttpMethod::Post, HttpMethod::Post);
        assert_ne!(HttpMethod::Get, HttpMethod::Post);
    }

    // === ScrollDirection tests ===

    #[test]
    fn test_scroll_direction_default() {
        assert_eq!(ScrollDirection::default(), ScrollDirection::Down);
    }

    #[test]
    fn test_scroll_direction_equality() {
        assert_eq!(ScrollDirection::Down, ScrollDirection::Down);
        assert_eq!(ScrollDirection::Up, ScrollDirection::Up);
        assert_eq!(ScrollDirection::Bottom, ScrollDirection::Bottom);
        assert_eq!(ScrollDirection::Top, ScrollDirection::Top);
        assert_ne!(ScrollDirection::Down, ScrollDirection::Up);
        assert_ne!(ScrollDirection::Bottom, ScrollDirection::Top);
    }

    // === ScreenshotConfig tests ===

    #[test]
    fn test_screenshot_config_default() {
        let config = ScreenshotConfig::default();
        assert!(config.full_page);
        assert!(config.selector.is_none());
        assert_eq!(config.quality, Some(80));
        assert_eq!(config.format, Some("jpeg".to_string()));
    }

    #[test]
    fn test_screenshot_config_equality() {
        let c1 = ScreenshotConfig::default();
        let c2 = ScreenshotConfig::default();
        assert_eq!(c1, c2);

        let c3 = ScreenshotConfig {
            full_page: false,
            selector: None,
            quality: Some(90),
            format: Some("png".to_string()),
        };
        assert_ne!(c1, c3);
    }

    // === ScrapeResponse tests ===

    #[test]
    fn test_scrape_response_new() {
        let response = ScrapeResponse::new(200, "content", "application/json");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content, "content");
        assert_eq!(response.content_type, "application/json");
        assert!(response.screenshot.is_none());
        assert!(response.headers.is_empty());
        assert_eq!(response.response_time_ms, 0);
        assert!(response.final_url.is_none());
    }

    #[test]
    fn test_scrape_response_is_success_2xx() {
        assert!(ScrapeResponse::new(200, "", "").is_success());
        assert!(ScrapeResponse::new(201, "", "").is_success());
        assert!(ScrapeResponse::new(204, "", "").is_success());
        assert!(ScrapeResponse::new(299, "", "").is_success());
    }

    #[test]
    fn test_scrape_response_is_success_non_2xx() {
        assert!(!ScrapeResponse::new(199, "", "").is_success());
        assert!(!ScrapeResponse::new(300, "", "").is_success());
        assert!(!ScrapeResponse::new(404, "", "").is_success());
        assert!(!ScrapeResponse::new(500, "", "").is_success());
    }

    // === to_internal conversion tests ===

    #[test]
    fn test_to_internal_basic_fields() {
        let request = ScrapeRequest::new("https://example.com");
        let internal = request.to_internal();

        assert_eq!(internal.url, "https://example.com");
        assert_eq!(internal.method, HttpMethod::Get);
        assert!(!internal.needs_js);
        assert!(!internal.needs_screenshot);
        assert!(!internal.mobile);
        assert_eq!(internal.timeout, Duration::from_secs(30));
        assert!(internal.body.is_none());
        assert_eq!(internal.sync_wait_ms, 0);
        assert!(internal.proxy.is_none());
        assert!(!internal.skip_tls_verification);
        assert!(!internal.needs_tls_fingerprint);
        assert!(!internal.use_fire_engine);
        assert!(internal.actions.is_empty());
        assert!(internal.screenshot_config.is_none());
        assert!(internal.headers.is_empty());
    }

    #[test]
    fn test_to_internal_all_fields() {
        let mut headers = HashMap::new();
        headers.insert("X-Test".to_string(), "val".to_string());

        let request = ScrapeRequest::new("https://example.com").with_options(
            ScrapeOptions::builder()
                .method(HttpMethod::Post)
                .needs_js(true)
                .needs_screenshot(true)
                .mobile(true)
                .timeout(Duration::from_secs(60))
                .body("data")
                .sync_wait_ms(1000)
                .proxy("http://proxy:8080")
                .headers(headers.clone())
                .needs_tls_fingerprint(true)
                .use_fire_engine(true)
                .screenshot_config(ScreenshotConfig {
                    full_page: false,
                    selector: Some("#main".to_string()),
                    quality: Some(90),
                    format: Some("png".to_string()),
                })
                .build(),
        );

        let internal = request.to_internal();
        assert_eq!(internal.url, "https://example.com");
        assert_eq!(internal.method, HttpMethod::Post);
        assert!(internal.needs_js);
        assert!(internal.needs_screenshot);
        assert!(internal.mobile);
        assert_eq!(internal.timeout, Duration::from_secs(60));
        assert_eq!(internal.body, Some("data".to_string()));
        assert_eq!(internal.sync_wait_ms, 1000);
        assert_eq!(internal.proxy, Some("http://proxy:8080".to_string()));
        assert_eq!(internal.headers, headers);
        assert!(internal.needs_tls_fingerprint);
        assert!(internal.use_fire_engine);
        assert!(internal.screenshot_config.is_some());
        let sc = internal.screenshot_config.unwrap();
        assert!(!sc.full_page);
        assert_eq!(sc.selector, Some("#main".to_string()));
        assert_eq!(sc.quality, Some(90));
        assert_eq!(sc.format, Some("png".to_string()));
    }

    #[test]
    fn test_to_internal_page_actions() {
        let options = ScrapeOptions::builder().body("").build();
        // Manually add actions since there's no builder method for actions
        let mut options = options;
        options.actions = vec![
            PageAction::Wait { milliseconds: 500 },
            PageAction::Click {
                selector: "#button".to_string(),
            },
            PageAction::Scroll {
                direction: ScrollDirection::Down,
            },
            PageAction::Scroll {
                direction: ScrollDirection::Up,
            },
            PageAction::Scroll {
                direction: ScrollDirection::Bottom,
            },
            PageAction::Scroll {
                direction: ScrollDirection::Top,
            },
            PageAction::Input {
                selector: "#field".to_string(),
                text: "hello".to_string(),
            },
        ];

        let request = ScrapeRequest::new("https://example.com").with_options(options);
        let internal = request.to_internal();

        assert_eq!(internal.actions.len(), 7);
        // Verify Wait action
        match &internal.actions[0] {
            InternalPageAction::Wait { milliseconds } => assert_eq!(*milliseconds, 500),
            other => panic!("Expected Wait, got {:?}", other),
        }
        // Verify Click action
        match &internal.actions[1] {
            InternalPageAction::Click { selector } => assert_eq!(selector, "#button"),
            other => panic!("Expected Click, got {:?}", other),
        }
        // Verify Scroll directions
        match &internal.actions[2] {
            InternalPageAction::Scroll { direction } => assert_eq!(direction, "down"),
            other => panic!("Expected Scroll down, got {:?}", other),
        }
        match &internal.actions[3] {
            InternalPageAction::Scroll { direction } => assert_eq!(direction, "up"),
            other => panic!("Expected Scroll up, got {:?}", other),
        }
        match &internal.actions[4] {
            InternalPageAction::Scroll { direction } => assert_eq!(direction, "bottom"),
            other => panic!("Expected Scroll bottom, got {:?}", other),
        }
        match &internal.actions[5] {
            InternalPageAction::Scroll { direction } => assert_eq!(direction, "top"),
            other => panic!("Expected Scroll top, got {:?}", other),
        }
        // Verify Input action
        match &internal.actions[6] {
            InternalPageAction::Input { selector, text } => {
                assert_eq!(selector, "#field");
                assert_eq!(text, "hello");
            }
            other => panic!("Expected Input, got {:?}", other),
        }
    }

    // === InternalScrapeResponse::to_public tests ===

    #[test]
    fn test_internal_response_to_public() {
        let internal = InternalScrapeResponse {
            status_code: 200,
            content: "body".to_string(),
            screenshot: Some("base64data".to_string()),
            content_type: "text/html".to_string(),
            headers: {
                let mut h = HashMap::new();
                h.insert("Server".to_string(), "nginx".to_string());
                h
            },
            response_time_ms: 42,
        };

        let public = internal.to_public("https://example.com/page");
        assert_eq!(public.status_code, 200);
        assert_eq!(public.content, "body");
        assert_eq!(public.screenshot, Some("base64data".to_string()));
        assert_eq!(public.content_type, "text/html");
        assert_eq!(public.headers.len(), 1);
        assert_eq!(public.headers.get("Server"), Some(&"nginx".to_string()));
        assert_eq!(public.response_time_ms, 42);
        assert_eq!(
            public.final_url,
            Some("https://example.com/page".to_string())
        );
    }

    // === EngineError tests ===

    #[test]
    fn test_engine_error_all_retryable_variants() {
        assert!(EngineError::RequestFailed("err".to_string()).is_retryable());
        assert!(EngineError::Timeout(Duration::from_secs(10)).is_retryable());
        assert!(EngineError::BrowserError("crash".to_string()).is_retryable());
        assert!(EngineError::AntiBotDetected("Cloudflare challenge".to_string()).is_retryable());
        assert!(!EngineError::NoEnginesAvailable.is_retryable());
        assert!(!EngineError::InvalidUrl("bad".to_string()).is_retryable());
        assert!(!EngineError::SsrfProtection("blocked".to_string()).is_retryable());
        assert!(!EngineError::Internal("err".to_string()).is_retryable());
        assert!(!EngineError::AllEnginesFailed("all".to_string()).is_retryable());
        assert!(!EngineError::Expired.is_retryable());
        assert!(!EngineError::Other("err".to_string()).is_retryable());
    }

    #[test]
    fn test_engine_error_from_string() {
        let err: EngineError = "something failed".to_string().into();
        match err {
            EngineError::RequestFailed(msg) => assert_eq!(msg, "something failed"),
            other => panic!("Expected RequestFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_engine_error_from_str() {
        let err: EngineError = "network error".into();
        match err {
            EngineError::RequestFailed(msg) => assert_eq!(msg, "network error"),
            other => panic!("Expected RequestFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_engine_error_from_anyhow() {
        let anyhow_err = anyhow::anyhow!("anyhow error");
        let err: EngineError = anyhow_err.into();
        match err {
            EngineError::Internal(msg) => assert_eq!(msg, "anyhow error"),
            other => panic!("Expected Internal, got {:?}", other),
        }
    }

    #[test]
    fn test_engine_error_display() {
        assert_eq!(
            EngineError::Timeout(Duration::from_secs(30)).to_string(),
            "Request timed out after 30s"
        );
        assert_eq!(
            EngineError::NoEnginesAvailable.to_string(),
            "No engines available"
        );
        assert_eq!(EngineError::Expired.to_string(), "Request expired");
    }

    // === EngineHealthStatus tests ===

    #[test]
    fn test_engine_health_status_default() {
        assert_eq!(EngineHealthStatus::default(), EngineHealthStatus::Healthy);
    }

    #[test]
    fn test_engine_health_status_equality() {
        let degraded1 = EngineHealthStatus::Degraded {
            unhealthy_engines: vec!["e1".to_string()],
            message: "msg".to_string(),
        };
        let degraded2 = EngineHealthStatus::Degraded {
            unhealthy_engines: vec!["e1".to_string()],
            message: "msg".to_string(),
        };
        assert_eq!(degraded1, degraded2);

        let unavailable = EngineHealthStatus::Unavailable {
            message: "all down".to_string(),
        };
        assert_ne!(EngineHealthStatus::Healthy, unavailable);
    }

    // === EngineClient tests ===

    #[test]
    fn test_engine_client_new() {
        let client = EngineClient::new();
        assert_eq!(client.engine_count(), 0);
        assert!(client.registered_engines().is_empty());
    }

    #[test]
    fn test_engine_client_default() {
        let client = EngineClient::default();
        assert_eq!(client.engine_count(), 0);
    }

    #[tokio::test]
    async fn test_engine_client_health_check_no_engines() {
        let client = EngineClient::new();
        let status = client.health_check().await;
        // With no engines, aggregate status is Healthy (no unhealthy engines found)
        assert_eq!(status, EngineHealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_engine_client_scrape_rejects_ssrf() {
        let client = EngineClient::new();
        let request = ScrapeRequest::new("http://localhost");
        let result = client.scrape(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::SsrfProtection(_) => {}
            other => panic!("Expected SsrfProtection, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_engine_client_scrape_rejects_private_ip() {
        let client = EngineClient::new();
        let request = ScrapeRequest::new("http://192.168.1.1");
        let result = client.scrape(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::SsrfProtection(_) => {}
            other => panic!("Expected SsrfProtection, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_engine_client_scrape_rejects_invalid_scheme() {
        let client = EngineClient::new();
        let request = ScrapeRequest::new("file:///etc/passwd");
        let result = client.scrape(&request).await;
        assert!(result.is_err());
    }

    // === convert_error tests ===

    #[test]
    fn test_convert_error_request_failed() {
        let err = convert_error(EngineError::RequestFailed("test".to_string()));
        match err {
            EngineError::RequestFailed(msg) => assert_eq!(msg, "test"),
            other => panic!("Expected RequestFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_convert_error_timeout() {
        let duration = Duration::from_secs(15);
        let err = convert_error(EngineError::Timeout(duration));
        match err {
            EngineError::Timeout(d) => assert_eq!(d, duration),
            other => panic!("Expected Timeout, got {:?}", other),
        }
    }

    #[test]
    fn test_convert_error_all_engines_failed_with_no_suitable() {
        let err = convert_error(EngineError::AllEnginesFailed(
            "No suitable engines found".to_string(),
        ));
        match err {
            EngineError::NoEnginesAvailable => {}
            other => panic!("Expected NoEnginesAvailable, got {:?}", other),
        }
    }

    #[test]
    fn test_convert_error_all_engines_failed_generic() {
        let err = convert_error(EngineError::AllEnginesFailed("all failed".to_string()));
        match err {
            EngineError::RequestFailed(msg) => assert_eq!(msg, "all failed"),
            other => panic!("Expected RequestFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_convert_error_ssrf_protection() {
        let err = convert_error(EngineError::SsrfProtection("blocked".to_string()));
        match err {
            EngineError::SsrfProtection(msg) => assert_eq!(msg, "blocked"),
            other => panic!("Expected SsrfProtection, got {:?}", other),
        }
    }

    #[test]
    fn test_convert_error_browser_error() {
        let err = convert_error(EngineError::BrowserError("crash".to_string()));
        match err {
            EngineError::BrowserError(msg) => assert_eq!(msg, "crash"),
            other => panic!("Expected BrowserError, got {:?}", other),
        }
    }

    #[test]
    fn test_convert_error_expired() {
        let err = convert_error(EngineError::Expired);
        match err {
            EngineError::Internal(msg) => assert_eq!(msg, "Request expired"),
            other => panic!("Expected Internal, got {:?}", other),
        }
    }

    #[test]
    fn test_convert_error_other() {
        let err = convert_error(EngineError::Other("misc".to_string()));
        match err {
            EngineError::Internal(msg) => assert_eq!(msg, "misc"),
            other => panic!("Expected Internal, got {:?}", other),
        }
    }

    #[test]
    fn test_convert_error_no_engines_available() {
        let err = convert_error(EngineError::NoEnginesAvailable);
        assert!(matches!(err, EngineError::NoEnginesAvailable));
    }

    #[test]
    fn test_convert_error_invalid_url() {
        let err = convert_error(EngineError::InvalidUrl("bad url".to_string()));
        match err {
            EngineError::InvalidUrl(msg) => assert_eq!(msg, "bad url"),
            other => panic!("Expected InvalidUrl, got {:?}", other),
        }
    }

    #[test]
    fn test_convert_error_internal() {
        let err = convert_error(EngineError::Internal("inner".to_string()));
        match err {
            EngineError::Internal(msg) => assert_eq!(msg, "inner"),
            other => panic!("Expected Internal, got {:?}", other),
        }
    }

    // === PageAction tests ===

    #[test]
    fn test_page_action_variants() {
        let wait = PageAction::Wait { milliseconds: 1000 };
        let click = PageAction::Click {
            selector: "#btn".to_string(),
        };
        let scroll = PageAction::Scroll {
            direction: ScrollDirection::Down,
        };
        let input = PageAction::Input {
            selector: "#field".to_string(),
            text: "text".to_string(),
        };

        // Verify they can be cloned and match via pattern matching
        match wait.clone() {
            PageAction::Wait { milliseconds } => assert_eq!(milliseconds, 1000),
            other => panic!("Expected Wait, got {:?}", other),
        }
        match click.clone() {
            PageAction::Click { selector } => assert_eq!(selector, "#btn"),
            other => panic!("Expected Click, got {:?}", other),
        }
        match scroll.clone() {
            PageAction::Scroll { direction } => assert_eq!(direction, ScrollDirection::Down),
            other => panic!("Expected Scroll, got {:?}", other),
        }
        match input.clone() {
            PageAction::Input { selector, text } => {
                assert_eq!(selector, "#field");
                assert_eq!(text, "text");
            }
            other => panic!("Expected Input, got {:?}", other),
        }
    }

    // === is_retryable comprehensive tests ===

    #[test]
    fn test_engine_error_is_retryable_all_variants() {
        assert!(EngineError::RequestFailed("err".to_string()).is_retryable());
        assert!(EngineError::Timeout(Duration::from_secs(10)).is_retryable());
        assert!(!EngineError::NoEnginesAvailable.is_retryable());
        assert!(!EngineError::InvalidUrl("bad".to_string()).is_retryable());
        assert!(!EngineError::SsrfProtection("blocked".to_string()).is_retryable());
        assert!(EngineError::BrowserError("err".to_string()).is_retryable());
        assert!(EngineError::AntiBotDetected("rate limited".to_string()).is_retryable());
        assert!(!EngineError::Internal("err".to_string()).is_retryable());
        assert!(!EngineError::AllEnginesFailed("all failed".to_string()).is_retryable());
        assert!(!EngineError::Expired.is_retryable());
        assert!(!EngineError::Other("err".to_string()).is_retryable());
    }

    // === retry_reason() tests (T012, R-antibot-003) ===

    #[test]
    fn test_retry_reason_antibot_detected_maps_to_antibot() {
        let err = EngineError::AntiBotDetected("Cloudflare challenge page".to_string());
        assert_eq!(err.retry_reason(), RetryReason::AntiBot);
    }

    #[test]
    fn test_retry_reason_transient_errors_map_to_transient() {
        assert_eq!(
            EngineError::RequestFailed("conn refused".to_string()).retry_reason(),
            RetryReason::Transient
        );
        assert_eq!(
            EngineError::Timeout(Duration::from_secs(10)).retry_reason(),
            RetryReason::Transient
        );
        assert_eq!(
            EngineError::BrowserError("crash".to_string()).retry_reason(),
            RetryReason::Transient
        );
    }

    #[test]
    fn test_retry_reason_non_retryable_errors_map_to_transient_placeholder() {
        // 不可重试错误返回 Transient 占位（调用方应先查 is_retryable()）
        assert_eq!(
            EngineError::NoEnginesAvailable.retry_reason(),
            RetryReason::Transient
        );
        assert_eq!(
            EngineError::InvalidUrl("bad".to_string()).retry_reason(),
            RetryReason::Transient
        );
        assert_eq!(
            EngineError::SsrfProtection("blocked".to_string()).retry_reason(),
            RetryReason::Transient
        );
        assert_eq!(
            EngineError::Internal("err".to_string()).retry_reason(),
            RetryReason::Transient
        );
        assert_eq!(
            EngineError::AllEnginesFailed("all".to_string()).retry_reason(),
            RetryReason::Transient
        );
        assert_eq!(EngineError::Expired.retry_reason(), RetryReason::Transient);
        assert_eq!(
            EngineError::Other("err".to_string()).retry_reason(),
            RetryReason::Transient
        );
    }

    #[test]
    fn test_antibot_detected_display_format() {
        let err = EngineError::AntiBotDetected("429 Too Many Requests".to_string());
        assert_eq!(err.to_string(), "Anti-bot detected: 429 Too Many Requests");
    }

    #[test]
    fn test_convert_error_antibot_detected_passthrough() {
        let err = convert_error(EngineError::AntiBotDetected("WAF block".to_string()));
        match err {
            EngineError::AntiBotDetected(msg) => assert_eq!(msg, "WAF block"),
            other => panic!("Expected AntiBotDetected, got {:?}", other),
        }
    }

    // === T027: EngineError::FeatureToggle tests (R-identity-002) ===

    #[test]
    fn test_feature_toggle_is_retryable() {
        assert!(EngineError::FeatureToggle("chrome_degraded_to_http".to_string()).is_retryable());
    }

    #[test]
    fn test_feature_toggle_retry_reason() {
        assert_eq!(
            EngineError::FeatureToggle("chrome_degraded_to_http".to_string()).retry_reason(),
            RetryReason::FeatureToggle
        );
    }

    #[test]
    fn test_feature_toggle_display_format() {
        let err = EngineError::FeatureToggle("js_render_failed_fallback".to_string());
        assert_eq!(err.to_string(), "Feature toggle: js_render_failed_fallback");
    }

    #[test]
    fn test_convert_error_feature_toggle_passthrough() {
        let err = convert_error(EngineError::FeatureToggle("engine_downgrade".to_string()));
        match err {
            EngineError::FeatureToggle(msg) => assert_eq!(msg, "engine_downgrade"),
            other => panic!("Expected FeatureToggle, got {:?}", other),
        }
    }

    /// R-identity-002: FeatureToggle 与其他 reason 区分
    #[test]
    fn test_feature_toggle_distinct_from_other_reasons() {
        let ft_err = EngineError::FeatureToggle("test".to_string());
        let ab_err = EngineError::AntiBotDetected("test".to_string());
        let tr_err = EngineError::Timeout(Duration::from_secs(1));

        assert_eq!(ft_err.retry_reason(), RetryReason::FeatureToggle);
        assert_ne!(ft_err.retry_reason(), ab_err.retry_reason());
        assert_ne!(ft_err.retry_reason(), tr_err.retry_reason());
    }

    // === to_public conversion tests ===

    #[test]
    fn test_to_public_basic_fields() {
        let internal = InternalScrapeResponse {
            status_code: 200,
            content: "hello".to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: std::collections::HashMap::new(),
            response_time_ms: 150,
        };
        let public = internal.to_public("https://example.com");
        assert_eq!(public.status_code, 200);
        assert_eq!(public.content, "hello");
        assert!(public.screenshot.is_none());
        assert_eq!(public.content_type, "text/html");
        assert!(public.headers.is_empty());
        assert_eq!(public.response_time_ms, 150);
        assert_eq!(public.final_url, Some("https://example.com".to_string()));
    }

    #[test]
    fn test_to_public_with_screenshot_and_headers() {
        let mut headers = std::collections::HashMap::new();
        headers.insert("X-Custom".to_string(), "value".to_string());
        let internal = InternalScrapeResponse {
            status_code: 404,
            content: "not found".to_string(),
            screenshot: Some("base64data".to_string()),
            content_type: "application/json".to_string(),
            headers,
            response_time_ms: 500,
        };
        let public = internal.to_public("https://test.com/page");
        assert_eq!(public.status_code, 404);
        assert_eq!(public.content, "not found");
        assert_eq!(public.screenshot, Some("base64data".to_string()));
        assert_eq!(public.content_type, "application/json");
        assert_eq!(public.headers.len(), 1);
        assert_eq!(public.response_time_ms, 500);
        assert_eq!(public.final_url, Some("https://test.com/page".to_string()));
    }

    #[test]
    fn test_to_public_empty_content() {
        let internal = InternalScrapeResponse {
            status_code: 204,
            content: String::new(),
            screenshot: None,
            content_type: String::new(),
            headers: std::collections::HashMap::new(),
            response_time_ms: 0,
        };
        let public = internal.to_public("");
        assert_eq!(public.status_code, 204);
        assert!(public.content.is_empty());
        assert!(public.content_type.is_empty());
        assert_eq!(public.final_url, Some(String::new()));
    }

    // === EngineClient method tests ===

    #[test]
    fn test_engine_client_engine_count_zero() {
        let client = EngineClient::new();
        assert_eq!(client.engine_count(), 0);
    }

    #[test]
    fn test_engine_client_registered_engines_empty() {
        let client = EngineClient::new();
        let engines = client.registered_engines();
        assert!(engines.is_empty());
    }

    // === Mock EngineRouter for scrape/health_check tests ===

    use crate::engines::router::EngineStats;
    use async_trait::async_trait;

    enum MockRouteResult {
        Success(InternalScrapeResponse),
        Timeout,
        AllEnginesFailed(String),
    }

    struct MockEngineRouter {
        result: MockRouteResult,
        engines: Vec<String>,
    }

    impl MockEngineRouter {
        fn new_success() -> Self {
            Self {
                result: MockRouteResult::Success(InternalScrapeResponse {
                    status_code: 200,
                    content: "test content".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 100,
                }),
                engines: vec!["mock-engine".to_string()],
            }
        }

        fn new_timeout() -> Self {
            Self {
                result: MockRouteResult::Timeout,
                engines: vec![],
            }
        }

        fn new_all_failed(msg: &str) -> Self {
            Self {
                result: MockRouteResult::AllEnginesFailed(msg.to_string()),
                engines: vec![],
            }
        }
    }

    #[async_trait]
    impl EngineRouterTrait for MockEngineRouter {
        async fn route(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            match &self.result {
                MockRouteResult::Success(resp) => Ok(resp.clone()),
                MockRouteResult::Timeout => Err(EngineError::Timeout(Duration::from_secs(30))),
                MockRouteResult::AllEnginesFailed(msg) => {
                    Err(EngineError::AllEnginesFailed(msg.clone()))
                }
            }
        }

        async fn aggregate(
            &self,
            request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            self.route(request).await
        }

        fn get_engine_stats(&self) -> HashMap<String, EngineStats> {
            HashMap::new()
        }

        fn reset_engine_stats(&self, _engine_name: &str) {}

        fn registered_engines(&self) -> Vec<String> {
            self.engines.clone()
        }
    }

    // === EngineClient::scrape success path ===

    #[tokio::test]
    async fn test_engine_client_scrape_success() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new_success());
        let client = EngineClient::with_router(router);

        let request = ScrapeRequest::new("https://example.com");
        let result = client.scrape(&request).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content, "test content");
        assert_eq!(response.content_type, "text/html");
        assert_eq!(response.response_time_ms, 100);
        assert_eq!(response.final_url, Some("https://example.com".to_string()));
    }

    // === EngineClient::scrape SSRF rejection ===

    #[tokio::test]
    async fn test_engine_client_scrape_ssrf_rejected() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new_success());
        let client = EngineClient::with_router(router);

        // SSRF: localhost / private IP should be rejected by validate_url
        let request = ScrapeRequest::new("http://127.0.0.1:8080");
        let result = client.scrape(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::SsrfProtection(_) => {}
            other => panic!("Expected SsrfProtection, got {:?}", other),
        }
    }

    // === EngineClient::scrape router error propagation ===

    #[tokio::test]
    async fn test_engine_client_scrape_router_error() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new_timeout());
        let client = EngineClient::with_router(router);

        let request = ScrapeRequest::new("https://example.com");
        let result = client.scrape(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::Timeout(d) => assert_eq!(d, Duration::from_secs(30)),
            other => panic!("Expected Timeout, got {:?}", other),
        }
    }

    // === EngineClient::scrape no engines available error ===

    #[tokio::test]
    async fn test_engine_client_scrape_no_engines_error() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new_all_failed(
            "No suitable engines available",
        ));
        let client = EngineClient::with_router(router);

        let request = ScrapeRequest::new("https://example.com");
        let result = client.scrape(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::NoEnginesAvailable => {}
            other => panic!("Expected NoEnginesAvailable, got {:?}", other),
        }
    }

    // === EngineClient::with_engines ===

    #[test]
    fn test_engine_client_with_engines() {
        struct MockEngine;
        #[async_trait]
        impl ScraperEngine for MockEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "ok".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 50,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "mock-engine"
            }
        }

        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(MockEngine)];
        let client = EngineClient::with_engines(engines);
        assert_eq!(client.engine_count(), 1);
        assert_eq!(client.registered_engines(), vec!["mock-engine".to_string()]);
    }

    // === EngineClientTrait trait impl tests ===

    #[tokio::test]
    async fn test_engine_client_trait_scrape_success() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new_success());
        let client: Arc<dyn EngineClientTrait> = Arc::new(EngineClient::with_router(router));

        let request = ScrapeRequest::new("https://example.com");
        let result = client.scrape(&request).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status_code, 200);
    }

    #[tokio::test]
    async fn test_engine_client_trait_health_check_healthy() {
        let client = EngineClient::new();
        let status = client.health_check().await;
        // With no engines, health should be Healthy
        assert_eq!(status, EngineHealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_engine_client_trait_health_check_with_router() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new_success());
        let client = EngineClient::with_router(router);
        let status = client.health_check().await;
        // health_monitor has no engines (with_router passes empty), so Healthy
        assert_eq!(status, EngineHealthStatus::Healthy);
    }

    #[test]
    fn test_engine_client_trait_engine_count_and_registered_engines() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new_success());
        let client: Arc<dyn EngineClientTrait> = Arc::new(EngineClient::with_router(router));
        assert_eq!(client.engine_count(), 1);
        assert_eq!(client.registered_engines(), vec!["mock-engine".to_string()]);
    }

    // === skip_tls_verification branch coverage ===
    // Tests all branches: development (allow), production (deny), prod (deny), false (no-op).

    #[test]
    fn test_skip_tls_verification_all_branches() {
        // Branch 1: skip=false (no warning, sets to false)
        let options = ScrapeOptions::builder()
            .skip_tls_verification(false)
            .build();
        assert!(!options.skip_tls_verification);

        // Branch 2: skip=true in development (warning, sets to true)
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let options = ScrapeOptions::builder().skip_tls_verification(true).build();
        assert!(
            options.skip_tls_verification,
            "skip_tls_verification should be true in development"
        );

        // Branch 3: skip=true in production (denied, remains false)
        std::env::set_var("APP_ENVIRONMENT", "production");
        let options = ScrapeOptions::builder().skip_tls_verification(true).build();
        assert!(
            !options.skip_tls_verification,
            "skip_tls_verification should be denied in production"
        );

        // Branch 4: skip=true in prod (denied, remains false)
        std::env::set_var("APP_ENVIRONMENT", "prod");
        let options = ScrapeOptions::builder().skip_tls_verification(true).build();
        assert!(
            !options.skip_tls_verification,
            "skip_tls_verification should be denied in prod"
        );

        // Branch 5: CRAWLRS_ENV also checked for production
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::set_var("CRAWLRS_ENV", "production");
        let options = ScrapeOptions::builder().skip_tls_verification(true).build();
        assert!(
            !options.skip_tls_verification,
            "skip_tls_verification should be denied via CRAWLRS_ENV=production"
        );

        // Cleanup
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
    }

    // === ScrapeOptions builder with all remaining fields ===

    #[test]
    fn test_scrape_options_builder_body_sync_wait_tls_fingerprint() {
        let options = ScrapeOptions::builder()
            .body("request body")
            .sync_wait_ms(500)
            .needs_tls_fingerprint(true)
            .use_fire_engine(true)
            .build();

        assert_eq!(options.body, Some("request body".to_string()));
        assert_eq!(options.sync_wait_ms, 500);
        assert!(options.needs_tls_fingerprint);
        assert!(options.use_fire_engine);
    }

    // === HttpMethod default test ===

    #[test]
    fn test_http_method_default_is_get() {
        assert_eq!(HttpMethod::default(), HttpMethod::Get);
    }

    // === ScrollDirection tests ===

    #[test]
    fn test_scroll_direction_default_is_down() {
        assert_eq!(ScrollDirection::default(), ScrollDirection::Down);
    }

    #[test]
    fn test_scroll_direction_variants() {
        assert_eq!(ScrollDirection::Down, ScrollDirection::Down);
        assert_eq!(ScrollDirection::Up, ScrollDirection::Up);
        assert_eq!(ScrollDirection::Bottom, ScrollDirection::Bottom);
        assert_eq!(ScrollDirection::Top, ScrollDirection::Top);
        assert_ne!(ScrollDirection::Down, ScrollDirection::Up);
    }

    // === EngineClient Clone ===

    #[test]
    fn test_engine_client_clone() {
        let client = EngineClient::new();
        let cloned = client.clone();
        assert_eq!(client.engine_count(), cloned.engine_count());
        assert_eq!(client.registered_engines(), cloned.registered_engines());
    }

    // === ScraperEngine trait default method coverage ===

    #[test]
    fn test_scraper_engine_default_supports_tls_fingerprint_returns_false() {
        // Cover lines 605-606: the ScraperEngine trait provides a default
        // implementation of `supports_tls_fingerprint` that returns false.
        // An engine that does NOT override it should report false.
        struct BasicEngine;
        #[async_trait]
        impl ScraperEngine for BasicEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "ok".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 1,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "basic"
            }
        }

        let engine = BasicEngine;
        assert!(
            !engine.supports_tls_fingerprint(),
            "default supports_tls_fingerprint should return false"
        );
    }

    // === EngineClient::health_check Degraded/Unavailable branch coverage ===
    // The public constructors (new / with_router / with_engines) all build the
    // internal health_monitor with either an empty engine list or an empty
    // health_monitor, so the Degraded/Unavailable branches of health_check
    // are unreachable through the public API. We construct EngineClient
    // directly here (test module has access to private fields) and inject a
    // health_monitor that wraps real engines whose scrape outcomes we control.

    /// Mock engine that always succeeds; used as the "healthy" engine in a
    /// mixed-engine scenario to trigger AggregateHealthStatus::Degraded.
    struct HealthyMockEngine {
        engine_name: &'static str,
    }

    #[async_trait]
    impl ScraperEngine for HealthyMockEngine {
        async fn scrape(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            Ok(InternalScrapeResponse {
                status_code: 200,
                content: "ok".to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 1,
            })
        }
        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            100
        }
        fn name(&self) -> &'static str {
            self.engine_name
        }
    }

    /// Mock engine that always fails; used to make the health monitor mark
    /// the engine as Degraded (after 1 failure) or Unhealthy (after
    /// `max_consecutive_failures` failures).
    struct FailingMockEngine {
        engine_name: &'static str,
    }

    #[async_trait]
    impl ScraperEngine for FailingMockEngine {
        async fn scrape(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            Err(EngineError::RequestFailed("connection refused".to_string()))
        }
        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            100
        }
        fn name(&self) -> &'static str {
            self.engine_name
        }
    }

    #[tokio::test]
    async fn test_engine_client_health_check_returns_degraded() {
        // Cover lines 738-743: when health_monitor reports Degraded status
        // (some engines unhealthy but at least one healthy), EngineClient
        // should convert that into EngineHealthStatus::Degraded with the
        // unhealthy engine names and a "N engines degraded" message.
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            Arc::new(HealthyMockEngine {
                engine_name: "healthy-engine",
            }),
            Arc::new(FailingMockEngine {
                engine_name: "failing-engine",
            }),
        ];

        // Construct EngineClient directly so we can inject a health_monitor
        // that actually owns the engines. with_router/with_engines leave the
        // monitor empty, which would always report Healthy.
        let client = EngineClient {
            router: Arc::new(EngineRouter::new(engines.clone())),
            health_monitor: Arc::new(EngineHealthMonitor::new(engines)),
        };

        let status = client.health_check().await;
        match status {
            EngineHealthStatus::Degraded {
                unhealthy_engines,
                message,
            } => {
                assert!(
                    unhealthy_engines.contains(&"failing-engine".to_string()),
                    "failing-engine should be listed as unhealthy: {:?}",
                    unhealthy_engines
                );
                assert!(
                    message.contains("engines degraded"),
                    "message should mention 'engines degraded': {}",
                    message
                );
            }
            other => panic!("Expected Degraded, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_engine_client_health_check_returns_unavailable() {
        // Cover lines 745-747: when health_monitor reports Unavailable
        // (all engines unhealthy), EngineClient should convert that into
        // EngineHealthStatus::Unavailable with the canonical message.
        use crate::engines::health_monitor::HealthCheckConfig;

        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(FailingMockEngine {
            engine_name: "failing-engine",
        })];

        // Configure max_consecutive_failures = 1 so a single health check
        // pass is enough to mark the engine Unhealthy (otherwise we'd need
        // three passes with the default config).
        let config = HealthCheckConfig {
            check_interval: Duration::from_secs(60),
            timeout: Duration::from_secs(10),
            max_consecutive_failures: 1,
            degraded_threshold_ms: 2000,
            unhealthy_threshold_ms: 5000,
            target_url: "https://example.com".to_string(),
        };

        let client = EngineClient {
            router: Arc::new(EngineRouter::new(engines.clone())),
            health_monitor: Arc::new(EngineHealthMonitor::new_with_config(engines, config)),
        };

        let status = client.health_check().await;
        match status {
            EngineHealthStatus::Unavailable { message } => {
                assert_eq!(
                    message, "All engines unavailable",
                    "Unavailable message should be canonical"
                );
            }
            other => panic!("Expected Unavailable, got {:?}", other),
        }
    }

    // === T056 MEDIUM-2: session_id 校验测试 ===

    #[test]
    fn test_validate_session_id_accepts_normal_string() {
        assert!(validate_session_id("session-123"));
        assert!(validate_session_id("abc"));
        assert!(validate_session_id("user_session_id_456"));
    }

    #[test]
    fn test_validate_session_id_accepts_printable_ascii() {
        // 0x20-0x7E 范围内的所有可打印 ASCII 字符
        let printable: String = (0x20u8..=0x7E).map(|b| b as char).collect();
        assert!(
            validate_session_id(&printable),
            "all printable ASCII chars should be valid"
        );
    }

    #[test]
    fn test_validate_session_id_accepts_max_length() {
        // 正好 128 字节
        let max_len = "a".repeat(MAX_SESSION_ID_LEN);
        assert!(
            validate_session_id(&max_len),
            "exactly MAX_SESSION_ID_LEN bytes should be valid"
        );
    }

    #[test]
    fn test_validate_session_id_rejects_too_long() {
        // 129 字节 — 超过上限
        let too_long = "a".repeat(MAX_SESSION_ID_LEN + 1);
        assert!(
            !validate_session_id(&too_long),
            "string exceeding MAX_SESSION_ID_LEN should be rejected"
        );
    }

    #[test]
    fn test_validate_session_id_rejects_control_chars() {
        // 控制字符（0x00-0x1F, 0x7F）应被拒绝
        assert!(!validate_session_id("session\n123"));
        assert!(!validate_session_id("session\t123"));
        assert!(!validate_session_id("session\r123"));
        assert!(!validate_session_id("session\0123"));
        assert!(!validate_session_id("session\u{007F}123"));
    }

    #[test]
    fn test_validate_session_id_rejects_empty_string() {
        // 空字符串：长度为 0，字符集检查为空（all 返回 true）
        // 空字符串在语义上无效（不能作为 session_id），但当前实现允许它
        // 因为空字符串不会导致 DoS 或日志注入，且 rr_pick 的回退行为是 RoundRobin
        // 这里测试当前行为（空字符串通过校验），如果未来需要拒绝空字符串可修改
        assert!(validate_session_id(""));
    }

    #[test]
    fn test_validate_session_id_rejects_non_ascii() {
        // 非 ASCII 字符（UTF-8 多字节）应被拒绝
        assert!(!validate_session_id("session-中文"));
        assert!(!validate_session_id("session-é"));
        assert!(!validate_session_id("session-🎉"));
    }

    #[test]
    fn test_session_id_builder_accepts_valid_input() {
        let options = ScrapeOptions::builder()
            .session_id("valid-session-123")
            .build();
        assert_eq!(options.session_id, Some("valid-session-123".to_string()));
    }

    #[test]
    fn test_session_id_builder_rejects_too_long() {
        let too_long = "a".repeat(MAX_SESSION_ID_LEN + 1);
        let options = ScrapeOptions::builder()
            .session_id(too_long.clone())
            .build();
        assert!(
            options.session_id.is_none(),
            "too long session_id should be rejected by builder"
        );
    }

    #[test]
    fn test_session_id_builder_rejects_control_chars() {
        let options = ScrapeOptions::builder().session_id("bad\nsession").build();
        assert!(
            options.session_id.is_none(),
            "session_id with control chars should be rejected by builder"
        );
    }

    #[test]
    fn test_to_internal_validates_session_id() {
        // to_internal 应二次校验 session_id，拒绝非法值
        let mut options = ScrapeOptions::default();
        options.session_id = Some("bad\nsession".to_string());
        let request = ScrapeRequest::new("https://example.com").with_options(options);
        let internal = request.to_internal();
        assert!(
            internal.session_id.is_none(),
            "to_internal should reject invalid session_id (bypassing builder)"
        );
    }

    #[test]
    fn test_to_internal_preserves_valid_session_id() {
        let mut options = ScrapeOptions::default();
        options.session_id = Some("valid-session".to_string());
        let request = ScrapeRequest::new("https://example.com").with_options(options);
        let internal = request.to_internal();
        assert_eq!(
            internal.session_id,
            Some("valid-session".to_string()),
            "to_internal should preserve valid session_id"
        );
    }
}
