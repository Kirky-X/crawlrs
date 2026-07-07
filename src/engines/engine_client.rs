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
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use log::warn;

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
}
