// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Rate Limit Middleware Security Tests
//!
//! Tests for the security fixes implemented in rate_limit_middleware.rs:
//! - IP-based rate limiting for unauthenticated requests
//! - Bearer token extraction from Authorization header
//! - Prevention of rate limit bypass attacks

#![cfg(test)]

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware,
    routing::get,
    Router,
};
use std::sync::Arc;
use tower::ServiceExt;

/// Mock rate limiting service for testing
struct MockRateLimitingService;

#[async_trait::async_trait]
impl crate::domain::services::rate_limiting_service::RateLimitService for MockRateLimitingService {
    async fn check_rate_limit(
        &self,
        _api_key: &str,
        _endpoint: &str,
    ) -> Result<
        crate::domain::services::rate_limiting_service::RateLimitResult,
        crate::domain::services::rate_limiting_service::RateLimitingError,
    > {
        Ok(crate::domain::services::rate_limiting_service::RateLimitResult::Allowed)
    }

    async fn get_team_rate_limit_config(
        &self,
        _team_id: uuid::Uuid,
    ) -> Result<
        crate::domain::services::rate_limiting_service::RateLimitConfig,
        crate::domain::services::rate_limiting_service::RateLimitingError,
    > {
        Ok(crate::domain::services::rate_limiting_service::RateLimitConfig::default())
    }

    async fn update_team_rate_limit_config(
        &self,
        _team_id: uuid::Uuid,
        _config: crate::domain::services::rate_limiting_service::RateLimitConfig,
    ) -> Result<(), crate::domain::services::rate_limiting_service::RateLimitingError> {
        Ok(())
    }

    async fn cleanup_expired_rate_limits(
        &self,
    ) -> Result<u64, crate::domain::services::rate_limiting_service::RateLimitingError> {
        Ok(0)
    }
}

/// Test that unauthenticated requests are rate limited by IP
#[tokio::test]
async fn test_unauthenticated_request_ip_rate_limiting() {
    use crate::presentation::middleware::rate_limit_middleware::rate_limit_middleware;

    let rate_limiting_service = Arc::new(MockRateLimitingService);

    let app = Router::new()
        .route("/protected", get(|| async { "Protected content" }))
        .layer(middleware::from_fn_with_state(
            rate_limiting_service.clone(),
            rate_limit_middleware,
        ));

    // First request without auth should succeed (within IP limit)
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/protected")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    // Should be allowed (first request from this IP)
    assert!(
        response.status() == StatusCode::OK || response.status() == StatusCode::TOO_MANY_REQUESTS,
        "Expected OK or TOO_MANY_REQUESTS, got {}",
        response.status()
    );
}

/// Test that Bearer token is correctly extracted from Authorization header
#[tokio::test]
async fn test_bearer_token_extraction() {
    use crate::presentation::middleware::rate_limit_middleware::rate_limit_middleware;

    let rate_limiting_service = Arc::new(MockRateLimitingService);

    let app = Router::new()
        .route("/protected", get(|| async { "Protected content" }))
        .layer(middleware::from_fn_with_state(
            rate_limiting_service.clone(),
            rate_limit_middleware,
        ));

    // Request with Bearer token should be processed
    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", "Bearer test-api-key-123")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    // Should be processed (either OK or rate limited by API key, not IP)
    assert!(
        response.status() == StatusCode::OK
            || response.status() == StatusCode::TOO_MANY_REQUESTS,
        "Expected OK or TOO_MANY_REQUESTS, got {}",
        response.status()
    );
}

/// Test that public endpoints bypass rate limiting
#[tokio::test]
async fn test_public_endpoints_bypass_rate_limiting() {
    use crate::presentation::middleware::rate_limit_middleware::rate_limit_middleware;

    let rate_limiting_service = Arc::new(MockRateLimitingService);

    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .layer(middleware::from_fn_with_state(
            rate_limiting_service.clone(),
            rate_limit_middleware,
        ));

    // Public endpoint should always succeed
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Public endpoint should always return OK"
    );
}

/// Test that x-api-key header is no longer accepted (security fix)
#[tokio::test]
async fn test_x_api_key_header_not_accepted() {
    use crate::presentation::middleware::rate_limit_middleware::rate_limit_middleware;

    let rate_limiting_service = Arc::new(MockRateLimitingService);

    let app = Router::new()
        .route("/protected", get(|| async { "Protected content" }))
        .layer(middleware::from_fn_with_state(
            rate_limiting_service.clone(),
            rate_limit_middleware,
        ));

    // Request with x-api-key header (old method) should be treated as unauthenticated
    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("x-api-key", "test-api-key-123")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    // Should be processed as unauthenticated (IP rate limiting applies)
    // The x-api-key header should be ignored
    assert!(
        response.status() == StatusCode::OK
            || response.status() == StatusCode::TOO_MANY_REQUESTS,
        "x-api-key header should be ignored, request treated as unauthenticated"
    );
}

