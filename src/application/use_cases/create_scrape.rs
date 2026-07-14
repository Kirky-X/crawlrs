// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! CreateScrapeUseCase - Use case for creating scrape tasks
//!
//! This module implements the use case for creating scrape tasks.
//! Uses the new EngineClient API for scraping operations.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::application::dto::scrape_request::{
    ScrapeActionDto, ScrapeOptionsDto, ScrapeRequestDto,
};
use crate::domain::models::DomainError;
use crate::engines::engine_client::{
    EngineClient, HttpMethod, PageAction, ScrapeOptions, ScrapeRequest, ScrapeResponse,
    ScreenshotConfig, ScrollDirection,
};

// === Section: Use Case Definition ===

/// Trait for CreateScrapeUseCase - enables dependency injection
#[async_trait::async_trait]
pub trait CreateScrapeUseCaseTrait: Send + Sync {
    /// Execute the scrape use case with the given request DTO
    async fn execute(&self, request_dto: ScrapeRequestDto) -> Result<ScrapeResponse, DomainError>;
}

pub struct CreateScrapeUseCase {
    engine_client: Arc<EngineClient>,
}

// === Section: Implementation ===

impl CreateScrapeUseCase {
    pub fn new(engine_client: Arc<EngineClient>) -> Self {
        Self { engine_client }
    }

    /// Execute the scrape use case with the given request DTO.
    ///
    /// This method maps the DTO to a scrape request, executes it using
    /// the EngineClient, and returns the response or an error.
    ///
    /// # Arguments
    ///
    /// * `request_dto` - The scrape request DTO containing URL and options
    ///
    /// # Returns
    ///
    /// * `Ok(ScrapeResponse)` on success
    /// * `Err(DomainError)` on failure with specific error type
    pub async fn execute(
        &self,
        request_dto: ScrapeRequestDto,
    ) -> Result<ScrapeResponse, DomainError> {
        self._execute_impl(request_dto).await
    }

    async fn _execute_impl(
        &self,
        request_dto: ScrapeRequestDto,
    ) -> Result<ScrapeResponse, DomainError> {
        let scrape_request = self.map_dto_to_request(request_dto)?;
        self.engine_client
            .scrape(&scrape_request)
            .await
            .map_err(map_engine_error)
    }

