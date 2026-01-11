// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

//! EngineClient - Unified public API for scraping operations
//!
//! This module provides the single entry point for all scraping operations.
//! All internal implementation details (UA rotation, circuit breaker, engine selection)
//! are encapsulated within EngineClient and not exposed to callers.

use crate::engines::health_monitor::{AggregateHealthStatus, EngineHealthMonitor};
use crate::engines::router::EngineRouter;
use crate::engines::traits::ScrapeRequest as InternalScrapeRequest;
use crate::engines::traits::ScrapeResponse as InternalScrapeResponse;
use crate::engines::validators::validate_url;
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
    /// Whether JavaScript rendering is required (default: false)
    pub needs_js: bool,
    /// Whether screenshot capture is required (default: false)
    pub needs_screenshot: bool,
    /// Whether to use mobile user agent (default: false)
    pub mobile: bool,
    /// Request timeout duration (default: 30 seconds)
    pub timeout: Duration,
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
}

impl Default for ScrapeOptions {
    fn default() -> Self {
        Self {
            needs_js: false,
            needs_screenshot: false,
            mobile: false,
            timeout: Duration::from_secs(30),
            sync_wait_ms: 0,
            actions: Vec::new(),
            screenshot_config: None,
            proxy: None,
            skip_tls_verification: false,
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

    pub fn sync_wait_ms(mut self, ms: u32) -> Self {
        self.0.sync_wait_ms = ms;
        self
    }

    pub fn proxy(mut self, proxy: impl Into<String>) -> Self {
        self.0.proxy = Some(proxy.into());
        self
    }

    pub fn skip_tls_verification(mut self, skip: bool) -> Self {
        self.0.skip_tls_verification = skip;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Down,
    Up,
    Bottom,
    Top,
}

impl Default for ScrollDirection {
    fn default() -> Self {
        ScrollDirection::Down
    }
}

/// Screenshot configuration.
#[derive(Debug, Clone)]
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

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
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
        }
    }
}

/// Health status of the engine system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineHealthStatus {
    /// All engines are operational
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

impl Default for EngineHealthStatus {
    fn default() -> Self {
        Self::Healthy
    }
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
    router: Arc<EngineRouter>,
    /// Internal health monitor for tracking engine health
    health_monitor: Arc<EngineHealthMonitor>,
}

impl EngineClient {
    /// Create a new EngineClient with default configuration.
    pub fn new() -> Self {
        Self::with_router(Arc::new(EngineRouter::new(Vec::new())))
    }

    /// Create an EngineClient with a custom router.
    pub fn with_router(router: Arc<EngineRouter>) -> Self {
        Self {
            router,
            health_monitor: Arc::new(EngineHealthMonitor::new(Vec::new())),
        }
    }

    /// Create an EngineClient with engines pre-registered.
    pub fn with_engines(engines: Vec<Arc<dyn crate::engines::traits::ScraperEngine>>) -> Self {
        let router = Arc::new(EngineRouter::new(engines));
        let engines_for_health = router.get_engines().clone();
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
        let internal_request = InternalScrapeRequest::from_public(request);

        // Route to appropriate engine
        match self.router.route(&internal_request).await {
            Ok(response) => Ok(InternalScrapeResponse::from_internal(
                response,
                &request.url,
            )),
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

/// Convert internal errors to public EngineError
fn convert_error(e: crate::engines::traits::EngineError) -> EngineError {
    match e {
        crate::engines::traits::EngineError::RequestFailed(msg) => EngineError::RequestFailed(msg),
        crate::engines::traits::EngineError::Timeout(_) => {
            EngineError::Timeout(Duration::from_secs(30))
        }
        crate::engines::traits::EngineError::AllEnginesFailed(msg) => {
            if msg.contains("No suitable engines") {
                EngineError::NoEnginesAvailable
            } else {
                EngineError::RequestFailed(msg)
            }
        }
        crate::engines::traits::EngineError::SsrfProtection(msg) => {
            EngineError::SsrfProtection(msg)
        }
        crate::engines::traits::EngineError::BrowserError(msg) => EngineError::BrowserError(msg),
        crate::engines::traits::EngineError::Expired => {
            EngineError::Internal("Request expired".to_string())
        }
        crate::engines::traits::EngineError::Other(msg) => EngineError::Internal(msg),
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
