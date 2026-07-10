// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! EngineClient - Unified public API for scraping operations
//!
//! This module provides the single entry point for all scraping operations.
//! All internal implementation details (UA rotation, circuit breaker, engine selection)
//! are encapsulated within EngineClient and not exposed to callers.

#![allow(deprecated)]

use crate::engines::health_monitor::{AggregateHealthStatus, EngineHealthMonitor};
use crate::engines::router::{EngineRouter, EngineRouterTrait};
use crate::engines::validators::validate_url;
use log::warn;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

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

    pub fn build(self) -> ScrapeOptions {
        self.0
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
#[derive(Debug, Clone)]
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
#[allow(dead_code)]
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
}

/// Internal screenshot configuration
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct InternalScreenshotConfig {
    pub full_page: bool,
    pub selector: Option<String>,
    pub quality: Option<u8>,
    pub format: Option<String>,
}

/// Internal page action for engine operations
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum InternalPageAction {
    Wait { milliseconds: u64 },
    Click { selector: String },
    Scroll { direction: String },
    Input { selector: String, text: String },
    Screenshot { full_page: bool },
}

/// Internal response type for engine operations
#[derive(Debug, Clone)]
#[allow(dead_code)]
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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
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
        }
    }
}

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

    /// Request expired (circuit breaker open)
    #[error("Request expired")]
    Expired,

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
            Self::Internal(_) => false,
            Self::AllEnginesFailed(_) => false,
            Self::Expired => false,
            Self::Other(_) => false,
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
pub trait EngineClientTrait: shaku::Interface + Send + Sync {
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
        EngineError::Expired => EngineError::Internal("Request expired".to_string()),
        EngineError::Other(msg) => EngineError::Internal(msg),
        EngineError::NoEnginesAvailable => EngineError::NoEnginesAvailable,
        EngineError::InvalidUrl(msg) => EngineError::InvalidUrl(msg),
        EngineError::Internal(msg) => EngineError::Internal(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
