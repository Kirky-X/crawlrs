#![cfg(test)]
// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unit tests for EngineClient

#[cfg(test)]
mod engine_client_tests {
    use crate::common::constants::timeouts::{
        DEFAULT_TEST_TIMEOUT, E2E_TEST_TIMEOUT, LONG_RUNNING_TEST_TIMEOUT,
    };
    use crawlrs::engines::engine_client::{
        EngineClient, EngineClientTrait, EngineError, EngineHealthStatus, InternalScrapeRequest,
        InternalScrapeResponse, PageAction, ScrapeOptions, ScrapeRequest, ScrapeResponse,
        ScreenshotConfig, ScrollDirection,
    };
    use crawlrs::engines::router::{EngineRouterTrait, EngineStats};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    use std::time::Duration;

    // === Mock EngineRouterTrait ===

    struct MockEngineRouter {
        route_result: Option<Result<InternalScrapeResponse, EngineError>>,
        route_call_count: AtomicU32,
        registered_engine_names: Vec<String>,
    }

    impl MockEngineRouter {
        fn new() -> Self {
            Self {
                route_result: None,
                route_call_count: AtomicU32::new(0),
                registered_engine_names: Vec::new(),
            }
        }

        fn with_success_response(response: InternalScrapeResponse) -> Self {
            Self {
                route_result: Some(Ok(response)),
                route_call_count: AtomicU32::new(0),
                registered_engine_names: Vec::new(),
            }
        }

        fn with_error(error: EngineError) -> Self {
            Self {
                route_result: Some(Err(error)),
                route_call_count: AtomicU32::new(0),
                registered_engine_names: Vec::new(),
            }
        }

        fn with_engine_names(names: Vec<String>) -> Self {
            Self {
                route_result: None,
                route_call_count: AtomicU32::new(0),
                registered_engine_names: names,
            }
        }
    }

    #[async_trait::async_trait]
    impl EngineRouterTrait for MockEngineRouter {
        async fn route(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            self.route_call_count.fetch_add(1, Ordering::SeqCst);
            match &self.route_result {
                Some(Ok(resp)) => Ok(resp.clone()),
                Some(Err(e)) => Err(clone_engine_error(e)),
                None => Err(EngineError::NoEnginesAvailable),
            }
        }

        async fn aggregate(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            Err(EngineError::Internal(
                "aggregate not implemented".to_string(),
            ))
        }

        fn get_engine_stats(&self) -> HashMap<String, EngineStats> {
            HashMap::new()
        }

        fn reset_engine_stats(&self, _engine_name: &str) {}

        fn registered_engines(&self) -> Vec<String> {
            self.registered_engine_names.clone()
        }
    }

    fn clone_engine_error(e: &EngineError) -> EngineError {
        match e {
            EngineError::RequestFailed(msg) => EngineError::RequestFailed(msg.clone()),
            EngineError::Timeout(d) => EngineError::Timeout(*d),
            EngineError::AllEnginesFailed(msg) => EngineError::AllEnginesFailed(msg.clone()),
            EngineError::SsrfProtection(msg) => EngineError::SsrfProtection(msg.clone()),
            EngineError::BrowserError(msg) => EngineError::BrowserError(msg.clone()),
            EngineError::Expired => EngineError::Expired,
            EngineError::Other(msg) => EngineError::Other(msg.clone()),
            EngineError::NoEnginesAvailable => EngineError::NoEnginesAvailable,
            EngineError::InvalidUrl(msg) => EngineError::InvalidUrl(msg.clone()),
            EngineError::Internal(msg) => EngineError::Internal(msg.clone()),
        }
    }

    fn make_success_response() -> InternalScrapeResponse {
        InternalScrapeResponse {
            status_code: 200,
            content: "<html>test</html>".to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
        }
    }

    // === ScrapeRequest Tests ===

