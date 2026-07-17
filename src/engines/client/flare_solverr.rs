// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! FlareSolverr Engine - Uses FlareSolverr API to bypass Cloudflare and other anti-bot protections
//!
//! FlareSolverr is a proxy server that uses Selenium with undetected-chromedriver
//! to bypass Cloudflare protection and other anti-bot measures.
//!
//! 本模块合并了原 `FireEngineCdp`（CDP 模式）和 `FireEngineTls`（TLS 模式）
//! 两个独立引擎，统一为 `FlareSolverrEngine` + `FlareSolverrMode` 模式枚举。
//! 三种模式（Full / Cdp / Tls）共享同一个 FlareSolverr API 客户端实现，
//! 仅在 support_score 和 name 上有差异，Tls 模式额外拒绝截图请求。
//!
//! This engine is particularly useful for:
//! - Google search (bypasses CAPTCHA)
//! - Cloudflare-protected sites
//! - Sites with strong anti-bot measures

use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// FlareSolverr 工作模式枚举
///
/// 合并原 `FireEngineCdp` / `FireEngineTls` / `FlareSolverrEngine` 三个引擎。
/// 所有模式共享 FlareSolverr API 调用逻辑，仅在以下方面有差异：
/// - `support_score`：不同模式的优先级策略
/// - `name`：用于路由器注册和日志
/// - `scrape`：Tls 模式拒绝截图请求
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlareSolverrMode {
    /// 完整模式（原 FlareSolverrEngine）
    ///
    /// 支持完整功能：JS 渲染、session 管理、CAPTCHA 检测、截图返回。
    /// 最适合 needs_js 请求（score=100）。
    Full,
    /// CDP 模式（原 FireEngineCdp）
    ///
    /// 支持完整浏览器自动化、JS 渲染、截图、TLS 指纹对抗。
    /// 成本较高，速度较慢。
    Cdp,
    /// TLS 模式（原 FireEngineTls）
    ///
    /// 专注于 TLS 指纹对抗，速度较快，不支持截图和复杂 JS 交互。
    /// 拒绝 needs_screenshot 请求。
    Tls,
}

impl Default for FlareSolverrMode {
    fn default() -> Self {
        Self::Full
    }
}

impl FlareSolverrMode {
    /// 获取该模式下的引擎名称（用于 ScraperEngine::name()）
    pub fn engine_name(&self) -> &'static str {
        match self {
            Self::Full => "flaresolverr",
            Self::Cdp => "fire_engine_cdp",
            Self::Tls => "fire_engine_tls",
        }
    }

    /// 是否支持 TLS 指纹对抗
    ///
    /// - Full: FlareSolverr 本身不直接暴露 TLS 指纹控制（保持原行为）
    /// - Cdp: 支持（通过浏览器自动化）
    /// - Tls: 支持（专门为 TLS 指纹对抗设计）
    pub fn supports_tls_fingerprint(&self) -> bool {
        match self {
            Self::Full => false,
            Self::Cdp | Self::Tls => true,
        }
    }
}

/// FlareSolverr configuration
#[derive(Debug, Clone)]
pub struct FlareSolverrConfig {
    /// FlareSolverr server URL
    pub url: String,
    /// Request timeout in seconds
    pub timeout_seconds: u64,
    /// Default session ID (optional)
    pub session_id: Option<String>,
    /// 工作模式（默认 Full）
    pub mode: FlareSolverrMode,
    /// 代理 URL（用于 Cdp/Tls 模式记录到 X-Proxy-URL header）
    pub proxy_url: Option<String>,
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
            mode: FlareSolverrMode::default(),
            proxy_url: None,
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

/// 脱敏代理 URL 中的凭证信息
///
/// 若代理 URL 含 `user:pass@host` 形式的 userinfo，将其替换为 `***@host`，
/// 防止日志泄露凭证。无法解析的 URL 原样返回（不阻塞日志）。
fn redact_proxy_url(proxy_url: &str) -> String {
    let parsed = match url::Url::parse(proxy_url) {
        Ok(u) => u,
        Err(_) => return proxy_url.to_string(),
    };

    let username = parsed.username();
    let password = parsed.password();

    if username.is_empty() && password.is_none() {
        // 无凭证，原样返回
        return proxy_url.to_string();
    }

    // 重建脱敏 URL：保留 scheme/host/port/path，替换 userinfo
    let mut redacted = String::new();
    redacted.push_str(parsed.scheme());
    redacted.push_str("://***@");

    if let Some(host_str) = parsed.host_str() {
        redacted.push_str(host_str);
    }
    if let Some(port) = parsed.port() {
        redacted.push_str(&format!(":{}", port));
    }
    redacted.push_str(parsed.path());
    if let Some(query) = parsed.query() {
        redacted.push_str(&format!("?{}", query));
    }
    if let Some(fragment) = parsed.fragment() {
        redacted.push_str(&format!("#{}", fragment));
    }

    redacted
}

#[cfg(test)]
mod redact_proxy_url_tests {
    use super::redact_proxy_url;

