// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Rate limit checking helper
//!
//! Provides a unified rate limit check function used across handlers.
//! Eliminates code duplication in crawl, scrape, search, and webhook handlers.

use crate::domain::services::rate_limiting_service::{RateLimitResult, RateLimitingService};
use crate::presentation::errors::AppError;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use log::error;
use serde_json::json;

/// Check rate limit for an API key and endpoint.
///
/// This helper consolidates the rate limit check logic that was repeated
/// across multiple handlers (crawl, scrape, search, webhook).
///
/// # Arguments
///
/// * `service` - The rate limiting service (implements RateLimitingService trait)
/// * `api_key` - The API key to check
/// * `endpoint` - The endpoint path being accessed
///
/// # Returns
///
/// * `Ok(())` - Rate limit check passed
/// * `Err(Response)` - Rate limit exceeded, with appropriate error response
pub async fn check_rate_limit<T: RateLimitingService + ?Sized>(
    service: &T,
    api_key: &str,
    endpoint: &str,
) -> Result<(), Response> {
    match service.check_rate_limit(api_key, endpoint).await {
        Ok(RateLimitResult::Denied { reason }) => Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "success": false,
                "error": format!("Rate limit exceeded: {}", reason)
            })),
        )
            .into_response()),
        Ok(RateLimitResult::RetryAfter {
            retry_after_seconds,
        }) => Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "success": false,
                "error": "Rate limit exceeded, please retry later",
                "retry_after_seconds": retry_after_seconds
            })),
        )
            .into_response()),
        Err(e) => {
            error!("Rate limiting service error: {}", e);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Check rate limit for an API key and endpoint, returning AppError.
///
/// This variant is for handlers that return `Result<T, AppError>`.
///
/// # Arguments
///
/// * `service` - The rate limiting service (implements RateLimitingService trait)
/// * `api_key` - The API key to check
/// * `endpoint` - The endpoint path being accessed
///
/// # Returns
///
/// * `Ok(())` - Rate limit check passed
/// * `Err(AppError::RateLimit)` - Rate limit exceeded
pub async fn check_rate_limit_as_app_error<T: RateLimitingService + ?Sized>(
    service: &T,
    api_key: &str,
    endpoint: &str,
) -> Result<(), AppError> {
    match service.check_rate_limit(api_key, endpoint).await {
        Ok(RateLimitResult::Denied { reason }) => Err(AppError::RateLimit(format!(
            "Rate limit exceeded: {}",
            reason
        ))),
        Ok(RateLimitResult::RetryAfter {
            retry_after_seconds,
        }) => Err(AppError::RateLimit(format!(
            "Rate limit exceeded, please retry after {} seconds",
            retry_after_seconds
        ))),
        Err(e) => {
            error!("Rate limiting service error: {}", e);
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult,
        QuotaService, RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError,
    };
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use uuid::Uuid;

    /// Which error variant the mock should return from `check_rate_limit`.
    #[derive(Clone, Copy)]
    enum MockError {
        Database,
        Credits,
        Configuration,
        Other,
    }

    struct MockRateLimitingService {
        call_count: Arc<AtomicUsize>,
        result: RateLimitResult,
        /// When `Some`, `check_rate_limit` returns the corresponding error.
        error: Option<MockError>,
    }

    impl MockRateLimitingService {
        fn new(result: RateLimitResult) -> Self {
            Self {
                call_count: Arc::new(AtomicUsize::new(0)),
                result,
                error: None,
            }
        }

        fn with_error() -> Self {
            Self {
                call_count: Arc::new(AtomicUsize::new(0)),
                result: RateLimitResult::Allowed,
                error: Some(MockError::Database),
            }
        }

        /// Build a mock that returns the specified error variant from `check_rate_limit`.
        fn with_error_kind(kind: MockError) -> Self {
            Self {
                call_count: Arc::new(AtomicUsize::new(0)),
                result: RateLimitResult::Allowed,
                error: Some(kind),
            }
        }

        fn make_error(kind: MockError) -> RateLimitingError {
            match kind {
                MockError::Database => RateLimitingError::DatabaseError,
                MockError::Credits => RateLimitingError::CreditsError,
                MockError::Configuration => {
                    RateLimitingError::ConfigurationError("mock config error".to_string())
                }
                MockError::Other => RateLimitingError::Other(anyhow::anyhow!("mock other error")),
            }
        }
    }

    #[async_trait]
    impl RateLimitService for MockRateLimitingService {
        async fn check_rate_limit(
            &self,
            _api_key: &str,
            _endpoint: &str,
        ) -> Result<RateLimitResult, RateLimitingError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            if let Some(kind) = self.error {
                return Err(Self::make_error(kind));
            }
            Ok(self.result.clone())
        }

        async fn get_team_rate_limit_config(
            &self,
            _team_id: Uuid,
        ) -> Result<RateLimitConfig, RateLimitingError> {
            Ok(RateLimitConfig::default())
        }

        async fn update_team_rate_limit_config(
            &self,
            _team_id: Uuid,
            _config: RateLimitConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait]
    impl ConcurrencyControlService for MockRateLimitingService {
        async fn check_team_concurrency(
            &self,
            _team_id: Uuid,
            _task_id: Uuid,
        ) -> Result<ConcurrencyResult, RateLimitingError> {
            Ok(ConcurrencyResult::Allowed)
        }

        async fn release_team_concurrency_slot(
            &self,
            _team_id: Uuid,
            _task_id: Uuid,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_team_current_concurrency(
            &self,
            _team_id: Uuid,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }

        async fn get_team_concurrency_config(
            &self,
            _team_id: Uuid,
        ) -> Result<ConcurrencyConfig, RateLimitingError> {
            Ok(ConcurrencyConfig::default())
        }

        async fn update_team_concurrency_config(
            &self,
            _team_id: Uuid,
            _config: ConcurrencyConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }

    #[async_trait]
    impl BacklogService for MockRateLimitingService {
        async fn process_backlog_tasks(&self, _team_id: Uuid) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait]
    impl QuotaService for MockRateLimitingService {
        async fn check_and_deduct_quota(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: crate::domain::models::CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_quota_balance(&self, _team_id: Uuid) -> Result<i64, RateLimitingError> {
            Ok(1000)
        }
    }

    // 实现组合 trait（向后兼容）
    #[async_trait]
    impl RateLimitingService for MockRateLimitingService {}

    #[tokio::test]
    async fn test_allowed_passed() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let service = MockRateLimitingService {
            call_count: call_count.clone(),
            result: RateLimitResult::Allowed,
            error: None,
        };
        let result = check_rate_limit(&service, "test-key", "/v1/test").await;
        assert!(result.is_ok());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_denied_returns_429() {
        let service = MockRateLimitingService {
            call_count: Arc::new(AtomicUsize::new(0)),
            result: RateLimitResult::Denied {
                reason: "Too many requests".to_string(),
            },
            error: None,
        };
        let result = check_rate_limit(&service, "test-key", "/v1/test").await;
        assert!(result.is_err());
        let response = result.unwrap_err();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_retry_after_returns_429_with_seconds() {
        let service = MockRateLimitingService {
            call_count: Arc::new(AtomicUsize::new(0)),
            result: RateLimitResult::RetryAfter {
                retry_after_seconds: 30,
            },
            error: None,
        };
        let result = check_rate_limit(&service, "test-key", "/v1/test").await;
        assert!(result.is_err());
        let response = result.unwrap_err();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_check_rate_limit_error_fails_open() {
        // When the rate limiting service returns an error, the helper fails open
        // (returns Ok) per the fail-open security policy.
        let service = MockRateLimitingService::with_error();
        let result = check_rate_limit(&service, "test-key", "/v1/test").await;
        assert!(
            result.is_ok(),
            "fail-open should return Ok on service error"
        );
        assert_eq!(
            service.call_count.load(Ordering::SeqCst),
            1,
            "service should be called once"
        );
    }

    // ===== check_rate_limit_as_app_error tests =====

    #[tokio::test]
    async fn test_app_error_allowed() {
        let service = MockRateLimitingService::new(RateLimitResult::Allowed);
        let result = check_rate_limit_as_app_error(&service, "test-key", "/v1/test").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_app_error_denied() {
        let service = MockRateLimitingService::new(RateLimitResult::Denied {
            reason: "Too many requests".to_string(),
        });
        let result = check_rate_limit_as_app_error(&service, "test-key", "/v1/test").await;
        let err = result.expect_err("Denied should map to AppError::RateLimit");
        assert_eq!(err.status_code(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(err.error_code(), "RATE_LIMITED");
        match err {
            AppError::RateLimit(msg) => {
                assert!(msg.contains("Rate limit exceeded"));
                assert!(msg.contains("Too many requests"));
            }
            other => panic!("expected AppError::RateLimit, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_app_error_retry_after() {
        let service = MockRateLimitingService::new(RateLimitResult::RetryAfter {
            retry_after_seconds: 60,
        });
        let result = check_rate_limit_as_app_error(&service, "test-key", "/v1/test").await;
        let err = result.expect_err("RetryAfter should map to AppError::RateLimit");
        assert_eq!(err.status_code(), StatusCode::TOO_MANY_REQUESTS);
        match err {
            AppError::RateLimit(msg) => {
                assert!(msg.contains("retry after"));
                assert!(msg.contains("60 seconds"));
            }
            other => panic!("expected AppError::RateLimit, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_app_error_service_error_fails_open() {
        // Fail-open: service error returns Ok, not AppError.
        let service = MockRateLimitingService::with_error();
        let result = check_rate_limit_as_app_error(&service, "test-key", "/v1/test").await;
        assert!(
            result.is_ok(),
            "fail-open should return Ok on service error"
        );
    }

    #[tokio::test]
    async fn test_check_rate_limit_response_body_denied() {
        // Verify the JSON body structure for the Denied case.
        let service = MockRateLimitingService::new(RateLimitResult::Denied {
            reason: "quota exhausted".to_string(),
        });
        let result = check_rate_limit(&service, "k", "/v1/test").await;
        let response = result.unwrap_err();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.contains("false"),
            "success=false expected: {}",
            body_str
        );
        assert!(
            body_str.contains("quota exhausted"),
            "reason in body expected: {}",
            body_str
        );
    }

    #[tokio::test]
    async fn test_check_rate_limit_response_body_retry_after() {
        // Verify the JSON body structure for the RetryAfter case.
        let service = MockRateLimitingService::new(RateLimitResult::RetryAfter {
            retry_after_seconds: 42,
        });
        let result = check_rate_limit(&service, "k", "/v1/test").await;
        let response = result.unwrap_err();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.contains("42"),
            "retry seconds in body expected: {}",
            body_str
        );
        assert!(
            body_str.contains("retry_after_seconds"),
            "retry_after_seconds key expected: {}",
            body_str
        );
    }

    #[tokio::test]
    async fn test_check_rate_limit_denied_body_contains_exact_reason() {
        // Distinct reason string to ensure format! macro line is fully executed.
        let reason = "quota exhausted for team abc-123";
        let service = MockRateLimitingService::new(RateLimitResult::Denied {
            reason: reason.to_string(),
        });
        let result = check_rate_limit(&service, "key", "/v1/scrape").await;
        let response = result.unwrap_err();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.contains(reason),
            "exact reason expected in body: {}",
            body_str
        );
        assert!(
            body_str.contains("Rate limit exceeded"),
            "prefix expected: {}",
            body_str
        );
    }

    #[tokio::test]
    async fn test_app_error_denied_contains_exact_reason() {
        let reason = "per-minute limit hit";
        let service = MockRateLimitingService::new(RateLimitResult::Denied {
            reason: reason.to_string(),
        });
        let result = check_rate_limit_as_app_error(&service, "k", "/v1/test").await;
        match result.expect_err("should be RateLimit") {
            AppError::RateLimit(msg) => {
                assert!(
                    msg.contains(reason),
                    "msg should contain exact reason: {}",
                    msg
                );
            }
            other => panic!("expected AppError::RateLimit, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_app_error_retry_after_contains_exact_seconds() {
        let service = MockRateLimitingService::new(RateLimitResult::RetryAfter {
            retry_after_seconds: 120,
        });
        let result = check_rate_limit_as_app_error(&service, "k", "/v1/test").await;
        match result.expect_err("should be RateLimit") {
            AppError::RateLimit(msg) => {
                assert!(
                    msg.contains("120 seconds"),
                    "msg should contain seconds: {}",
                    msg
                );
            }
            other => panic!("expected AppError::RateLimit, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_check_rate_limit_with_empty_inputs() {
        // Empty api_key and endpoint should still work (delegates to service).
        let service = MockRateLimitingService::new(RateLimitResult::Allowed);
        let result = check_rate_limit(&service, "", "").await;
        assert!(result.is_ok(), "empty inputs with Allowed should return Ok");
    }

    #[tokio::test]
    async fn test_app_error_with_empty_inputs() {
        let service = MockRateLimitingService::new(RateLimitResult::Allowed);
        let result = check_rate_limit_as_app_error(&service, "", "").await;
        assert!(result.is_ok(), "empty inputs with Allowed should return Ok");
    }

    // ===== Supplementary tests: error variant coverage and boundary cases =====

    #[tokio::test]
    async fn test_check_rate_limit_fail_open_on_credits_error() {
        // Fail-open must hold for all RateLimitingError variants, not just DatabaseError.
        let service = MockRateLimitingService::with_error_kind(MockError::Credits);
        let result = check_rate_limit(&service, "k", "/v1/test").await;
        assert!(result.is_ok(), "CreditsError should fail-open");
        assert_eq!(
            service.call_count.load(Ordering::SeqCst),
            1,
            "service should be called once"
        );
    }

    #[tokio::test]
    async fn test_check_rate_limit_fail_open_on_configuration_error() {
        let service = MockRateLimitingService::with_error_kind(MockError::Configuration);
        let result = check_rate_limit(&service, "k", "/v1/test").await;
        assert!(result.is_ok(), "ConfigurationError should fail-open");
    }

    #[tokio::test]
    async fn test_check_rate_limit_fail_open_on_other_error() {
        let service = MockRateLimitingService::with_error_kind(MockError::Other);
        let result = check_rate_limit(&service, "k", "/v1/test").await;
        assert!(result.is_ok(), "Other error should fail-open");
    }

    #[tokio::test]
    async fn test_app_error_fail_open_on_credits_error() {
        // check_rate_limit_as_app_error must also fail-open on non-DatabaseError variants.
        let service = MockRateLimitingService::with_error_kind(MockError::Credits);
        let result = check_rate_limit_as_app_error(&service, "k", "/v1/test").await;
        assert!(
            result.is_ok(),
            "CreditsError should fail-open in app_error path"
        );
    }

    #[tokio::test]
    async fn test_app_error_fail_open_on_other_error() {
        let service = MockRateLimitingService::with_error_kind(MockError::Other);
        let result = check_rate_limit_as_app_error(&service, "k", "/v1/test").await;
        assert!(
            result.is_ok(),
            "Other error should fail-open in app_error path"
        );
    }

    // ===== Boundary values: empty reason / zero / max seconds =====

    #[tokio::test]
    async fn test_denied_with_empty_reason() {
        // Empty reason string should not panic; JSON body should still contain the prefix.
        let service = MockRateLimitingService::new(RateLimitResult::Denied {
            reason: String::new(),
        });
        let result = check_rate_limit(&service, "k", "/v1/test").await;
        let response = result.unwrap_err();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.contains("Rate limit exceeded"),
            "prefix expected even with empty reason: {}",
            body_str
        );
        assert!(
            body_str.contains("\"success\":false"),
            "success=false expected: {}",
            body_str
        );
    }

    #[tokio::test]
    async fn test_retry_after_with_zero_seconds() {
        // Zero seconds is a boundary value; the response must still be 429.
        let service = MockRateLimitingService::new(RateLimitResult::RetryAfter {
            retry_after_seconds: 0,
        });
        let result = check_rate_limit(&service, "k", "/v1/test").await;
        let response = result.unwrap_err();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.contains("\"retry_after_seconds\":0"),
            "zero seconds expected in body: {}",
            body_str
        );
    }

    #[tokio::test]
    async fn test_retry_after_with_max_seconds() {
        // Maximum seconds boundary; ensure it appears verbatim in the body.
        let max_secs = u64::MAX;
        let service = MockRateLimitingService::new(RateLimitResult::RetryAfter {
            retry_after_seconds: max_secs,
        });
        let result = check_rate_limit(&service, "k", "/v1/test").await;
        let response = result.unwrap_err();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.contains(&max_secs.to_string()),
            "max seconds expected in body: {}",
            body_str
        );
    }

    #[tokio::test]
    async fn test_app_error_denied_with_empty_reason() {
        // Empty reason should still map to AppError::RateLimit with the prefix.
        let service = MockRateLimitingService::new(RateLimitResult::Denied {
            reason: String::new(),
        });
        let result = check_rate_limit_as_app_error(&service, "k", "/v1/test").await;
        match result.expect_err("should be RateLimit") {
            AppError::RateLimit(msg) => {
                assert!(
                    msg.contains("Rate limit exceeded"),
                    "prefix expected: {}",
                    msg
                );
            }
            other => panic!("expected AppError::RateLimit, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_app_error_retry_after_with_zero_seconds() {
        // Zero seconds boundary; message should still mention retry-after.
        let service = MockRateLimitingService::new(RateLimitResult::RetryAfter {
            retry_after_seconds: 0,
        });
        let result = check_rate_limit_as_app_error(&service, "k", "/v1/test").await;
        match result.expect_err("should be RateLimit") {
            AppError::RateLimit(msg) => {
                assert!(
                    msg.contains("0 seconds"),
                    "zero seconds expected in msg: {}",
                    msg
                );
            }
            other => panic!("expected AppError::RateLimit, got {:?}", other),
        }
    }

    // ===== Multiple calls counter =====

    #[tokio::test]
    async fn test_multiple_calls_increment_counter() {
        // Verify the helper does not cache results; each call hits the service.
        let service = MockRateLimitingService::new(RateLimitResult::Allowed);
        for _ in 0..5 {
            let result = check_rate_limit(&service, "k", "/v1/test").await;
            assert!(result.is_ok(), "Allowed should always return Ok");
        }
        assert_eq!(
            service.call_count.load(Ordering::SeqCst),
            5,
            "service should be called 5 times"
        );
    }

    #[tokio::test]
    async fn test_multiple_calls_app_error_increment_counter() {
        let service = MockRateLimitingService::new(RateLimitResult::Allowed);
        for _ in 0..3 {
            let result = check_rate_limit_as_app_error(&service, "k", "/v1/test").await;
            assert!(result.is_ok(), "Allowed should always return Ok");
        }
        assert_eq!(
            service.call_count.load(Ordering::SeqCst),
            3,
            "service should be called 3 times"
        );
    }

    // ===== Response JSON structure completeness =====

    #[tokio::test]
    async fn test_denied_response_json_has_success_false_and_error_key() {
        // Verify the JSON structure has both success and error keys.
        let service = MockRateLimitingService::new(RateLimitResult::Denied {
            reason: "limit hit".to_string(),
        });
        let result = check_rate_limit(&service, "k", "/v1/test").await;
        let response = result.unwrap_err();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("body should be valid JSON");
        assert_eq!(json["success"], serde_json::Value::Bool(false));
        assert!(json["error"].is_string());
        assert!(json["error"].as_str().unwrap().contains("limit hit"));
    }

    #[tokio::test]
    async fn test_retry_after_response_json_has_all_three_keys() {
        // Verify the JSON structure has success, error, and retry_after_seconds keys.
        let service = MockRateLimitingService::new(RateLimitResult::RetryAfter {
            retry_after_seconds: 99,
        });
        let result = check_rate_limit(&service, "k", "/v1/test").await;
        let response = result.unwrap_err();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("body should be valid JSON");
        assert_eq!(json["success"], serde_json::Value::Bool(false));
        assert!(json["error"].is_string());
        assert_eq!(json["retry_after_seconds"], serde_json::json!(99));
    }

    #[tokio::test]
    async fn test_app_error_status_code_and_error_code_for_retry_after() {
        // Verify error_code is RATE_LIMITED for RetryAfter (same as Denied).
        let service = MockRateLimitingService::new(RateLimitResult::RetryAfter {
            retry_after_seconds: 7,
        });
        let result = check_rate_limit_as_app_error(&service, "k", "/v1/test").await;
        let err = result.expect_err("RetryAfter should map to AppError::RateLimit");
        assert_eq!(err.status_code(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(err.error_code(), "RATE_LIMITED");
    }
}