    fn map_dto_to_request(&self, dto: ScrapeRequestDto) -> Result<ScrapeRequest, DomainError> {
        let options = dto.options.unwrap_or(ScrapeOptionsDto {
            headers: None,
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

        let headers = self.parse_headers(options.headers)?;

        let screenshot_config = options.screenshot_options.map(|opts| ScreenshotConfig {
            full_page: opts.full_page.unwrap_or(false),
            selector: opts.selector,
            quality: opts.quality,
            format: opts.format,
        });

        let scrape_options = ScrapeOptions {
            method: HttpMethod::Get,
            needs_js: options.js_rendering.unwrap_or(false),
            needs_screenshot: options.screenshot.unwrap_or(false),
            mobile: options.mobile.unwrap_or(false),
            timeout: Duration::from_secs(options.timeout.unwrap_or(30)),
            body: None,
            sync_wait_ms: dto.sync_wait_ms.unwrap_or(0),
            actions: self.parse_actions(dto.actions),
            screenshot_config,
            proxy: options.proxy,
            skip_tls_verification: options.skip_tls_verification.unwrap_or(false),
            headers,
            needs_tls_fingerprint: options.needs_tls_fingerprint.unwrap_or(false),
            use_fire_engine: options.use_fire_engine.unwrap_or(false),
        };

        Ok(ScrapeRequest::new(dto.url).with_options(scrape_options))
    }

    fn parse_actions(&self, dto_actions: Option<Vec<ScrapeActionDto>>) -> Vec<PageAction> {
        dto_actions
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| match a {
                ScrapeActionDto::Wait { milliseconds } => Some(PageAction::Wait { milliseconds }),
                ScrapeActionDto::Click { selector } => Some(PageAction::Click { selector }),
                ScrapeActionDto::Scroll { direction } => {
                    let rust_direction = match direction.as_str() {
                        "up" => ScrollDirection::Up,
                        "down" => ScrollDirection::Down,
                        "top" => ScrollDirection::Top,
                        "bottom" => ScrollDirection::Bottom,
                        _ => ScrollDirection::Down,
                    };
                    Some(PageAction::Scroll {
                        direction: rust_direction,
                    })
                }
                ScrapeActionDto::Screenshot { .. } => {
                    // Screenshot is handled via ScrapeOptions.needs_screenshot
                    None
                }
                ScrapeActionDto::Input { selector, text } => {
                    Some(PageAction::Input { selector, text })
                }
            })
            .collect()
    }

    fn parse_headers(
        &self,
        headers_value: Option<Value>,
    ) -> Result<HashMap<String, String>, DomainError> {
        match headers_value {
            Some(Value::Object(map)) => map
                .into_iter()
                .map(|(k, v)| {
                    let v_str = v.as_str().ok_or_else(|| {
                        DomainError::ValidationError(format!("Invalid header value for key: {}", k))
                    })?;
                    Ok((k, v_str.to_string()))
                })
                .collect(),
            Some(_) => Err(DomainError::ValidationError(
                "Headers must be a map of string key-value pairs".to_string(),
            )),
            None => Ok(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl CreateScrapeUseCaseTrait for CreateScrapeUseCase {
    async fn execute(&self, request_dto: ScrapeRequestDto) -> Result<ScrapeResponse, DomainError> {
        self._execute_impl(request_dto).await
    }
}

// === Section: Module Organization ===

// Note: In a real application, you might have more complex logic here,
// such as handling different extraction rules, webhooks, etc.
// This implementation focuses on the core scraping request flow.

/// Map EngineError to DomainError with specific error types
fn map_engine_error(engine_error: crate::engines::engine_client::EngineError) -> DomainError {
    match engine_error {
        crate::engines::engine_client::EngineError::Timeout(duration) => {
            DomainError::TimeoutError(format!("Request timed out after {:?}", duration))
        }
        crate::engines::engine_client::EngineError::SsrfProtection(msg) => {
            DomainError::SecurityError(msg)
        }
        crate::engines::engine_client::EngineError::InvalidUrl(msg) => DomainError::InvalidUrl(msg),
        crate::engines::engine_client::EngineError::RequestFailed(msg)
        | crate::engines::engine_client::EngineError::BrowserError(msg) => {
            DomainError::NetworkError(msg)
        }
        crate::engines::engine_client::EngineError::NoEnginesAvailable => {
            DomainError::EngineError("No engines available".to_string())
        }
        crate::engines::engine_client::EngineError::Internal(msg) => DomainError::EngineError(msg),
        crate::engines::engine_client::EngineError::AllEnginesFailed(msg) => {
            DomainError::EngineError(msg)
        }
        crate::engines::engine_client::EngineError::Expired => {
            DomainError::EngineError("Engine request expired".to_string())
        }
        crate::engines::engine_client::EngineError::Other(msg) => DomainError::EngineError(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::engine_client::{EngineError, PageAction, ScrollDirection};
    use std::sync::Arc;

    // ============ Helpers ============

    fn make_engine_client() -> Arc<EngineClient> {
        Arc::new(EngineClient::new())
    }

    fn make_dto(url: &str) -> ScrapeRequestDto {
        ScrapeRequestDto {
            url: url.to_string(),
            formats: None,
            include_tags: None,
            exclude_tags: None,
            webhook: None,
            extraction_rules: None,
            actions: None,
            options: None,
            metadata: None,
            sync_wait_ms: None,
        }
    }

    // ============ new ============

    #[test]
    fn test_new_constructs_without_calling_engine() {
        let client = make_engine_client();
        let use_case = CreateScrapeUseCase::new(client);
        // Should construct without panicking; engine_count is 0 for empty client
        assert_eq!(use_case.engine_client.engine_count(), 0);
    }

    // ============ map_dto_to_request ============

    #[test]
    fn test_map_dto_to_request_minimal_uses_defaults() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let dto = make_dto("https://example.com");

        let request = use_case
            .map_dto_to_request(dto)
            .expect("minimal dto should map successfully");

        assert_eq!(request.url, "https://example.com");
        assert_eq!(request.options.method, HttpMethod::Get);
        assert!(!request.options.needs_js);
        assert!(!request.options.needs_screenshot);
        assert!(!request.options.mobile);
        assert_eq!(request.options.timeout, Duration::from_secs(30));
        assert!(request.options.body.is_none());
        assert_eq!(request.options.sync_wait_ms, 0);
        assert!(request.options.actions.is_empty());
        assert!(request.options.screenshot_config.is_none());
        assert!(request.options.proxy.is_none());
        assert!(!request.options.skip_tls_verification);
        assert!(request.options.headers.is_empty());
        assert!(!request.options.needs_tls_fingerprint);
        assert!(!request.options.use_fire_engine);
    }

    #[test]
    fn test_map_dto_to_request_full_options() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let dto = ScrapeRequestDto {
            url: "https://example.com".to_string(),
            formats: None,
            include_tags: None,
            exclude_tags: None,
            webhook: None,
            extraction_rules: None,
            actions: None,
            options: Some(ScrapeOptionsDto {
                headers: Some(serde_json::json!({"X-Custom": "value"})),
                wait_for: Some(1000),
                timeout: Some(60),
                js_rendering: Some(true),
                screenshot: Some(true),
                screenshot_options: None,
                mobile: Some(true),
                proxy: Some("http://proxy:8080".to_string()),
                skip_tls_verification: Some(true),
                needs_tls_fingerprint: Some(true),
                use_fire_engine: Some(true),
            }),
            metadata: None,
            sync_wait_ms: Some(500),
        };

        let request = use_case
            .map_dto_to_request(dto)
            .expect("full options dto should map successfully");

        assert!(request.options.needs_js);
        assert!(request.options.needs_screenshot);
        assert!(request.options.mobile);
        assert_eq!(request.options.timeout, Duration::from_secs(60));
        assert_eq!(request.options.sync_wait_ms, 500);
        assert_eq!(request.options.proxy.as_deref(), Some("http://proxy:8080"));
        assert!(request.options.skip_tls_verification);
        assert!(request.options.needs_tls_fingerprint);
        assert!(request.options.use_fire_engine);
        assert_eq!(
            request.options.headers.get("X-Custom").map(|v| v.as_str()),
            Some("value")
        );
    }

    #[test]
    fn test_map_dto_to_request_screenshot_options_mapped() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let dto = ScrapeRequestDto {
            url: "https://example.com".to_string(),
            formats: None,
            include_tags: None,
            exclude_tags: None,
            webhook: None,
            extraction_rules: None,
            actions: None,
            options: Some(ScrapeOptionsDto {
                headers: None,
                wait_for: None,
                timeout: None,
                js_rendering: None,
                screenshot: None,
                screenshot_options: Some(
                    crate::application::dto::scrape_request::ScreenshotOptionsDto {
                        full_page: Some(true),
                        selector: Some("#content".to_string()),
                        quality: Some(80),
                        format: Some("png".to_string()),
                    },
                ),
                mobile: None,
                proxy: None,
                skip_tls_verification: None,
                needs_tls_fingerprint: None,
                use_fire_engine: None,
            }),
            metadata: None,
            sync_wait_ms: None,
        };

        let request = use_case
            .map_dto_to_request(dto)
            .expect("screenshot options should map successfully");

        let config = request
            .options
            .screenshot_config
            .as_ref()
            .expect("screenshot_config should be Some");
        assert!(config.full_page);
        assert_eq!(config.selector.as_deref(), Some("#content"));
        assert_eq!(config.quality, Some(80));
        assert_eq!(config.format.as_deref(), Some("png"));
    }

    #[test]
    fn test_map_dto_to_request_screenshot_options_defaults_full_page_false() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let dto = ScrapeRequestDto {
            url: "https://example.com".to_string(),
            formats: None,
            include_tags: None,
            exclude_tags: None,
            webhook: None,
            extraction_rules: None,
            actions: None,
            options: Some(ScrapeOptionsDto {
                headers: None,
                wait_for: None,
                timeout: None,
                js_rendering: None,
                screenshot: None,
                screenshot_options: Some(
                    crate::application::dto::scrape_request::ScreenshotOptionsDto {
                        full_page: None,
                        selector: None,
                        quality: None,
                        format: None,
                    },
                ),
                mobile: None,
                proxy: None,
                skip_tls_verification: None,
                needs_tls_fingerprint: None,
                use_fire_engine: None,
            }),
            metadata: None,
            sync_wait_ms: None,
        };

        let request = use_case
            .map_dto_to_request(dto)
            .expect("dto should map successfully");
        let config = request
            .options
            .screenshot_config
            .as_ref()
            .expect("screenshot_config should be Some even with all None");
        assert!(!config.full_page, "full_page should default to false");
    }

    // ============ parse_actions ============

    #[test]
    fn test_parse_actions_none_returns_empty() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let actions = use_case.parse_actions(None);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_parse_actions_empty_vec_returns_empty() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let actions = use_case.parse_actions(Some(vec![]));
        assert!(actions.is_empty());
    }

    #[test]
    fn test_parse_actions_all_meaningful_variants() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let dto_actions = vec![
            ScrapeActionDto::Wait { milliseconds: 500 },
            ScrapeActionDto::Click {
                selector: "#btn".to_string(),
            },
            ScrapeActionDto::Scroll {
                direction: "up".to_string(),
            },
            ScrapeActionDto::Scroll {
                direction: "down".to_string(),
            },
            ScrapeActionDto::Scroll {
                direction: "top".to_string(),
            },
            ScrapeActionDto::Scroll {
                direction: "bottom".to_string(),
            },
            ScrapeActionDto::Input {
                selector: "#field".to_string(),
                text: "hello".to_string(),
            },
        ];

        let actions = use_case.parse_actions(Some(dto_actions));
        // Screenshot is filtered out, so 7 meaningful actions remain
        assert_eq!(actions.len(), 7);

        assert!(matches!(
            &actions[0],
            PageAction::Wait { milliseconds: 500 }
        ));
        assert!(matches!(
            &actions[1],
            PageAction::Click { selector } if selector == "#btn"
        ));
        assert!(matches!(
            &actions[2],
            PageAction::Scroll {
                direction: ScrollDirection::Up
            }
        ));
        assert!(matches!(
            &actions[3],
            PageAction::Scroll {
                direction: ScrollDirection::Down
            }
        ));
        assert!(matches!(
            &actions[4],
            PageAction::Scroll {
                direction: ScrollDirection::Top
            }
        ));
        assert!(matches!(
            &actions[5],
            PageAction::Scroll {
                direction: ScrollDirection::Bottom
            }
        ));
        assert!(matches!(
            &actions[6],
            PageAction::Input { selector, text } if selector == "#field" && text == "hello"
        ));
    }

    #[test]
    fn test_parse_actions_invalid_scroll_direction_defaults_to_down() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let dto_actions = vec![ScrapeActionDto::Scroll {
            direction: "sideways".to_string(),
        }];

        let actions = use_case.parse_actions(Some(dto_actions));
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            PageAction::Scroll {
                direction: ScrollDirection::Down
            }
        ));
    }

    #[test]
    fn test_parse_actions_screenshot_is_filtered_out() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let dto_actions = vec![
            ScrapeActionDto::Screenshot {
                full_page: Some(true),
            },
            ScrapeActionDto::Screenshot { full_page: None },
        ];

        let actions = use_case.parse_actions(Some(dto_actions));
        assert!(
            actions.is_empty(),
            "screenshot actions should be filtered out"
        );
    }

    // ============ parse_headers ============

    #[test]
    fn test_parse_headers_none_returns_empty_map() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let headers = use_case
            .parse_headers(None)
            .expect("None headers should succeed");
        assert!(headers.is_empty());
    }

    #[test]
    fn test_parse_headers_object_with_string_values() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let value =
            serde_json::json!({"Authorization": "Bearer token", "Accept": "application/json"});
        let headers = use_case
            .parse_headers(Some(value))
            .expect("object headers should succeed");
        assert_eq!(headers.len(), 2);
        assert_eq!(
            headers.get("Authorization").map(|v| v.as_str()),
            Some("Bearer token")
        );
        assert_eq!(
            headers.get("Accept").map(|v| v.as_str()),
            Some("application/json")
        );
    }

    #[test]
    fn test_parse_headers_non_object_returns_validation_error() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let result = use_case.parse_headers(Some(serde_json::json!("not-a-map")));
        let err = match result {
            Err(DomainError::ValidationError(msg)) => msg,
            Err(e) => panic!("expected ValidationError, got: {:?}", e),
            Ok(_) => panic!("expected error for non-object headers"),
        };
        assert!(
            err.contains("Headers must be a map"),
            "error should mention map requirement, got: {}",
            err
        );
    }

    #[test]
    fn test_parse_headers_non_string_value_returns_validation_error() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let value = serde_json::json!({"X-Count": 42});
        let result = use_case.parse_headers(Some(value));
        let err = match result {
            Err(DomainError::ValidationError(msg)) => msg,
            Err(e) => panic!("expected ValidationError, got: {:?}", e),
            Ok(_) => panic!("expected error for non-string header value"),
        };
        assert!(
            err.contains("Invalid header value for key: X-Count"),
            "error should mention the offending key, got: {}",
            err
        );
    }

    #[test]
    fn test_parse_headers_empty_object_returns_empty_map() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let headers = use_case
            .parse_headers(Some(serde_json::json!({})))
            .expect("empty object should succeed");
        assert!(headers.is_empty());
    }

    // ============ map_engine_error ============

    #[test]
    fn test_map_engine_error_timeout() {
        let err = map_engine_error(EngineError::Timeout(Duration::from_secs(10)));
        match err {
            DomainError::TimeoutError(msg) => {
                assert!(msg.contains("10s"), "should mention duration, got: {}", msg)
            }
            e => panic!("expected TimeoutError, got: {:?}", e),
        }
    }

    #[test]
    fn test_map_engine_error_ssrf_protection() {
        let err = map_engine_error(EngineError::SsrfProtection("blocked".to_string()));
        match err {
            DomainError::SecurityError(msg) => assert_eq!(msg, "blocked"),
            e => panic!("expected SecurityError, got: {:?}", e),
        }
    }

    #[test]
    fn test_map_engine_error_invalid_url() {
        let err = map_engine_error(EngineError::InvalidUrl("bad url".to_string()));
        match err {
            DomainError::InvalidUrl(msg) => assert_eq!(msg, "bad url"),
            e => panic!("expected InvalidUrl, got: {:?}", e),
        }
    }

    #[test]
    fn test_map_engine_error_request_failed_maps_to_network() {
        let err = map_engine_error(EngineError::RequestFailed("conn refused".to_string()));
        match err {
            DomainError::NetworkError(msg) => assert_eq!(msg, "conn refused"),
            e => panic!("expected NetworkError, got: {:?}", e),
        }
    }

    #[test]
    fn test_map_engine_error_browser_error_maps_to_network() {
        let err = map_engine_error(EngineError::BrowserError("playwright down".to_string()));
        match err {
            DomainError::NetworkError(msg) => assert_eq!(msg, "playwright down"),
            e => panic!("expected NetworkError, got: {:?}", e),
        }
    }

    #[test]
    fn test_map_engine_error_no_engines_available() {
        let err = map_engine_error(EngineError::NoEnginesAvailable);
        match err {
            DomainError::EngineError(msg) => {
                assert_eq!(msg, "No engines available")
            }
            e => panic!("expected EngineError, got: {:?}", e),
        }
    }

    #[test]
    fn test_map_engine_error_internal() {
        let err = map_engine_error(EngineError::Internal("boom".to_string()));
        match err {
            DomainError::EngineError(msg) => assert_eq!(msg, "boom"),
            e => panic!("expected EngineError, got: {:?}", e),
        }
    }

    #[test]
    fn test_map_engine_error_all_engines_failed() {
        let err = map_engine_error(EngineError::AllEnginesFailed("all dead".to_string()));
        match err {
            DomainError::EngineError(msg) => assert_eq!(msg, "all dead"),
            e => panic!("expected EngineError, got: {:?}", e),
        }
    }

    #[test]
    fn test_map_engine_error_expired() {
        let err = map_engine_error(EngineError::Expired);
        match err {
            DomainError::EngineError(msg) => {
                assert!(
                    msg.contains("expired"),
                    "should mention expired, got: {}",
                    msg
                )
            }
            e => panic!("expected EngineError, got: {:?}", e),
        }
    }

    #[test]
    fn test_map_engine_error_other() {
        let err = map_engine_error(EngineError::Other("misc".to_string()));
        match err {
            DomainError::EngineError(msg) => assert_eq!(msg, "misc"),
            e => panic!("expected EngineError, got: {:?}", e),
        }
    }

    // ============ execute (end-to-end) ============

    #[tokio::test]
    async fn test_execute_with_localhost_returns_security_error() {
        // "localhost" is blocked by SSRF hostname check (no DNS needed),
        // so map_dto_to_request runs fully then engine returns SsrfProtection.
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let dto = make_dto("http://localhost");

        let result = use_case.execute(dto).await;
        let err = match result {
            Err(DomainError::SecurityError(msg)) => msg,
            Err(e) => panic!("expected SecurityError for localhost, got: {:?}", e),
            Ok(_) => panic!("expected error for localhost, got Ok"),
        };
        assert!(
            err.contains("localhost") || err.contains("blocked"),
            "error should mention localhost/blocked, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_execute_with_bad_headers_returns_validation_error_before_engine() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let dto = ScrapeRequestDto {
            url: "https://example.com".to_string(),
            formats: None,
            include_tags: None,
            exclude_tags: None,
            webhook: None,
            extraction_rules: None,
            actions: None,
            options: Some(ScrapeOptionsDto {
                headers: Some(serde_json::json!(123)),
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
            }),
            metadata: None,
            sync_wait_ms: None,
        };

        let result = use_case.execute(dto).await;
        match result {
            Err(DomainError::ValidationError(msg)) => {
                assert!(msg.contains("Headers must be a map"), "got: {}", msg)
            }
            Err(e) => panic!("expected ValidationError, got: {:?}", e),
            Ok(_) => panic!("expected error for bad headers"),
        }
    }

    #[tokio::test]
    async fn test_execute_with_actions_runs_mapping_then_fails_ssrf() {
        // Actions are mapped, then localhost SSRF triggers SecurityError.
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let dto = ScrapeRequestDto {
            url: "http://localhost".to_string(),
            formats: None,
            include_tags: None,
            exclude_tags: None,
            webhook: None,
            extraction_rules: None,
            actions: Some(vec![
                ScrapeActionDto::Click {
                    selector: "#x".to_string(),
                },
                ScrapeActionDto::Scroll {
                    direction: "up".to_string(),
                },
            ]),
            options: None,
            metadata: None,
            sync_wait_ms: Some(100),
        };

        let result = use_case.execute(dto).await;
        assert!(
            matches!(result, Err(DomainError::SecurityError(_))),
            "action mapping should succeed then SSRF should trigger SecurityError, got: {:?}",
            result
        );
    }

    // ============ CreateScrapeUseCaseTrait ============

    #[tokio::test]
    async fn test_trait_execute_delegates_to_impl() {
        let use_case = CreateScrapeUseCase::new(make_engine_client());
        let trait_ref: &dyn CreateScrapeUseCaseTrait = &use_case;
        let dto = make_dto("http://localhost");

        let result = trait_ref.execute(dto).await;
        assert!(
            matches!(result, Err(DomainError::SecurityError(_))),
            "trait execute should also return SecurityError for localhost, got: {:?}",
            result
        );
    }
}