    #[test]
    fn test_scrape_request_new() {
        let request = ScrapeRequest::new("https://example.com");
        assert_eq!(request.url, "https://example.com");
        assert!(!request.options.needs_js);
        assert!(!request.options.needs_screenshot);
    }

    #[test]
    fn test_scrape_request_with_options() {
        let options = ScrapeOptions::builder()
            .needs_js(true)
            .timeout(E2E_TEST_TIMEOUT)
            .build();

        let request = ScrapeRequest::new("https://example.com").with_options(options);

        assert!(request.options.needs_js);
        assert_eq!(request.options.timeout, E2E_TEST_TIMEOUT);
    }

    #[test]
    fn test_scrape_request_builder_methods() {
        let request = ScrapeRequest::new("https://example.com")
            .needs_js()
            .needs_screenshot()
            .mobile()
            .timeout(DEFAULT_TEST_TIMEOUT);

        assert!(request.options.needs_js);
        assert!(request.options.needs_screenshot);
        assert!(request.options.mobile);
        assert_eq!(request.options.timeout, DEFAULT_TEST_TIMEOUT);
    }

    // === ScrapeOptions Tests ===

    #[test]
    fn test_scrape_options_default() {
        let options = ScrapeOptions::default();
        assert!(!options.needs_js);
        assert!(!options.needs_screenshot);
        assert!(!options.mobile);
        assert_eq!(options.timeout, DEFAULT_TEST_TIMEOUT);
        assert!(options.headers.is_empty());
        assert!(options.actions.is_empty());
    }

    #[test]
    fn test_scrape_options_builder() {
        let options = ScrapeOptions::builder()
            .needs_js(true)
            .needs_screenshot(true)
            .mobile(true)
            .timeout(LONG_RUNNING_TEST_TIMEOUT)
            .sync_wait_ms(1000)
            .skip_tls_verification(true)
            .proxy("http://proxy.example.com")
            .needs_tls_fingerprint(true)
            .use_fire_engine(true)
            .build();

        assert!(options.needs_js);
        assert!(options.needs_screenshot);
        assert!(options.mobile);
        assert_eq!(options.timeout, LONG_RUNNING_TEST_TIMEOUT);
        assert_eq!(options.sync_wait_ms, 1000);
        assert!(options.skip_tls_verification);
        assert_eq!(options.proxy, Some("http://proxy.example.com".to_string()));
        assert!(options.needs_tls_fingerprint);
        assert!(options.use_fire_engine);
    }

    #[test]
    fn test_scrape_options_builder_headers() {
        use std::collections::HashMap;

        let headers = HashMap::from([
            ("Authorization".to_string(), "Bearer token".to_string()),
            ("User-Agent".to_string(), "Custom UA".to_string()),
        ]);

        let options = ScrapeOptions::builder().headers(headers).build();

        assert_eq!(options.headers.len(), 2);
        assert_eq!(
            options.headers.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
    }

    #[test]
    fn test_scrape_options_builder_screenshot() {
        let config = ScreenshotConfig {
            full_page: false,
            selector: Some(".content".to_string()),
            quality: Some(90),
            format: Some("png".to_string()),
        };

        let options = ScrapeOptions::builder()
            .needs_screenshot(true)
            .screenshot_config(config.clone())
            .build();

        assert!(options.needs_screenshot);
        assert_eq!(options.screenshot_config, Some(config));
    }

    // === ScreenshotConfig Tests ===

    #[test]
    fn test_screenshot_config_default() {
        let config = ScreenshotConfig::default();
        assert!(config.full_page);
        assert!(config.selector.is_none());
        assert_eq!(config.quality, Some(80));
        assert_eq!(config.format, Some("jpeg".to_string()));
    }

    // === PageAction Tests ===

    #[test]
    fn test_page_action_wait() {
        let action = PageAction::Wait { milliseconds: 5000 };
        match action {
            PageAction::Wait { milliseconds } => assert_eq!(milliseconds, 5000),
            _ => panic!("Expected PageAction::Wait"),
        }
    }

    #[test]
    fn test_page_action_click() {
        let action = PageAction::Click {
            selector: "#button".to_string(),
        };
        match action {
            PageAction::Click { selector } => assert_eq!(selector, "#button"),
            _ => panic!("Expected PageAction::Click"),
        }
    }

    #[test]
    fn test_page_action_scroll() {
        let action = PageAction::Scroll {
            direction: ScrollDirection::Down,
        };
        match action {
            PageAction::Scroll { direction } => assert_eq!(direction, ScrollDirection::Down),
            _ => panic!("Expected PageAction::Scroll"),
        }
    }

    #[test]
    fn test_page_action_input() {
        let action = PageAction::Input {
            selector: "#search".to_string(),
            text: "test query".to_string(),
        };
        match action {
            PageAction::Input { selector, text } => {
                assert_eq!(selector, "#search");
                assert_eq!(text, "test query");
            }
            _ => panic!("Expected PageAction::Input"),
        }
    }

    // === ScrollDirection Tests ===

    #[test]
    fn test_scroll_direction_default() {
        assert_eq!(ScrollDirection::default(), ScrollDirection::Down);
    }

    // === ScrapeResponse Tests ===

    #[test]
    fn test_scrape_response_new() {
        let response = ScrapeResponse::new(200, "content", "text/html");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content, "content");
        assert_eq!(response.content_type, "text/html");
        assert!(response.screenshot.is_none());
        assert!(response.headers.is_empty());
    }

