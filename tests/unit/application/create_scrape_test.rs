// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Create scrape use case tests
//!
//! Tests for the CreateScrapeUseCase including request mapping and error handling

use std::collections::HashMap;
use std::time::Duration;
use serde_json::json;
use uuid::Uuid;

use crawlrs::application::dto::scrape_request::{
    ScrapeActionDto, ScrapeOptionsDto, ScrapeRequestDto,
};
use crawlrs::application::use_cases::create_scrape::CreateScrapeUseCase;
use crawlrs::domain::models::DomainError;
use crawlrs::engines::engine_client::{
    EngineClient, EngineError, HttpMethod, PageAction, ScreenshotConfig, ScrollDirection,
    ScrapeOptions, ScrapeRequest, ScrapeResponse,
};

// === Mock Engine Client ===

struct MockEngineClient {
    should_fail: bool,
}

impl MockEngineClient {
    fn new() -> Self {
        Self { should_fail: false }
    }

    fn failing() -> Self {
        Self { should_fail: true }
    }
}

#[async_trait::async_trait]
impl EngineClient for MockEngineClient {
    async fn scrape(&self, _request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        if self.should_fail {
            Err(EngineError::RequestFailed("Mock error".to_string()))
        } else {
            Ok(ScrapeResponse {
                content: "<html>Mock content</html>".to_string(),
                status_code: 200,
                headers: HashMap::new(),
                screenshot: None,
                error: None,
            })
        }
    }
}

// === Helper Functions ===

fn create_test_scrape_request() -> ScrapeRequestDto {
    ScrapeRequestDto {
        url: "https://example.com".to_string(),
        actions: None,
        options: None,
        sync_wait_ms: None,
    }
}

fn create_test_scrape_request_with_options() -> ScrapeRequestDto {
    ScrapeRequestDto {
        url: "https://example.com".to_string(),
        actions: None,
        options: Some(ScrapeOptionsDto {
            headers: Some(json!({"User-Agent": "test"})),
            wait_for: None,
            timeout: Some(30),
            js_rendering: Some(true),
            screenshot: Some(true),
            screenshot_options: None,
            mobile: Some(false),
            proxy: None,
            skip_tls_verification: Some(false),
            needs_tls_fingerprint: Some(false),
            use_fire_engine: Some(false),
        }),
        sync_wait_ms: Some(100),
    }
}

// === Unit Tests ===

#[tokio::test]
async fn test_execute_scrape_success() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let dto = create_test_scrape_request();

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.status_code, 200);
    assert!(response.content.contains("Mock content"));
}

#[tokio::test]
async fn test_execute_scrape_failure() {
    let mock_client = MockEngineClient::failing();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let dto = create_test_scrape_request();

    let result = use_case.execute(dto).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DomainError::NetworkError(_)
    ));
}

#[tokio::test]
async fn test_execute_scrape_with_options() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let dto = create_test_scrape_request_with_options();

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_scrape_with_headers() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.options = Some(ScrapeOptionsDto {
        headers: Some(json!({
            "Authorization": "Bearer token123",
            "Accept": "application/json"
        })),
        wait_for: None,
        timeout: None,
        js_rendering: None,
        screenshot: None,
        screenshot_options: None,
        mobile: None,
        proxy: None,
        skip_tls_verification: None,
        needs_tls_fingerprint: None,
        use_fire_engine: None,
    });

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_scrape_with_actions() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.actions = Some(vec![
        ScrapeActionDto::Wait { milliseconds: 1000 },
        ScrapeActionDto::Click { selector: "#button".to_string() },
        ScrapeActionDto::Input {
            selector: "#input".to_string(),
            text: "test text".to_string(),
        },
        ScrapeActionDto::Scroll { direction: "down".to_string() },
    ]);

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

// === Header Parsing Tests ===

#[tokio::test]
async fn test_parse_valid_headers() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.options = Some(ScrapeOptionsDto {
        headers: Some(json!({
            "X-Custom-Header": "custom-value",
            "X-Another-Header": "another-value"
        })),
        ..Default::default()
    });

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_parse_invalid_headers_not_object() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.options = Some(ScrapeOptionsDto {
        headers: Some(json!("invalid")),
        ..Default::default()
    });

    let result = use_case.execute(dto).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DomainError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_parse_headers_with_non_string_value() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.options = Some(ScrapeOptionsDto {
        headers: Some(json!({
            "Valid-Header": "value",
            "Invalid-Header": 123
        })),
        ..Default::default()
    });

    let result = use_case.execute(dto).await;

    assert!(result.is_err());
}

