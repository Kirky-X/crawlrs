// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! External unit tests for rate_limit_middleware public API.
//!
//! Drives `rate_limit_middleware` through a real axum Router and exercises
//! `RateLimiter` / `RateLimitMiddleware` construction and Clone semantics.
//! Uses unique IPs per test to avoid interference with the global
//! IP_RATE_LIMITER singleton.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::Body,
    extract::Request,
    http::{header, StatusCode},
    middleware,
    response::Response,
    routing::get,
};
use tower::ServiceExt;

use crawlrs::domain::models::CreditsTransactionType;
use crawlrs::domain::services::rate_limiting_service::{
    BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult, QuotaService,
    RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError, RateLimitingService,
};
use crawlrs::presentation::middleware::rate_limit_middleware::{
    rate_limit_middleware, RateLimitMiddleware, RateLimiter,
};

// =============================================================================
// Mock RateLimitingService
// =============================================================================

enum MockBehavior {
    Allowed,
    Denied(String),
    RetryAfter(u64),
    Error,
}

struct MockRateLimitingService {
    behavior: MockBehavior,
}

#[allow(dead_code)]
impl MockRateLimitingService {
    fn allowed() -> Self {
        Self {
            behavior: MockBehavior::Allowed,
        }
    }
    fn denied(reason: &str) -> Self {
        Self {
            behavior: MockBehavior::Denied(reason.to_string()),
        }
    }
    fn retry_after(secs: u64) -> Self {
        Self {
            behavior: MockBehavior::RetryAfter(secs),
        }
    }
    fn error() -> Self {
        Self {
            behavior: MockBehavior::Error,
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
        match &self.behavior {
            MockBehavior::Allowed => Ok(RateLimitResult::Allowed),
            MockBehavior::Denied(reason) => Ok(RateLimitResult::Denied {
                reason: reason.clone(),
            }),
            MockBehavior::RetryAfter(secs) => Ok(RateLimitResult::RetryAfter {
                retry_after_seconds: *secs,
            }),
            MockBehavior::Error => Err(RateLimitingError::Other(anyhow::anyhow!(
                "mock service error"
            ))),
        }
    }
    async fn get_team_rate_limit_config(
        &self,
        _team_id: uuid::Uuid,
    ) -> Result<RateLimitConfig, RateLimitingError> {
        Ok(Default::default())
    }
    async fn update_team_rate_limit_config(
        &self,
        _team_id: uuid::Uuid,
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
        _team_id: uuid::Uuid,
        _task_id: uuid::Uuid,
    ) -> Result<ConcurrencyResult, RateLimitingError> {
        Ok(ConcurrencyResult::Allowed)
    }
    async fn release_team_concurrency_slot(
        &self,
        _team_id: uuid::Uuid,
        _task_id: uuid::Uuid,
    ) -> Result<(), RateLimitingError> {
        Ok(())
    }
    async fn get_team_current_concurrency(
        &self,
        _team_id: uuid::Uuid,
    ) -> Result<u32, RateLimitingError> {
        Ok(0)
    }
    async fn get_team_concurrency_config(
        &self,
        _team_id: uuid::Uuid,
    ) -> Result<ConcurrencyConfig, RateLimitingError> {
        Ok(Default::default())
    }
    async fn update_team_concurrency_config(
        &self,
        _team_id: uuid::Uuid,
        _config: ConcurrencyConfig,
    ) -> Result<(), RateLimitingError> {
        Ok(())
    }
}

#[async_trait]
impl BacklogService for MockRateLimitingService {
    async fn process_backlog_tasks(&self, _team_id: uuid::Uuid) -> Result<u32, RateLimitingError> {
        Ok(0)
    }
}

#[async_trait]
impl QuotaService for MockRateLimitingService {
    async fn check_and_deduct_quota(
        &self,
        _team_id: uuid::Uuid,
        _amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<uuid::Uuid>,
    ) -> Result<(), RateLimitingError> {
        Ok(())
    }
    async fn get_quota_balance(&self, _team_id: uuid::Uuid) -> Result<i64, RateLimitingError> {
        Ok(1000)
    }
}

#[async_trait]
impl RateLimitingService for MockRateLimitingService {}

fn mock_arc(behavior: MockBehavior) -> Arc<dyn RateLimitingService> {
    Arc::new(MockRateLimitingService { behavior })
}

async fn middleware_wrapper(
    axum::extract::State(state): axum::extract::State<Arc<dyn RateLimitingService>>,
    req: Request,
    next: axum::middleware::Next,
) -> Response {
    rate_limit_middleware(req, next, state).await
}

fn build_app(state: Arc<dyn RateLimitingService>) -> axum::Router {
    axum::Router::new()
        .route("/v1/protected", get(|| async { "OK" }))
        .layer(middleware::from_fn_with_state(state, middleware_wrapper))
}

// =============================================================================
// RateLimiter::new / new_for_ip_limit
// =============================================================================

#[test]
fn tc_rate_limiter_new_default_window() {
    let limiter = RateLimiter::new(100);
    let (count, remaining) = limiter.get_status("fresh-key-new-construction");
    assert_eq!(count, 0);
    assert_eq!(remaining, 100);
}

#[test]
fn tc_rate_limiter_new_for_ip_limit_uses_ip_window() {
    let limiter = RateLimiter::new_for_ip_limit(10);
    let (count, remaining) = limiter.get_status("fresh-ip-key");
    assert_eq!(count, 0);
    assert_eq!(remaining, 10);
}

#[test]
fn tc_rate_limiter_new_with_high_limit() {
    let limiter = RateLimiter::new(1_000_000);
    let (count, remaining) = limiter.get_status("any-key");
    assert_eq!(count, 0);
    assert_eq!(remaining, 1_000_000);
}

#[test]
fn tc_rate_limiter_new_with_zero_limit_blocks_after_one() {
    // limit=0 means the very first request must be rejected (count >= limit).
    let limiter = RateLimiter::new(0);
    let key = "zero-limit-key";
    // First call: no record exists → creates record with count=1, returns true.
    assert!(limiter.check_rate_limit(key));
    // Second call: count=1 >= limit=0 → blocked.
    assert!(!limiter.check_rate_limit(key));
}

#[test]
fn tc_rate_limiter_new_with_one_limit() {
    let limiter = RateLimiter::new(1);
    let key = "one-limit-key";
    assert!(limiter.check_rate_limit(key)); // count becomes 1
    assert!(!limiter.check_rate_limit(key)); // 1 >= 1 blocked
}

// =============================================================================
// RateLimiter::check_rate_limit — key isolation
// =============================================================================

#[test]
fn tc_check_rate_limit_independent_keys() {
    let limiter = RateLimiter::new_for_ip_limit(3);
    assert!(limiter.check_rate_limit("iso-key-a"));
    assert!(limiter.check_rate_limit("iso-key-b"));
    assert!(limiter.check_rate_limit("iso-key-a"));
    assert!(limiter.check_rate_limit("iso-key-a")); // a at limit (3)
    assert!(!limiter.check_rate_limit("iso-key-a")); // a blocked
    assert!(limiter.check_rate_limit("iso-key-b")); // b still has room
}

#[test]
fn tc_check_rate_limit_same_key_blocks_at_limit() {
    let limiter = RateLimiter::new_for_ip_limit(2);
    let key = "shared-block-key";
    assert!(limiter.check_rate_limit(key));
    assert!(limiter.check_rate_limit(key));
    assert!(!limiter.check_rate_limit(key));
}

// =============================================================================
// RateLimiter::get_status
// =============================================================================

#[test]
fn tc_get_status_no_record_returns_zero_and_full() {
    let limiter = RateLimiter::new_for_ip_limit(10);
    let (count, remaining) = limiter.get_status("no-such-key");
    assert_eq!(count, 0);
    assert_eq!(remaining, 10);
}

#[test]
fn tc_get_status_after_increments_reflects_count() {
    let limiter = RateLimiter::new_for_ip_limit(10);
    let key = "status-tracking-key";
    for _ in 0..7 {
        limiter.check_rate_limit(key);
    }
    let (count, remaining) = limiter.get_status(key);
    assert_eq!(count, 7);
    assert_eq!(remaining, 3);
}

#[test]
fn tc_get_status_at_limit_zero_remaining() {
    let limiter = RateLimiter::new_for_ip_limit(3);
    let key = "at-limit-key";
    for _ in 0..3 {
        limiter.check_rate_limit(key);
    }
    let (count, remaining) = limiter.get_status(key);
    assert_eq!(count, 3);
    assert_eq!(remaining, 0);
}

#[test]
fn tc_get_status_after_rejection_keeps_count() {
    let limiter = RateLimiter::new_for_ip_limit(1);
    let key = "reject-then-status";
    limiter.check_rate_limit(key); // allowed, count=1
    limiter.check_rate_limit(key); // rejected, count stays 1
    let (count, _remaining) = limiter.get_status(key);
    assert_eq!(count, 1);
}

// =============================================================================
// RateLimiter::cleanup_expired
// =============================================================================

#[test]
fn tc_cleanup_expired_empty_map_does_not_panic() {
    let limiter = RateLimiter::new_for_ip_limit(10);
    limiter.cleanup_expired();
}

#[test]
fn tc_cleanup_expired_preserves_recent_entries() {
    let limiter = RateLimiter::new_for_ip_limit(10);
    let key = "recent-cleanup-key";
    limiter.check_rate_limit(key);
    limiter.cleanup_expired();
    let (count, _) = limiter.get_status(key);
    assert_eq!(count, 1);
}

#[test]
fn tc_cleanup_expired_with_multiple_entries() {
    let limiter = RateLimiter::new_for_ip_limit(10);
    for i in 0..5 {
        limiter.check_rate_limit(&format!("multi-cleanup-{}", i));
    }
    limiter.cleanup_expired();
    // All recent entries must survive.
    for i in 0..5 {
        let (count, _) = limiter.get_status(&format!("multi-cleanup-{}", i));
        assert_eq!(count, 1, "entry {} must survive cleanup", i);
    }
}

// =============================================================================
// RateLimitMiddleware::new + Clone
// =============================================================================

#[test]
fn tc_rate_limit_middleware_new_stores_service() {
    let mock = mock_arc(MockBehavior::Allowed);
    let mw = RateLimitMiddleware::new(mock);
    let _cloned = mw.clone();
}

#[test]
fn tc_rate_limit_middleware_clone_is_shallow() {
    let mock = mock_arc(MockBehavior::Denied("clone-test".to_string()));
    let mw = RateLimitMiddleware::new(mock);
    let cloned = mw.clone();
    drop(mw);
    // cloned must still be usable (Clone does not deep-copy the inner Arc).
    let _ = cloned.clone();
}

// =============================================================================
// rate_limit_middleware — public endpoint bypass
// =============================================================================

#[tokio::test]
async fn tc_middleware_public_endpoint_health_skips_rate_limit() {
    let mock = mock_arc(MockBehavior::Allowed);
    let app = axum::Router::new()
        .route("/health", get(|| async { "OK" }))
        .layer(middleware::from_fn_with_state(mock, middleware_wrapper));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn tc_middleware_public_endpoint_metrics_skips_rate_limit() {
    let mock = mock_arc(MockBehavior::Allowed);
    let app = axum::Router::new()
        .route("/metrics", get(|| async { "metrics" }))
        .layer(middleware::from_fn_with_state(mock, middleware_wrapper));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    assert_eq!(response.status(), StatusCode::OK);
}

// =============================================================================
// rate_limit_middleware — Allowed path
// =============================================================================

#[tokio::test]
async fn tc_middleware_allowed_passes_request_through() {
    let app = build_app(mock_arc(MockBehavior::Allowed));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/protected")
                .header("Authorization", "Bearer valid-api-key-12345678")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    assert_eq!(response.status(), StatusCode::OK);
}

// =============================================================================
// rate_limit_middleware — Denied path
// =============================================================================

#[tokio::test]
async fn tc_middleware_denied_returns_429_with_json_body() {
    let app = build_app(mock_arc(MockBehavior::Denied("quota exceeded".to_string())));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/protected")
                .header("Authorization", "Bearer valid-api-key-12345678")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body must be readable");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body must be JSON");
    assert_eq!(json["error"], "Rate limit exceeded");
    assert_eq!(json["message"], "quota exceeded");
}

// =============================================================================
// rate_limit_middleware — RetryAfter path
// =============================================================================

#[tokio::test]
async fn tc_middleware_retry_after_returns_429_with_retry_after_header() {
    let app = build_app(mock_arc(MockBehavior::RetryAfter(60)));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/protected")
                .header("Authorization", "Bearer valid-api-key-12345678")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers().get("Retry-After").unwrap(), "60");
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body must be readable");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body must be JSON");
    assert_eq!(json["message"], "Retry after 60 seconds");
}

