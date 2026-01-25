// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! FlareSolverr Engine - Uses FlareSolverr API to bypass Cloudflare and other anti-bot protections
//!
//! FlareSolverr is a proxy server that uses Selenium with undetected-chromedriver
//! to bypass Cloudflare protection and other anti-bot measures.
//!
//! This engine is particularly useful for:
//! - Google search (bypasses CAPTCHA)
//! - Cloudflare-protected sites
//! - Sites with strong anti-bot measures

use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// FlareSolverr configuration
#[derive(Debug, Clone)]
pub struct FlareSolverrConfig {
    /// FlareSolverr server URL
    pub url: String,
    /// Request timeout in seconds
    pub timeout_seconds: u64,
    /// Default session ID (optional)
    pub session_id: Option<String>,
}

impl Default for FlareSolverrConfig {
    fn default() -> Self {
        let url = std::env::var("FLARESOLVERR_URL")
            .unwrap_or_else(|_| "http://localhost:8191".to_string());

        // Validate URL format and protocol
        let validated_url = validate_flaresolverr_url(&url)
            .expect("Invalid FLARESOLVERR_URL: must be http:// or https:// with valid format");

        Self {
            url: validated_url,
            timeout_seconds: 60,
            session_id: None,
        }
    }
}

/// Validate FlareSolverr URL - only allow http/https protocols
fn validate_flaresolverr_url(url: &str) -> Result<String, String> {
    let parsed = url::Url::parse(url).map_err(|_| "Invalid URL format".to_string())?;

    // Only allow http and https protocols
    match parsed.scheme() {
        "http" | "https" => Ok(url.to_string()),
        _ => Err(format!(
            "Invalid protocol '{}': FLARESOLVERR_URL must use http or https",
            parsed.scheme()
        )),
    }
}

/// FlareSolverr HTTP client
#[derive(Debug, Clone)]
pub struct FlareSolverrEngine {
    /// HTTP client for FlareSolverr API
    client: Arc<Client>,
    /// FlareSolverr configuration
    config: FlareSolverrConfig,
    /// Session ID for persistent sessions
    session_id: Option<String>,
}

impl FlareSolverrEngine {
    /// Create a new FlareSolverrEngine with default configuration
    pub fn new(client: Arc<Client>) -> Self {
        Self::with_config(client, FlareSolverrConfig::default())
    }

    /// Create a new FlareSolverrEngine from configuration URL
    pub fn with_url(client: Arc<Client>, url: impl Into<String>) -> Self {
        let config = FlareSolverrConfig {
            url: url.into(),
            timeout_seconds: 60,
            session_id: None,
        };
        Self::with_config(client, config)
    }

    /// Create a new FlareSolverrEngine with custom configuration
    pub fn with_config(client: Arc<Client>, config: FlareSolverrConfig) -> Self {
        let session_id = config.session_id.clone();

        Self {
            client,
            config,
            session_id,
        }
    }

    /// Create a builder for FlareSolverrEngine
    pub fn builder() -> FlareSolverrEngineBuilder {
        FlareSolverrEngineBuilder::default()
    }

    /// Get the current session ID
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Get the API URL for FlareSolverr
    /// Handles URLs with or without trailing /v1 to avoid duplication
    fn api_url(&self) -> String {
        let base_url = self.config.url.trim_end_matches('/');
        if base_url.ends_with("/v1") {
            base_url.to_string()
        } else {
            format!("{}/v1", base_url)
        }
    }

    /// Create a new session
    pub async fn create_session(&mut self) -> Result<String, EngineError> {
        #[derive(Serialize)]
        struct CreateSessionRequest {
            cmd: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            session: Option<String>,
        }

        let request = CreateSessionRequest {
            cmd: "sessions.create".to_string(),
            session: None,
        };

        let response: SessionResponse = self
            .client
            .post(self.api_url())
            .json(&request)
            .send()
            .await
            .map_err(|e| EngineError::Other(format!("Failed to create session: {}", e)))?
            .json()
            .await
            .map_err(|e| EngineError::Other(format!("Failed to parse session response: {}", e)))?;

        if response.status == "ok" {
            if let Some(session_id) = response.session {
                self.session_id = Some(session_id.clone());
                info!("Created FlareSolverr session: {}", session_id);
                Ok(session_id)
            } else {
                Err(EngineError::Other("No session ID in response".to_string()))
            }
        } else {
            Err(EngineError::Other(format!(
                "Failed to create session: {}",
                response.message
            )))
        }
    }