    #[test]
    fn test_redact_proxy_url_no_credentials() {
        // 无凭证的代理 URL 应原样返回
        assert_eq!(
            redact_proxy_url("http://proxy.example.com:8080"),
            "http://proxy.example.com:8080"
        );
    }

    #[test]
    fn test_redact_proxy_url_with_user_only() {
        // 仅有用户名的代理 URL 应脱敏用户名
        assert_eq!(
            redact_proxy_url("http://user@proxy.example.com:8080"),
            "http://***@proxy.example.com:8080"
        );
    }

    #[test]
    fn test_redact_proxy_url_with_user_and_password() {
        // 含 user:pass 的代理 URL 应完全脱敏 userinfo
        assert_eq!(
            redact_proxy_url("http://user:secret@proxy.example.com:8080"),
            "http://***@proxy.example.com:8080"
        );
    }

    #[test]
    fn test_redact_proxy_url_invalid_url_returned_as_is() {
        // 无法解析的 URL 原样返回（不 panic，不阻塞日志）
        assert_eq!(redact_proxy_url("not a url at all"), "not a url at all");
    }

    #[test]
    fn test_redact_proxy_url_preserves_path_and_query() {
        // 脱敏后应保留 path 和 query
        assert_eq!(
            redact_proxy_url("http://user:pass@proxy.example.com:8080/path?q=1"),
            "http://***@proxy.example.com:8080/path?q=1"
        );
    }
}

/// FlareSolverr HTTP client
///
/// 统一的 FlareSolverr API 客户端，通过 `mode` 字段区分工作模式。
/// 取代原 `FireEngineCdp` / `FireEngineTls` / `FlareSolverrEngine` 三个独立 struct。
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
    /// Create a new FlareSolverrEngine with default configuration (Full mode)
    pub fn new(client: Arc<Client>) -> Self {
        Self::with_config(client, FlareSolverrConfig::default())
    }

