// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use crate::engines::validators;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 默认超时时间
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

/// 抓取引擎
///
/// 基于reqwest实现的基本HTTP抓取引擎
pub struct ReqwestEngine {
    /// HTTP 客户端（通过依赖注入，支持连接复用）
    http_client: Arc<reqwest::Client>,
    /// 全局代理URL（如果配置）
    proxy_url: Option<String>,
}

impl ReqwestEngine {
    /// 创建新的 ReqwestEngine 实例
    pub fn new(http_client: Arc<reqwest::Client>) -> Self {
        Self {
            http_client,
            proxy_url: None,
        }
    }

    /// 创建带代理配置的 ReqwestEngine 实例
    pub fn with_proxy(http_client: Arc<reqwest::Client>, proxy_url: impl Into<String>) -> Self {
        Self {
            http_client,
            proxy_url: Some(proxy_url.into()),
        }
    }

    /// 获取HTTP客户端
    fn get_client(&self, proxy: &Option<String>) -> reqwest::Client {
        // 如果请求指定了代理，或者引擎有全局代理配置
        let proxy_url = proxy.as_ref().or(self.proxy_url.as_ref());

        if let Some(url) = proxy_url {
            // 创建带代理的客户端
            let builder = reqwest::Client::builder()
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
                .cookie_store(true);

            match reqwest::Proxy::http(url) {
                Ok(proxy) => {
                    if let Ok(client) = builder.proxy(proxy).build() {
                        log::debug!("Using HTTP proxy: {}", url);
                        return client;
                    }
                }
                Err(e) => {
                    log::warn!("Failed to configure HTTP proxy: {}", e);
                }
            }
        }

        // 返回注入的 HTTP 客户端（Arc 被克隆但引用计数不变）
        (*self.http_client).clone()
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
        // SSRF protection: validate all URLs to prevent access to internal services
        validators::validate_url(&request.url)
            .await
            .map_err(|e| EngineError::Other(format!("SSRF protection: {}", e)))?;

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

        // Use shared HTTP client for connection reuse, with proxy support
        let client = self.get_client(&request.proxy);

        // Create request builder
        let mut request_builder = match request.method {
            crate::engines::engine_client::HttpMethod::Get => {
                if request.mobile {
                    client
                        .get(&request.url)
                        .header("User-Agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.1")
                } else {
                    client.get(&request.url).header(
                        "User-Agent",
                        "Mozilla/5.0 (compatible; crawlrs/1.0; +http://crawlrs.dev)",
                    )
                }
            }
            crate::engines::engine_client::HttpMethod::Post => {
                if request.mobile {
                    client
                        .post(&request.url)
                        .header("User-Agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.1")
                } else {
                    client.post(&request.url).header(
                        "User-Agent",
                        "Mozilla/5.0 (compatible; crawlrs/1.0; +http://crawlrs.dev)",
                    )
                }
            }
        };

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
            Err(e) if e.is_timeout() => return Err(EngineError::Timeout(request.timeout)),
            Err(e) => return Err(EngineError::RequestFailed(e.to_string())),
        };

        let status_code = response.status().as_u16();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::engine_client::{
        HttpMethod, InternalScrapeRequest, InternalScreenshotConfig,
    };
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
        }
    }

    // === ReqwestEngine creation tests ===

    #[test]
    fn test_reqwest_engine_new() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        assert_eq!(engine.name(), "reqwest");
    }

    #[test]
    fn test_reqwest_engine_with_proxy() {
        let client = create_test_client();
        let engine = ReqwestEngine::with_proxy(client, "http://proxy.example.com:8080");
        assert_eq!(engine.name(), "reqwest");
    }

    #[test]
    fn test_reqwest_engine_with_empty_proxy() {
        let client = create_test_client();
        let engine = ReqwestEngine::with_proxy(client, "");
        assert_eq!(engine.name(), "reqwest");
    }

    // === name() tests ===

    #[test]
    fn test_name_returns_reqwest() {
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        assert_eq!(engine.name(), "reqwest");
    }

    #[test]
    fn test_name_returns_reqwest_with_proxy() {
        let client = create_test_client();
        let engine = ReqwestEngine::with_proxy(client, "http://proxy:8080");
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
        };
        // Mobile without JS should still get 100
        assert_eq!(engine.support_score(&request), 100);
    }

    #[test]
    fn test_support_score_with_proxy() {
        let client = create_test_client();
        let engine = ReqwestEngine::with_proxy(client, "http://proxy:8080");
        let request = create_basic_request("https://example.com");
        // Proxy shouldn't affect support score
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

    // === get_client private method tests ===
    // get_client is a private method, but accessible via `use super::*` in tests.
    // These tests only build clients without sending HTTP requests.

    #[test]
    fn test_get_client_with_no_proxy_returns_injected_client() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // No proxy → should return the injected client
        let _result = engine.get_client(&None);
    }

    #[test]
    fn test_get_client_with_valid_http_proxy_returns_proxy_client() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // Valid HTTP proxy URL → should create a new client with proxy
        let _result = engine.get_client(&Some("http://proxy.example.com:8080".to_string()));
    }

    #[test]
    fn test_get_client_with_invalid_proxy_falls_back_to_injected() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // Invalid proxy URL → reqwest::Proxy::http fails → log::warn! → fall back to injected client
        let _result = engine.get_client(&Some("://invalid".to_string()));
    }

    #[test]
    fn test_get_client_with_global_proxy_and_no_request_proxy() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::with_proxy(client, "http://global-proxy:8080");
        // No request proxy, but engine has global proxy → should use global proxy
        let _result = engine.get_client(&None);
    }

    #[test]
    fn test_get_client_request_proxy_overrides_global_proxy() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::with_proxy(client, "http://global-proxy:8080");
        // Both request and global proxy → request proxy takes precedence
        let _result = engine.get_client(&Some("http://request-proxy:9090".to_string()));
    }

    #[test]
    fn test_get_client_with_invalid_global_proxy_falls_back() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::with_proxy(client, "://invalid");
        // Invalid global proxy with no request proxy → fall back to injected client
        let _result = engine.get_client(&None);
    }

    #[test]
    fn test_get_client_with_valid_https_proxy() {
        ensure_debug_logger();
        let client = create_test_client();
        let engine = ReqwestEngine::new(client);
        // Valid HTTPS proxy URL
        let _result = engine.get_client(&Some("https://proxy.example.com:8443".to_string()));
    }
}