    /// Destroy a session
    pub async fn destroy_session(&mut self, session_id: &str) -> Result<(), EngineError> {
        #[derive(Serialize)]
        struct DestroySessionRequest {
            cmd: String,
            session: String,
        }

        let request = DestroySessionRequest {
            cmd: "sessions.destroy".to_string(),
            session: session_id.to_string(),
        };

        let response: GenericResponse = self
            .client
            .post(self.api_url())
            .json(&request)
            .send()
            .await
            .map_err(|e| EngineError::Other(format!("Failed to destroy session: {}", e)))?
            .json()
            .await
            .map_err(|e| EngineError::Other(format!("Failed to parse response: {}", e)))?;

        if response.status == "ok" {
            self.session_id = None;
            debug!("Destroyed FlareSolverr session: {}", session_id);
            Ok(())
        } else {
            Err(EngineError::Other(format!(
                "Failed to destroy session: {}",
                response.message
            )))
        }
    }
}

/// Builder for FlareSolverrEngine
#[derive(Debug, Default)]
pub struct FlareSolverrEngineBuilder {
    config: FlareSolverrConfig,
}

impl FlareSolverrEngineBuilder {
    /// Set FlareSolverr URL
    pub fn with_url(mut self, url: &str) -> Self {
        self.config.url = url.to_string();
        self
    }

    /// Set request timeout
    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.config.timeout_seconds = seconds;
        self
    }

    /// Set default session ID
    pub fn with_session(mut self, session_id: &str) -> Self {
        self.config.session_id = Some(session_id.to_string());
        self
    }

    /// Build the FlareSolverrEngine
    pub fn build(self, client: Arc<Client>) -> FlareSolverrEngine {
        FlareSolverrEngine::with_config(client, self.config)
    }
}

/// Response from FlareSolverr sessions.create
#[derive(Serialize, Deserialize, Debug)]
struct SessionResponse {
    status: String,
    message: String,
    session: Option<String>,
    #[serde(flatten)]
    _extra: HashMap<String, serde_json::Value>,
}

/// Generic response from FlareSolverr
#[derive(Serialize, Deserialize, Debug)]
struct GenericResponse {
    status: String,
    message: String,
    #[serde(flatten)]
    _extra: HashMap<String, serde_json::Value>,
}

/// Request to FlareSolverr
#[derive(Serialize, Debug)]
struct FlareSolverrRequest {
    cmd: String,
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    return_screenshot: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wait_in_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disable_media: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cookies: Option<Vec<Cookie>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    post_data: Option<String>,
}

/// Cookie for FlareSolverr
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Cookie {
    name: String,
    value: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    path: Option<String>,
}

/// Response from FlareSolverr
#[derive(Serialize, Deserialize, Debug)]
struct FlareSolverrResponse {
    status: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    solution: Option<FlareSolverrSolution>,
    #[serde(flatten)]
    _extra: HashMap<String, serde_json::Value>,
}

/// Solution from FlareSolverr
#[derive(Serialize, Deserialize, Debug)]
struct FlareSolverrSolution {
    url: String,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    headers: Option<HashMap<String, String>>,
    response: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cookies: Option<Vec<Cookie>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    screenshot: Option<String>,
    #[serde(default)]
    start_timestamp: i64,
    #[serde(default)]
    end_timestamp: i64,
}