    /// Create a new FlareSolverrEngine from configuration URL (Full mode)
    pub fn with_url(client: Arc<Client>, url: impl Into<String>) -> Self {
        let config = FlareSolverrConfig {
            url: url.into(),
            timeout_seconds: 60,
            session_id: None,
            mode: FlareSolverrMode::Full,
            proxy_url: None,
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

    /// Create a FlareSolverrEngine in CDP mode with proxy
    ///
    /// 取代原 `FireEngineCdp::new` / `FireEngineCdp::with_proxy`。
    /// `proxy_url` 仅用于记录到 X-Proxy-URL header 供 FlareSolverr 识别代理。
    pub fn with_cdp_mode(client: Arc<Client>, proxy_url: Option<&str>) -> Self {
        Self::with_mode_and_proxy(client, FlareSolverrMode::Cdp, proxy_url)
    }

    /// Create a FlareSolverrEngine in CDP mode with URL and proxy
    ///
    /// 取代原 `FireEngineCdp::with_url_and_proxy`。
    pub fn with_cdp_mode_and_url(
        client: Arc<Client>,
        base_url: &str,
        proxy_url: Option<&str>,
    ) -> Self {
        let config = FlareSolverrConfig {
            url: base_url.to_string(),
            timeout_seconds: 60,
            session_id: None,
            mode: FlareSolverrMode::Cdp,
            proxy_url: proxy_url.map(|s| s.to_string()),
        };
        Self::with_config(client, config)
    }

    /// Create a FlareSolverrEngine in TLS mode with proxy
    ///
    /// 取代原 `FireEngineTls::new` / `FireEngineTls::with_proxy`。
    pub fn with_tls_mode(client: Arc<Client>, proxy_url: Option<&str>) -> Self {
        Self::with_mode_and_proxy(client, FlareSolverrMode::Tls, proxy_url)
    }

    /// Create a FlareSolverrEngine in TLS mode with URL and proxy
    ///
    /// 取代原 `FireEngineTls::with_url_and_proxy`。
    pub fn with_tls_mode_and_url(
        client: Arc<Client>,
        base_url: &str,
        proxy_url: Option<&str>,
    ) -> Self {
        let config = FlareSolverrConfig {
            url: base_url.to_string(),
            timeout_seconds: 60,
            session_id: None,
            mode: FlareSolverrMode::Tls,
            proxy_url: proxy_url.map(|s| s.to_string()),
        };
        Self::with_config(client, config)
    }

    /// 通用构造：根据 mode 和 proxy_url 创建实例（base_url 用默认或环境变量）
    fn with_mode_and_proxy(
        client: Arc<Client>,
        mode: FlareSolverrMode,
        proxy_url: Option<&str>,
    ) -> Self {
        let config = FlareSolverrConfig {
            url: std::env::var("FLARESOLVERR_URL")
                .unwrap_or_else(|_| "http://localhost:8191".to_string()),
            timeout_seconds: 60,
            session_id: None,
            mode,
            proxy_url: proxy_url.map(|s| s.to_string()),
        };
        Self::with_config(client, config)
    }

    /// Create a builder for FlareSolverrEngine
    pub fn builder() -> FlareSolverrEngineBuilder {
        FlareSolverrEngineBuilder::default()
    }

    /// Get the current session ID
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Get the current working mode
    pub fn mode(&self) -> FlareSolverrMode {
        self.config.mode
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

    /// Set working mode
    pub fn with_mode(mut self, mode: FlareSolverrMode) -> Self {
        self.config.mode = mode;
        self
    }

    /// Set proxy URL (for Cdp/Tls modes)
    pub fn with_proxy(mut self, proxy_url: &str) -> Self {
        self.config.proxy_url = Some(proxy_url.to_string());
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
    /// 自定义请求头（用于 Cdp/Tls 模式传递 X-Proxy-URL 等）
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_headers: Option<HashMap<String, String>>,
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

        // Tls 模式拒绝截图请求（保持原 FireEngineTls 行为）
        if self.config.mode == FlareSolverrMode::Tls && request.needs_screenshot {
            return Err(EngineError::Other(
                "FireEngineTls does not support screenshots".to_string(),
            ));
        }

        let start_time = std::time::Instant::now();

        // Determine proxy to use: request-level override or engine-level default
        let proxy_url = request.proxy.as_ref().or(self.config.proxy_url.as_ref());

        // Prepare custom headers:合并 proxy 信息 + 用户传入的 headers
        // 不再静默丢弃用户 headers（B2/HIGH 修复）
        let mut custom_headers: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        if let Some(proxy) = proxy_url {
            custom_headers.insert("X-Proxy-URL".to_string(), proxy.clone());
            // 使用脱敏后的 proxy URL 打印日志（避免凭证泄露 - 安全 HIGH 修复）
            debug!(
                "{} using proxy: {}",
                self.config.mode.engine_name(),
                redact_proxy_url(proxy)
            );
        }

        // 合并用户传入的 headers（用户 headers 优先级高于 X-Proxy-URL，避免冲突）
        for (key, value) in &request.headers {
            // 不覆盖已设置的 X-Proxy-URL（proxy 信息优先）
            if key != "X-Proxy-URL" {
                custom_headers.insert(key.clone(), value.clone());
            }
        }

        if !request.headers.is_empty() {
            debug!(
                "{} forwarded {} custom headers to FlareSolverr",
                self.config.mode.engine_name(),
                request.headers.len()
            );
        }

        // Build FlareSolverr request
        let fs_request = FlareSolverrRequest {
            cmd: "request.get".to_string(),
            url: request.url.clone(),
            session: self.session_id.clone(),
            max_timeout: Some(request.timeout.as_millis() as u64),
            return_screenshot: if request.needs_screenshot {
                Some(true)
            } else {
                None
            },
            wait_in_seconds: if request.sync_wait_ms > 0 {
                Some(request.sync_wait_ms as u64 / 1000)
            } else {
                None
            },
            disable_media: None,
            cookies: None,
            post_data: None,
            custom_headers: if custom_headers.is_empty() {
                None
            } else {
                Some(custom_headers)
            },
        };

        // Build the base URL for FlareSolverr API
        // Ensure no double slashes or duplicate /v1
        let base_url = self.config.url.trim_end_matches('/');
        let api_url = if base_url.ends_with("/v1") {
            base_url.to_string()
        } else {
            format!("{}/v1", base_url)
        };

        debug!(
            "FlareSolverr request: mode={}, url={}, session={:?}",
            self.config.mode.engine_name(),
            request.url,
            fs_request.session
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
            "FlareSolverr success: mode={}, status={}, time={}ms, content_length={}",
            self.config.mode.engine_name(),
            scrape_response.status_code,
            response_time_ms,
            scrape_response.content.len()
        );

        Ok(scrape_response)
    }

    /// Calculate support score for the request
    ///
    /// 根据 `mode` 返回不同的优先级分数：
    /// - `Full`：原 FlareSolverrEngine 行为，needs_js=100, default=80
    /// - `Cdp`：原 FireEngineCdp 行为，全功能但成本高
    /// - `Tls`：原 FireEngineTls 行为，TLS 指纹优先，不支持截图
    fn support_score(&self, request: &InternalScrapeRequest) -> u8 {
        if request.method != crate::engines::engine_client::HttpMethod::Get {
            return 0;
        }

        match self.config.mode {
            FlareSolverrMode::Full => {
                // FlareSolverr is excellent for JS rendering and anti-bot protection
                if request.needs_js {
                    return 100;
                }
                // For non-JS requests, still useful for protected sites
                // But Reqwest would be faster for simple static content
                80
            }
            FlareSolverrMode::Cdp => {
                // 如果需要 TLS 指纹且需要截图，这是最佳选择
                if request.needs_tls_fingerprint && request.needs_screenshot {
                    return 100;
                }
                // 如果明确请求使用 Fire Engine
                if request.use_fire_engine {
                    return 100;
                }
                // 如果需要截图，但不需要 TLS，Playwright 可能更好，但这个也能做
                if request.needs_screenshot {
                    return 80;
                }
                // 如果需要 JS，支持
                if request.needs_js {
                    return 90;
                }
                // 如果有交互动作，支持
                if !request.actions.is_empty() {
                    return 90;
                }
                // 成本较高，默认优先级低
                40
            }
            FlareSolverrMode::Tls => {
                // 如果明确请求使用 Fire Engine (TLS 模式)
                if request.use_fire_engine {
                    return 95;
                }
                // 如果需要 TLS 指纹但不需要截图，TLS Engine 是最佳选择
                if request.needs_tls_fingerprint {
                    return 90;
                }
                // 如果需要 JS，支持
                if request.needs_js {
                    return 60;
                }
                // 如果有交互动作，支持但不是最佳
                if !request.actions.is_empty() {
                    return 50;
                }
                // 成本低，速度快，默认优先级高
                70
            }
        }
    }

    /// Get engine name
    fn name(&self) -> &'static str {
        self.config.mode.engine_name()
    }

    /// 是否支持 TLS 指纹对抗（根据 mode 返回）
    fn supports_tls_fingerprint(&self) -> bool {
        self.config.mode.supports_tls_fingerprint()
    }
}