// === Action Parsing Tests ===

#[tokio::test]
async fn test_parse_scroll_action_up() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.actions = Some(vec![ScrapeActionDto::Scroll {
        direction: "up".to_string(),
    }]);

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_parse_scroll_action_bottom() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.actions = Some(vec![ScrapeActionDto::Scroll {
        direction: "bottom".to_string(),
    }]);

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_parse_scroll_action_invalid_direction() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.actions = Some(vec![ScrapeActionDto::Scroll {
        direction: "invalid".to_string(),
    }]);

    let result = use_case.execute(dto).await;

    // Should default to "down"
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_parse_screenshot_action() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.actions = Some(vec![ScrapeActionDto::Screenshot {
        full_page: Some(true),
        selector: None,
        quality: None,
        format: None,
    }]);

    let result = use_case.execute(dto).await;

    // Screenshot action is handled via options, should still succeed
    assert!(result.is_ok());
}

// === Options Mapping Tests ===

#[tokio::test]
async fn test_map_options_with_timeout() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.options = Some(ScrapeOptionsDto {
        timeout: Some(60),
        ..Default::default()
    });

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_map_options_with_js_rendering() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.options = Some(ScrapeOptionsDto {
        js_rendering: Some(true),
        ..Default::default()
    });

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_map_options_with_screenshot() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.options = Some(ScrapeOptionsDto {
        screenshot: Some(true),
        screenshot_options: Some(crawlrs::application::dto::scrape_request::ScreenshotOptionsDto {
            full_page: Some(true),
            selector: Some("#main".to_string()),
            quality: Some(90),
            format: Some("png".to_string()),
        }),
        ..Default::default()
    });

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_map_options_with_mobile() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.options = Some(ScrapeOptionsDto {
        mobile: Some(true),
        ..Default::default()
    });

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_map_options_with_sync_wait() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.sync_wait_ms = Some(5000);

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_map_options_with_proxy() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.options = Some(ScrapeOptionsDto {
        proxy: Some("http://proxy.example.com:8080".to_string()),
        ..Default::default()
    });

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

// === Error Mapping Tests ===

#[tokio::test]
async fn test_map_timeout_error() {
    struct TimeoutMockClient;
    #[async_trait::async_trait]
    impl EngineClient for TimeoutMockClient {
        async fn scrape(&self, _request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
            Err(EngineError::Timeout(Duration::from_secs(30)))
        }
    }

    let mock_client = TimeoutMockClient;
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let dto = create_test_scrape_request();

    let result = use_case.execute(dto).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DomainError::TimeoutError(_)
    ));
}

#[tokio::test]
async fn test_map_invalid_url_error() {
    struct InvalidUrlMockClient;
    #[async_trait::async_trait]
    impl EngineClient for InvalidUrlMockClient {
        async fn scrape(&self, _request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
            Err(EngineError::InvalidUrl("Invalid URL".to_string()))
        }
    }

    let mock_client = InvalidUrlMockClient;
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let dto = create_test_scrape_request();

    let result = use_case.execute(dto).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DomainError::InvalidUrl(_)
    ));
}

#[tokio::test]
async fn test_map_ssrf_protection_error() {
    struct SsrfMockClient;
    #[async_trait::async_trait]
    impl EngineClient for SsrfMockClient {
        async fn scrape(&self, _request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
            Err(EngineError::SsrfProtection("SSRF blocked".to_string()))
        }
    }

    let mock_client = SsrfMockClient;
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let dto = create_test_scrape_request();

    let result = use_case.execute(dto).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DomainError::SecurityError(_)
    ));
}

// === Edge Cases ===

#[tokio::test]
async fn test_execute_with_empty_url() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.url = "".to_string();

    let result = use_case.execute(dto).await;

    // Engine client should handle invalid URL
    assert!(result.is_err());
}

#[tokio::test]
async fn test_execute_with_none_options() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.options = None;

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_with_zero_timeout() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.options = Some(ScrapeOptionsDto {
        timeout: Some(0),
        ..Default::default()
    });

    let result = use_case.execute(dto).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_with_negative_sync_wait() {
    let mock_client = MockEngineClient::new();
    let use_case = CreateScrapeUseCase::new(std::sync::Arc::new(mock_client));

    let mut dto = create_test_scrape_request();
    dto.sync_wait_ms = Some(-1000);

    let result = use_case.execute(dto).await;

    // Should still work, EngineClient handles validation
    assert!(result.is_ok());
}