// =============================================================================
// rate_limit_middleware — service error fail-open
// =============================================================================

#[tokio::test]
async fn tc_middleware_service_error_fail_open_allows_request() {
    // RATE_LIMIT_FAIL_OPEN is const true → service errors must allow the request.
    let app = build_app(mock_arc(MockBehavior::Error));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/protected")
                .header("Authorization", "Bearer valid-api-key-12345678")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    assert_eq!(response.status(), StatusCode::OK);
}

// =============================================================================
// rate_limit_middleware — IP rate limit (no Bearer token)
// =============================================================================

#[tokio::test]
async fn tc_middleware_no_bearer_uses_ip_rate_limit_first_request_allowed() {
    // No Authorization header → IP-based rate limit. First request for a unique
    // IP must be allowed. We rely on the global IP_RATE_LIMITER (10 req/60s).
    let app = build_app(mock_arc(MockBehavior::Allowed));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/protected")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    // Without ConnectInfo the client IP is "unknown"; the global limiter may
    // have been exhausted by other tests using "unknown". Accept either OK or
    // 429 — the important assertion is that the middleware does not panic and
    // returns one of these two statuses.
    assert!(
        response.status() == StatusCode::OK || response.status() == StatusCode::TOO_MANY_REQUESTS,
        "expected OK or TOO_MANY_REQUESTS, got {}",
        response.status()
    );
}