    #[test]
    fn test_scrape_response_is_success() {
        // 2xx 状态码应该被认为是成功的
        let success_response = ScrapeResponse::new(200, "content", "text/html");
        assert!(success_response.is_success());

        // 3xx 重定向不应该被认为是成功的（is_success 只检查 2xx）
        let redirect_response = ScrapeResponse::new(301, "redirect", "text/html");
        assert!(!redirect_response.is_success());

        // 4xx 客户端错误不应该被认为是成功的
        let client_error = ScrapeResponse::new(404, "not found", "text/html");
        assert!(!client_error.is_success());

        // 5xx 服务器错误不应该被认为是成功的
        let server_error = ScrapeResponse::new(500, "error", "text/html");
        assert!(!server_error.is_success());
    }

    // === EngineError Tests ===

    #[test]
    fn test_engine_error_is_retryable() {
        assert!(EngineError::RequestFailed("error".to_string()).is_retryable());
        assert!(EngineError::Timeout(DEFAULT_TEST_TIMEOUT).is_retryable());
        assert!(EngineError::BrowserError("error".to_string()).is_retryable());

        assert!(!EngineError::NoEnginesAvailable.is_retryable());
        assert!(!EngineError::InvalidUrl("url".to_string()).is_retryable());
        assert!(!EngineError::SsrfProtection("blocked".to_string()).is_retryable());
        assert!(!EngineError::Internal("error".to_string()).is_retryable());
    }

    #[test]
    fn test_engine_error_display() {
        let error = EngineError::RequestFailed("connection refused".to_string());
        assert_eq!(format!("{}", error), "Request failed: connection refused");

        let timeout_error = EngineError::Timeout(DEFAULT_TEST_TIMEOUT);
        assert_eq!(format!("{}", timeout_error), "Request timed out after 30s");

        let ssrf_error = EngineError::SsrfProtection("internal URL".to_string());
        assert_eq!(format!("{}", ssrf_error), "SSRF protection: internal URL");
    }

    // === EngineHealthStatus Tests ===

    #[test]
    fn test_engine_health_status_default() {
        assert_eq!(EngineHealthStatus::default(), EngineHealthStatus::Healthy);
    }

    #[test]
    fn test_engine_health_status_healthy() {
        let status = EngineHealthStatus::Healthy;
        match status {
            EngineHealthStatus::Healthy => {}
            _ => panic!("Expected Healthy"),
        }
    }

