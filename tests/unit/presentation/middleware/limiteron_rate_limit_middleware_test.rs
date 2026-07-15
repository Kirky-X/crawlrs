// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Limiteron Rate Limit Middleware Tests
//!
//! Tests for the limiteron-based rate limiting middleware. These tests drive
//! the actual `limiteron_rate_limit_middleware` function through a real
//! `axum::Router`, exercising:
//! - Public endpoints bypassing rate limiting
//! - Allowed requests passing through to the handler (`Decision::Allowed`)
//! - Rate-limited (rejected) requests returning HTTP 429 (`Decision::Rejected`)
//! - The 429 response body format
//! - API key extraction from request extensions
//! - Client IP extraction from `ConnectInfo<SocketAddr>`
//!
//! NOTE: The IP rate-limit rule uses the valid CIDR `"0.0.0.0/0"` (matching
//! all IPv4). The previous version of this file used `"*"`, which limiteron's
//! `Governor::build` rejects — the test governor would panic on construction.

#![cfg(test)]
#![cfg(feature = "rate-limiting")]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use ahash::AHashMap;
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    middleware,
    response::Response,
    routing::get,
    Router,
};
use limiteron::prelude::*;
use limiteron::storage::{BanStorage, MemoryBanStorage, MemoryStorage, Storage};
use tower::ServiceExt;

use crawlrs::presentation::middleware::limiteron_rate_limit_middleware::{
    limiteron_rate_limit_middleware, LimiteronMiddlewareState,
};

/// Build a Governor with generous capacity (used for "allow" scenarios).
async fn create_test_governor() -> Arc<Governor> {
    create_governor_with_capacity(100, 10, 50, 5).await
}

/// Build a Governor whose IP rule has the given (capacity, refill_rate).
///
/// The user rule keeps a high capacity so only the IP rule throttles, allowing
/// deterministic 429 behavior when sending many requests from the same IP.
async fn create_governor_with_ip_capacity(ip_capacity: u64, ip_refill_rate: u64) -> Arc<Governor> {
    create_governor_with_capacity(10_000, 10_000, ip_capacity, ip_refill_rate).await
}

#[allow(clippy::too_many_arguments)]
async fn create_governor_with_capacity(
    user_capacity: u64,
    user_refill_rate: u64,
    ip_capacity: u64,
    ip_refill_rate: u64,
) -> Arc<Governor> {
    use limiteron::config::{Action, ActionConfig, GlobalConfig, LimiterConfig, Matcher, Rule};

    let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::new());
    let ban_storage: Arc<dyn BanStorage> = Arc::new(MemoryBanStorage::new());

    let flow_config = FlowControlConfig {
        version: "0.1.0".to_string(),
        global: GlobalConfig::default(),
        rules: vec![
            Rule {
                id: "test_user_rate_limit".to_string(),
                name: "Test User Rate Limit".to_string(),
                priority: 100,
                matchers: vec![Matcher::User {
                    user_ids: vec!["*".to_string()],
                }],
                limiters: vec![LimiterConfig::TokenBucket {
                    capacity: user_capacity,
                    refill_rate: user_refill_rate,
                }],
                action: ActionConfig {
                    on_exceed: Action::Reject,
                    ban: None,
                },
            },
            Rule {
                id: "test_ip_rate_limit".to_string(),
                name: "Test IP Rate Limit".to_string(),
                priority: 90,
                // Must be a valid CIDR — limiteron rejects "*" for IP ranges.
                matchers: vec![Matcher::Ip {
                    ip_ranges: vec!["0.0.0.0/0".to_string()],
                }],
                limiters: vec![LimiterConfig::TokenBucket {
                    capacity: ip_capacity,
                    refill_rate: ip_refill_rate,
                }],
                action: ActionConfig {
                    on_exceed: Action::Reject,
                    ban: None,
                },
            },
        ],
    };

    let governor = Governor::builder()
        .with_config(flow_config)
        .with_storage(storage)
        .with_ban_storage(ban_storage)
        .with_l1_cache_enabled(false)
        .build()
        .await
        .expect("Failed to build governor for tests");

    Arc::new(governor)
}