#[async_trait]
impl ScraperEngine for FlareSolverrEngine {
    /// Execute a scraping request using FlareSolverr
    async fn scrape(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        if request.method != crate::engines::engine_client::HttpMethod::Get {
            return Err(EngineError::Other("Unsupported HTTP method".to_string()));
        }
        let start_time = std::time::Instant::now();

        // Build FlareSolverr request
        let fs_request = FlareSolverrRequest {
            cmd: "request.get".to_string(),
            url: request.url.clone(),
            session: self.session_id.clone(),
            max_timeout: Some(request.timeout.as_millis() as u64),
            return_screenshot: None,
            wait_in_seconds: if request.sync_wait_ms > 0 {
                Some(request.sync_wait_ms as u64 / 1000)
            } else {
                None
            },
            disable_media: None,
            cookies: None,
            post_data: None,
        };

        // Add custom headers if present (FlareSolverr doesn't support custom headers directly,
        // but we can set them in the request)
        if !request.headers.is_empty() {
            warn!(
                "Custom headers are not directly supported by FlareSolverr, ignoring {} headers",
                request.headers.len()
            );
        }

        // Build the base URL for FlareSolverr API
        // Ensure no double slashes or duplicate /v1
        let base_url = self.config.url.trim_end_matches('/');
        let api_url = if base_url.ends_with("/v1") {
            base_url.to_string()
        } else {
            format!("{}/v1", base_url)
        };

        debug!(
            "FlareSolverr request: url={}, session={:?}",
            request.url, fs_request.session
        );

        // Send request to FlareSolverr
        let raw_response = self
            .client
            .post(&api_url)
            .json(&fs_request)
            .send()
            .await
            .map_err(|e| EngineError::Other(format!("FlareSolverr request failed: {}", e)))?;

        // Get raw text first to debug any encoding issues
        let raw_text = raw_response
            .text()
            .await
            .map_err(|e| EngineError::Other(format!("Failed to get response text: {}", e)))?;

        debug!("FlareSolverr raw response length: {}", raw_text.len());

        // Try to parse as JSON
        let response: FlareSolverrResponse = serde_json::from_str(&raw_text).map_err(|e| {
            EngineError::Other(format!(
                "Failed to parse FlareSolverr response: {} (first 200 chars: {:?})",
                e,
                &raw_text[..raw_text.len().min(200)]
            ))
        })?;

        // Check response status
        if response.status != "ok" {
            error!("FlareSolverr error: {}", response.message);
            return Err(EngineError::Other(format!(
                "FlareSolverr error: {}",
                response.message
            )));
        }

        // Get solution
        let solution = response.solution.ok_or_else(|| {
            EngineError::Other("No solution in FlareSolverr response".to_string())
        })?;

        // Check for CAPTCHA or bot detection pages
        let html_content = &solution.response;
        let is_captcha_page = html_content.contains("captcha")
            || html_content.contains("CAPTCHA")
            || html_content.contains("unusual traffic")
            || html_content.contains("Our systems have detected")
            || html_content.contains("recaptcha")
            || html_content.contains("verify you are human");

        if is_captcha_page {
            warn!("FlareSolverr returned CAPTCHA/bot detection page");
            return Err(EngineError::Other(
                "FlareSolverr blocked by CAPTCHA or bot detection".to_string(),
            ));
        }

        let response_time_ms = start_time.elapsed().as_millis() as u64;

        // Build headers from solution
        let mut headers = solution.headers.unwrap_or_default();

        // Add content-type if not present
        if !headers.contains_key("content-type") {
            headers.insert("content-type".to_string(), "text/html".to_string());
        }

        // Build response - FlareSolverr returns HTML content
        let scrape_response = InternalScrapeResponse {
            status_code: solution.status,
            content: solution.response,
            content_type: headers
                .get("content-type")
                .cloned()
                .unwrap_or_else(|| "text/html".to_string()),
            screenshot: solution.screenshot,
            headers,
            response_time_ms,
        };

        info!(
            "FlareSolverr success: status={}, time={}ms, content_length={}",
            scrape_response.status_code,
            response_time_ms,
            scrape_response.content.len()
        );

        Ok(scrape_response)
    }

    /// Calculate support score for the request
    ///
    /// # Arguments
    ///
    /// * `request` - The scrape request
    ///
    /// # Returns
    ///
    /// Support score (0-100). FlareSolverr is best for:
    /// - JavaScript-heavy sites (returns 100)
    /// - Cloudflare/protected sites (returns 100)
    /// - Static content (returns 80, slightly lower than Reqwest for performance)
    fn support_score(&self, request: &InternalScrapeRequest) -> u8 {
        if request.method != crate::engines::engine_client::HttpMethod::Get {
            return 0;
        }
        // FlareSolverr is excellent for JS rendering and anti-bot protection
        if request.needs_js {
            return 100;
        }

        // For non-JS requests, still useful for protected sites
        // But Reqwest would be faster for simple static content
        80
    }

    /// Get engine name
    fn name(&self) -> &'static str {
        "flaresolverr"
    }

    /// FlareSolverr doesn't support TLS fingerprinting directly
    fn supports_tls_fingerprint(&self) -> bool {
        false
    }
}