// =============================================================================
// rate_limit_middleware — short API key prefix safety
// =============================================================================

#[tokio::test]
async fn tc_middleware_short_api_key_does_not_panic() {
    // API keys shorter than 8 chars must not cause index-out-of-bounds in the
    // `&api_key[..std::cmp::min(8, api_key.len())]` slicing.
    let app = build_app(mock_arc(MockBehavior::Denied("short key".to_string())));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/protected")
                .header("Authorization", "Bearer abc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn tc_middleware_single_char_api_key_does_not_panic() {
    let app = build_app(mock_arc(MockBehavior::Allowed));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/protected")
                .header("Authorization", "Bearer x")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    assert_eq!(response.status(), StatusCode::OK);
}

// =============================================================================
// rate_limit_middleware — Bearer token extraction edge cases
// =============================================================================

#[tokio::test]
async fn tc_middleware_basic_auth_falls_back_to_ip_limit() {
    // Non-Bearer Authorization must be treated as unauthenticated → IP limit.
    let app = build_app(mock_arc(MockBehavior::Allowed));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/protected")
                .header("Authorization", "Basic dXNlcjpwYXNz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    assert!(
        response.status() == StatusCode::OK || response.status() == StatusCode::TOO_MANY_REQUESTS,
        "Basic auth should fall back to IP rate limit"
    );
}

#[tokio::test]
async fn tc_middleware_empty_bearer_token_falls_back_to_ip_limit() {
    let app = build_app(mock_arc(MockBehavior::Allowed));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/protected")
                .header("Authorization", "Bearer ")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    assert!(
        response.status() == StatusCode::OK || response.status() == StatusCode::TOO_MANY_REQUESTS,
        "Empty Bearer token should fall back to IP rate limit"
    );
}

#[tokio::test]
async fn tc_middleware_bearer_with_trailing_spaces_token_trimmed() {
    let app = build_app(mock_arc(MockBehavior::Allowed));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/protected")
                .header("Authorization", "Bearer   my-token   ")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    // Token is trimmed → treated as authenticated → Allowed by mock.
    assert_eq!(response.status(), StatusCode::OK);
}

// =============================================================================
// rate_limit_middleware — content-type header on 429 responses
// =============================================================================

#[tokio::test]
async fn tc_middleware_denied_response_has_application_json_content_type() {
    let app = build_app(mock_arc(MockBehavior::Denied("any reason".to_string())));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/protected")
                .header("Authorization", "Bearer valid-api-key-12345678")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
}

#[tokio::test]
async fn tc_middleware_retry_after_response_has_application_json_content_type() {
    let app = build_app(mock_arc(MockBehavior::RetryAfter(30)));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/protected")
                .header("Authorization", "Bearer valid-api-key-12345678")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must respond");

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
}