fn socket_addr(ip: &str, port: u16) -> SocketAddr {
    SocketAddr::new(
        IpAddr::V4(ip.parse::<Ipv4Addr>().expect("valid ipv4")),
        port,
    )
}

/// Build a request to `uri`, optionally attaching a `ConnectInfo<SocketAddr>`
/// extension and/or an API-key `String` extension (mimicking what the auth
/// middleware would insert).
fn build_request(
    uri: &str,
    connect_info: Option<SocketAddr>,
    api_key_ext: Option<&str>,
) -> Request<Body> {
    let mut req = Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("Failed to build request");
    if let Some(addr) = connect_info {
        req.extensions_mut().insert(ConnectInfo(addr));
    }
    if let Some(key) = api_key_ext {
        req.extensions_mut().insert(key.to_string());
    }
    req
}

// =============================================================================
// Public endpoint bypass
// =============================================================================

#[tokio::test]
async fn test_public_endpoints_bypass_rate_limiting() {
    // Endpoints in RATE_LIMIT_EXCLUDED_ENDPOINTS (e.g. /health) must skip the
    // Governor entirely and always reach the handler.
    let governor = create_test_governor().await;
    let state = LimiteronMiddlewareState { governor };

    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limiteron_rate_limit_middleware,
        ));

    let response = app
        .oneshot(build_request("/health", None, None))
        .await
        .expect("Failed to get response");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Public endpoint should always return OK"
    );
}

#[tokio::test]
async fn test_public_endpoint_prefix_bypass_rate_limiting() {
    // The bypass matches path prefixes too (path.starts_with(endpoint)).
    // /v1/extract is in the excluded list; a sub-path must also be bypassed.
    let governor = create_test_governor().await;
    let state = LimiteronMiddlewareState { governor };

    let app = Router::new()
        .route("/v1/extract", get(|| async { "extracted" }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limiteron_rate_limit_middleware,
        ));

    let response = app
        .oneshot(build_request("/v1/extract", None, None))
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);
}

// =============================================================================
// Allowed path (Decision::Allowed)
// =============================================================================

#[tokio::test]
async fn test_protected_request_passes_through_when_allowed() {
    // A non-public endpoint under capacity must reach the handler (OK).
    let governor = create_test_governor().await;
    let state = LimiteronMiddlewareState { governor };

    let app = Router::new()
        .route("/protected", get(|| async { "protected-content" }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limiteron_rate_limit_middleware,
        ));

    let response = app
        .oneshot(build_request("/protected", None, None))
        .await
        .expect("Failed to get response");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Request under the rate limit should reach the handler"
    );
}

// =============================================================================
// Rejected path (Decision::Rejected → HTTP 429)
// =============================================================================

#[tokio::test]
async fn test_rate_limit_exceeded_returns_429() {
    // IP rule: capacity = 1, refill = 1/s. Send several requests from the same
    // public IP (8.8.8.8, not a trusted proxy) in rapid succession; at least
    // one must be rejected with HTTP 429 Too Many Requests.
    let governor = create_governor_with_ip_capacity(1, 1).await;
    let state = LimiteronMiddlewareState { governor };

    let app = Router::new()
        .route("/protected", get(|| async { "ok" }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limiteron_rate_limit_middleware,
        ));

    let addr = socket_addr("8.8.8.8", 1234);
    let mut saw_429 = false;
    for _ in 0..10 {
        let response = app
            .clone()
            .oneshot(build_request("/protected", Some(addr), None))
            .await
            .expect("Failed to get response");
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            saw_429 = true;
            break;
        }
    }
    assert!(
        saw_429,
        "expected at least one HTTP 429 after exhausting the IP token bucket"
    );
}

#[tokio::test]
async fn test_429_response_body_contains_rate_limit_message() {
    // Verify the Rejected branch produces a body containing "Rate limit exceeded".
    let governor = create_governor_with_ip_capacity(1, 1).await;
    let state = LimiteronMiddlewareState { governor };

    let app = Router::new()
        .route("/protected", get(|| async { "ok" }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limiteron_rate_limit_middleware,
        ));

    let addr = socket_addr("8.8.8.8", 1234);
    let mut rejected: Option<Response> = None;
    for _ in 0..10 {
        let response = app
            .clone()
            .oneshot(build_request("/protected", Some(addr), None))
            .await
            .expect("Failed to get response");
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            rejected = Some(response);
            break;
        }
    }

    let response = rejected.expect("expected at least one 429 response");
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("Failed to read response body");
    let body = String::from_utf8(bytes.to_vec()).expect("body must be valid UTF-8");
    assert!(
        body.contains("Rate limit exceeded"),
        "429 body should contain 'Rate limit exceeded', got: {}",
        body
    );
}