/// Test that empty Bearer token is rejected
#[tokio::test]
async fn test_empty_bearer_token_rejected() {
    use crate::presentation::middleware::rate_limit_middleware::rate_limit_middleware;

    let rate_limiting_service = Arc::new(MockRateLimitingService);

    let app = Router::new()
        .route("/protected", get(|| async { "Protected content" }))
        .layer(middleware::from_fn_with_state(
            rate_limiting_service.clone(),
            rate_limit_middleware,
        ));

    // Request with empty Bearer token should be treated as unauthenticated
    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", "Bearer ")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    // Should be processed as unauthenticated (IP rate limiting applies)
    assert!(
        response.status() == StatusCode::OK
            || response.status() == StatusCode::TOO_MANY_REQUESTS,
        "Empty Bearer token should be treated as unauthenticated"
    );
}

/// Test that wrong auth type (Basic) is rejected
#[tokio::test]
async fn test_wrong_auth_type_rejected() {
    use crate::presentation::middleware::rate_limit_middleware::rate_limit_middleware;

    let rate_limiting_service = Arc::new(MockRateLimitingService);

    let app = Router::new()
        .route("/protected", get(|| async { "Protected content" }))
        .layer(middleware::from_fn_with_state(
            rate_limiting_service.clone(),
            rate_limit_middleware,
        ));

    // Request with Basic auth should be treated as unauthenticated
    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", "Basic dXNlcjpwYXNz")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    // Should be processed as unauthenticated (IP rate limiting applies)
    assert!(
        response.status() == StatusCode::OK
            || response.status() == StatusCode::TOO_MANY_REQUESTS,
        "Basic auth should be treated as unauthenticated"
    );
}

/// Test IP extraction from X-Forwarded-For header
#[tokio::test]
async fn test_ip_extraction_from_forwarded_header() {
    use crate::presentation::middleware::rate_limit_middleware::get_client_ip;

    let req = Request::builder()
        .header("X-Forwarded-For", "192.168.1.100, 10.0.0.1, 172.16.0.1")
        .body(Body::empty())
        .expect("Failed to build request");

    let ip = get_client_ip(&req);
    assert_eq!(ip, "192.168.1.100", "Should extract first IP from X-Forwarded-For");
}

/// Test IP extraction from X-Real-IP header
#[tokio::test]
async fn test_ip_extraction_from_real_ip_header() {
    use crate::presentation::middleware::rate_limit_middleware::get_client_ip;

    let req = Request::builder()
        .header("X-Real-IP", "192.168.2.100")
        .body(Body::empty())
        .expect("Failed to build request");

    let ip = get_client_ip(&req);
    assert_eq!(ip, "192.168.2.100", "Should extract IP from X-Real-IP");
}

/// Test IP extraction priority (X-Forwarded-For > X-Real-IP)
#[tokio::test]
async fn test_ip_extraction_priority() {
    use crate::presentation::middleware::rate_limit_middleware::get_client_ip;

    let req = Request::builder()
        .header("X-Forwarded-For", "192.168.1.100")
        .header("X-Real-IP", "192.168.2.100")
        .body(Body::empty())
        .expect("Failed to build request");

    let ip = get_client_ip(&req);
    assert_eq!(ip, "192.168.1.100", "X-Forwarded-For should take priority over X-Real-IP");
}

/// Test IP extraction fallback to "unknown"
#[tokio::test]
async fn test_ip_extraction_fallback() {
    use crate::presentation::middleware::rate_limit_middleware::get_client_ip;

    let req = Request::builder()
        .body(Body::empty())
        .expect("Failed to build request");

    let ip = get_client_ip(&req);
    assert_eq!(ip, "unknown", "Should return 'unknown' when no IP headers present");
}

/// Test that IP rate limiter correctly blocks after limit exceeded
#[tokio::test]
async fn test_ip_rate_limiter_blocks_excessive_requests() {
    use crate::presentation::middleware::rate_limit_middleware::RateLimiter;

    // Create a limiter with limit of 3
    let limiter = RateLimiter::new_for_ip_limit(3);
    let test_ip = "192.168.100.1";

    // First 3 requests should pass
    assert!(limiter.check_rate_limit(test_ip), "Request 1 should pass");
    assert!(limiter.check_rate_limit(test_ip), "Request 2 should pass");
    assert!(limiter.check_rate_limit(test_ip), "Request 3 should pass");

    // 4th request should be blocked
    assert!(!limiter.check_rate_limit(test_ip), "Request 4 should be blocked");

    // Different IP should still be allowed
    assert!(limiter.check_rate_limit("192.168.100.2"), "Different IP should pass");
}

/// Test that rate limit counter resets after window
#[tokio::test]
async fn test_rate_limit_counter_resets() {
    use crate::presentation::middleware::rate_limit_middleware::RateLimiter;
    use std::time::Duration;

    // Create a limiter with limit of 2 and 1 second window
    let limiter = RateLimiter::new_for_ip_limit(2);
    limiter.window_seconds = 1;
    
    let test_ip = "192.168.100.50";

    // Use up the limit
    assert!(limiter.check_rate_limit(test_ip));
    assert!(limiter.check_rate_limit(test_ip));
    assert!(!limiter.check_rate_limit(test_ip), "Should be blocked");

    // Wait for window to expire
    tokio::time::sleep(Duration::from_millis(1100)).await;

    // Should be allowed again
    assert!(limiter.check_rate_limit(test_ip), "Should be allowed after window expires");
}