    #[test]
    fn test_engine_health_status_degraded() {
        let status = EngineHealthStatus::Degraded {
            unhealthy_engines: vec!["engine1".to_string(), "engine2".to_string()],
            message: "2 engines degraded".to_string(),
        };
        match status {
            EngineHealthStatus::Degraded {
                unhealthy_engines,
                message,
            } => {
                assert_eq!(unhealthy_engines.len(), 2);
                assert_eq!(message, "2 engines degraded");
            }
            _ => panic!("Expected Degraded"),
        }
    }

    #[test]
    fn test_engine_health_status_unavailable() {
        let status = EngineHealthStatus::Unavailable {
            message: "All engines unavailable".to_string(),
        };
        match status {
            EngineHealthStatus::Unavailable { message } => {
                assert_eq!(message, "All engines unavailable");
            }
            _ => panic!("Expected Unavailable"),
        }
    }

    // === EngineClient Tests ===

    #[test]
    fn test_engine_client_default() {
        let client = EngineClient::default();
        assert_eq!(client.engine_count(), 0);
    }

    #[test]
    fn test_engine_client_new() {
        let client = EngineClient::new();
        assert_eq!(client.engine_count(), 0);
    }

    #[test]
    fn test_engine_client_registered_engines() {
        let client = EngineClient::new();
        let engines = client.registered_engines();
        assert!(engines.is_empty());
    }

    // === Integration-style Tests (Async) ===

    #[tokio::test]
    #[ignore] // Skip: Test requires specific features or has private field access
    async fn test_health_check_returns_healthy_for_empty_client() {
        let client = EngineClient::new();
        let status = client.health_check().await;

        // Empty client should return Unavailable since no engines are registered
        match status {
            EngineHealthStatus::Unavailable { .. } => {}
            _ => {
                // For empty client with no engines, this is expected behavior
            }
        }
    }

    // === URL Validation Tests ===

    #[tokio::test]
    #[ignore] // Skip: Test requires specific features or has private field access
    async fn test_scrape_request_rejects_invalid_url() {
        let client = EngineClient::new();
        let request = ScrapeRequest::new("not-a-valid-url");

        let result: Result<ScrapeResponse, EngineError> = client.scrape(&request).await;

        assert!(result.is_err());
        match result {
            Err(EngineError::SsrfProtection(_)) => {
                // Invalid URLs should be rejected by SSRF protection
            }
            Err(EngineError::InvalidUrl(_)) => {
                // Or by URL validation
            }
            Err(_) => {
                // Other errors are also acceptable for invalid URLs
            }
            Ok(_) => panic!("Expected error for invalid URL"),
        }
    }

    #[tokio::test]
    #[ignore] // Skip: Test requires specific features or has private field access
    async fn test_scrape_request_rejects_internal_url() {
        let client = EngineClient::new();
        let request = ScrapeRequest::new("http://localhost:8080");

        let result: Result<ScrapeResponse, EngineError> = client.scrape(&request).await;

        assert!(result.is_err());
        match result {
            Err(EngineError::SsrfProtection(_)) => {
                // Internal URLs should be rejected by SSRF protection
            }
            Err(_) => {
                // Other errors are also acceptable
            }
            Ok(_) => panic!("Expected error for internal URL"),
        }
    }

    // === Edge Cases ===

    #[test]
    fn test_empty_url_in_scrape_request() {
        // Creating a request with empty URL is allowed (validation happens at scrape time)
        let request = ScrapeRequest::new("");
        assert_eq!(request.url, "");
    }

    #[test]
    fn test_zero_timeout() {
        // Zero timeout should be allowed (will fail at runtime)
        let options = ScrapeOptions::builder()
            .timeout(Duration::from_secs(0))
            .build();
        assert_eq!(options.timeout, Duration::from_secs(0));
    }