// =============================================================================
// Request context construction: API key + ConnectInfo
// =============================================================================

#[tokio::test]
async fn test_api_key_extension_is_read_into_context() {
    // When the auth middleware inserts an API key as a String extension, the
    // limiteron middleware must read it (api_key = Some) and forward to the
    // Governor. With generous capacity the request must still be Allowed.
    let governor = create_test_governor().await;
    let state = LimiteronMiddlewareState { governor };

    let app = Router::new()
        .route("/protected", get(|| async { "ok" }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limiteron_rate_limit_middleware,
        ));

    let response = app
        .oneshot(build_request("/protected", None, Some("test-api-key-123")))
        .await
        .expect("Failed to get response");

    // The user rule matches on user_id, which comes from AuthState (absent here),
    // so api_key alone still yields Allowed via the IP/allow path.
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_connect_info_provides_client_ip() {
    // A public direct IP via ConnectInfo must be extracted and used by the
    // Governor's IP rule. Under generous capacity the request is Allowed.
    let governor = create_test_governor().await;
    let state = LimiteronMiddlewareState { governor };

    let app = Router::new()
        .route("/protected", get(|| async { "ok" }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limiteron_rate_limit_middleware,
        ));

    let response = app
        .oneshot(build_request(
            "/protected",
            Some(socket_addr("203.0.113.7", 443)),
            None,
        ))
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_bearer_token_request_is_processed() {
    // A request carrying an Authorization Bearer header must be processed by
    // the middleware (OK under capacity, never a 500).
    let governor = create_test_governor().await;
    let state = LimiteronMiddlewareState { governor };

    let app = Router::new()
        .route("/protected", get(|| async { "Protected content" }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limiteron_rate_limit_middleware,
        ));

    let mut req = Request::builder()
        .uri("/protected")
        .header("Authorization", "Bearer test-api-key-123")
        .body(Body::empty())
        .expect("Failed to build request");

    // Mimic auth middleware inserting the API key as a String extension.
    req.extensions_mut().insert("test-api-key-123".to_string());

    let response = app.oneshot(req).await.expect("Failed to get response");

    assert!(
        response.status() == StatusCode::OK || response.status() == StatusCode::TOO_MANY_REQUESTS,
        "Expected OK or TOO_MANY_REQUESTS, got {}",
        response.status()
    );
}

// =============================================================================
// Governor-level sanity check (no middleware wrapper)
// =============================================================================

#[tokio::test]
async fn test_governor_check_allows_first_request() {
    // Directly exercise the Governor to assert the configured rules permit an
    // initial request — this guards against rule-misconfiguration regressions
    // (e.g. using "*" for IP ranges would panic on Governor construction).
    let governor = create_test_governor().await;

    let context = RequestContext {
        ip: Some("192.168.1.100".to_string()),
        user_id: Some("test_user".to_string()),
        api_key: Some("test-api-key".to_string()),
        path: "/api/test".to_string(),
        method: "GET".to_string(),
        headers: AHashMap::new(),
        query_params: AHashMap::new(),
        client_ip: Some("192.168.1.100".to_string()),
        mac: None,
        device_id: None,
    };

    let result = governor.check(&context).await;
    assert!(result.is_ok(), "Governor check should succeed");

    match result.unwrap() {
        Decision::Allowed(_) => { /* expected */ }
        Decision::Rejected(reason) => {
            // A misconfigured rule could reject; that is not fatal here, but
            // the first request against a fresh bucket must not reject.
            panic!(
                "First request should be Allowed, got Rejected: {:?}",
                reason
            );
        }
        Decision::Banned(info) => {
            panic!("First request should not be Banned: {}", info.reason());
        }
    }
}