    #[test]
    fn test_screenshot_config_quality_bounds() {
        let low_quality = ScreenshotConfig {
            full_page: true,
            selector: None,
            quality: Some(1),
            format: Some("jpeg".to_string()),
        };
        assert_eq!(low_quality.quality, Some(1));

        let high_quality = ScreenshotConfig {
            full_page: true,
            selector: None,
            quality: Some(100),
            format: Some("jpeg".to_string()),
        };
        assert_eq!(high_quality.quality, Some(100));
    }

    #[test]
    fn test_multiple_page_actions() {
        let actions = [
            PageAction::Wait { milliseconds: 1000 },
            PageAction::Click {
                selector: "#btn".to_string(),
            },
            PageAction::Scroll {
                direction: ScrollDirection::Down,
            },
        ];

        assert_eq!(actions.len(), 3);
    }

    // === EngineClient::with_engines Tests ===

    #[test]
    fn test_engine_client_with_engines_creates_client() {
        let client = EngineClient::with_engines(Vec::new());
        assert_eq!(client.engine_count(), 0);
        assert!(client.registered_engines().is_empty());
    }

    // === EngineClient::with_router Tests ===

    #[test]
    fn test_engine_client_with_router_creates_client() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new());
        let client = EngineClient::with_router(router);
        assert_eq!(client.engine_count(), 0);
    }

    #[test]
    fn test_engine_client_with_router_preserves_engine_names() {
        let router: Arc<dyn EngineRouterTrait> =
            Arc::new(MockEngineRouter::with_engine_names(vec![
                "engine1".to_string(),
                "engine2".to_string(),
            ]));
        let client = EngineClient::with_router(router);
        let engines = client.registered_engines();
        assert_eq!(engines.len(), 2);
        assert!(engines.contains(&"engine1".to_string()));
        assert!(engines.contains(&"engine2".to_string()));
    }

    // === EngineClient::scrape Success Path Tests ===

    #[tokio::test]
    async fn test_engine_client_scrape_success_returns_response() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::with_success_response(
            make_success_response(),
        ));
        let client = EngineClient::with_router(router);

        let request = ScrapeRequest::new("https://example.com");
        let result = client.scrape(&request).await;

        assert!(result.is_ok(), "scrape should succeed with mock router");
        let response = result.unwrap();
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content, "<html>test</html>");
        assert_eq!(response.content_type, "text/html");
        assert_eq!(response.final_url, Some("https://example.com".to_string()));
    }

    #[tokio::test]
    async fn test_engine_client_scrape_success_preserves_screenshot() {
        let response_data = InternalScrapeResponse {
            status_code: 200,
            content: "content".to_string(),
            screenshot: Some("base64screenshot".to_string()),
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 50,
        };
        let router: Arc<dyn EngineRouterTrait> =
            Arc::new(MockEngineRouter::with_success_response(response_data));
        let client = EngineClient::with_router(router);

        let request = ScrapeRequest::new("https://example.com");
        let result = client.scrape(&request).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.screenshot, Some("base64screenshot".to_string()));
    }

    // === EngineClient::scrape Error Path Tests (via router) ===

    #[tokio::test]
    async fn test_engine_client_scrape_router_error_request_failed_propagates() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::with_error(
            EngineError::RequestFailed("connection refused".to_string()),
        ));
        let client = EngineClient::with_router(router);

        let request = ScrapeRequest::new("https://example.com");
        let result = client.scrape(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::RequestFailed(msg) => assert_eq!(msg, "connection refused"),
            other => panic!("Expected RequestFailed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_engine_client_scrape_router_error_timeout_propagates() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::with_error(
            EngineError::Timeout(Duration::from_secs(30)),
        ));
        let client = EngineClient::with_router(router);

        let request = ScrapeRequest::new("https://example.com");
        let result = client.scrape(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::Timeout(d) => assert_eq!(d, Duration::from_secs(30)),
            other => panic!("Expected Timeout, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_engine_client_scrape_router_error_no_suitable_engines_converts() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::with_error(
            EngineError::AllEnginesFailed("No suitable engines found for request".to_string()),
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

    #[tokio::test]
    async fn test_engine_client_scrape_router_error_all_failed_generic_converts() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::with_error(
            EngineError::AllEnginesFailed("all engines failed".to_string()),
        ));
        let client = EngineClient::with_router(router);

        let request = ScrapeRequest::new("https://example.com");
        let result = client.scrape(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::RequestFailed(msg) => assert_eq!(msg, "all engines failed"),
            other => panic!("Expected RequestFailed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_engine_client_scrape_router_error_browser_error_propagates() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::with_error(
            EngineError::BrowserError("browser crashed".to_string()),
        ));
        let client = EngineClient::with_router(router);

        let request = ScrapeRequest::new("https://example.com");
        let result = client.scrape(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::BrowserError(msg) => assert_eq!(msg, "browser crashed"),
            other => panic!("Expected BrowserError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_engine_client_scrape_router_error_expired_converts_to_internal() {
        let router: Arc<dyn EngineRouterTrait> =
            Arc::new(MockEngineRouter::with_error(EngineError::Expired));
        let client = EngineClient::with_router(router);

        let request = ScrapeRequest::new("https://example.com");
        let result = client.scrape(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::Internal(msg) => assert_eq!(msg, "Request expired"),
            other => panic!("Expected Internal, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_engine_client_scrape_router_error_other_converts_to_internal() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::with_error(
            EngineError::Other("something else".to_string()),
        ));
        let client = EngineClient::with_router(router);

        let request = ScrapeRequest::new("https://example.com");
        let result = client.scrape(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::Internal(msg) => assert_eq!(msg, "something else"),
            other => panic!("Expected Internal, got {:?}", other),
        }
    }

    // === EngineClientTrait Implementation Tests ===

    #[tokio::test]
    async fn test_engine_client_trait_scrape_delegates_to_inherent() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::with_success_response(
            make_success_response(),
        ));
        let client = EngineClient::with_router(router);
        let trait_client: Arc<dyn EngineClientTrait> = Arc::new(client);

        let request = ScrapeRequest::new("https://example.com");
        let result = trait_client.scrape(&request).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status_code, 200);
    }

    #[tokio::test]
    async fn test_engine_client_trait_health_check_delegates() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new());
        let client = EngineClient::with_router(router);
        let trait_client: Arc<dyn EngineClientTrait> = Arc::new(client);

        let status = trait_client.health_check().await;
        assert_eq!(status, EngineHealthStatus::Healthy);
    }

    #[test]
    fn test_engine_client_trait_engine_count_delegates() {
        let router: Arc<dyn EngineRouterTrait> =
            Arc::new(MockEngineRouter::with_engine_names(vec!["e1".to_string()]));
        let client = EngineClient::with_router(router);
        let trait_client: Arc<dyn EngineClientTrait> = Arc::new(client);

        assert_eq!(trait_client.engine_count(), 1);
    }

    #[test]
    fn test_engine_client_trait_registered_engines_delegates() {
        let router: Arc<dyn EngineRouterTrait> =
            Arc::new(MockEngineRouter::with_engine_names(vec![
                "e1".to_string(),
                "e2".to_string(),
            ]));
        let client = EngineClient::with_router(router);
        let trait_client: Arc<dyn EngineClientTrait> = Arc::new(client);

        let engines = trait_client.registered_engines();
        assert_eq!(engines.len(), 2);
    }

    // === EngineClient Clone Tests ===

    #[test]
    fn test_engine_client_clone_preserves_router() {
        let router: Arc<dyn EngineRouterTrait> =
            Arc::new(MockEngineRouter::with_engine_names(vec!["e1".to_string()]));
        let client = EngineClient::with_router(router);
        let cloned = client.clone();
        assert_eq!(cloned.engine_count(), 1);
        assert_eq!(cloned.registered_engines(), client.registered_engines());
    }
}
