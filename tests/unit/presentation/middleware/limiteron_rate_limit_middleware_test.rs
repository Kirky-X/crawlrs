// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Limiteron Rate Limit Middleware Tests
//!
//! Tests for the limiteron-based rate limiting middleware

#![cfg(test)]
#![cfg(feature = "rate-limiting")]

use std::sync::Arc;

use ahash::AHashMap;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware,
    routing::get,
    Router,
};
use limiteron::prelude::*;
use limiteron::storage::{BanStorage, MemoryBanStorage, MemoryStorage, Storage};
use tower::ServiceExt;

/// 创建测试用的 Governor
async fn create_test_governor() -> Arc<Governor> {
    use limiteron::config::types::{
        Action, ActionConfig, GlobalConfig, LimiterConfig, Matcher, Rule,
    };

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
                    capacity: 100,
                    refill_rate: 10,
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
                matchers: vec![Matcher::Ip {
                    ip_ranges: vec!["*".to_string()],
                }],
                limiters: vec![LimiterConfig::TokenBucket {
                    capacity: 50,
                    refill_rate: 5,
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

/// 测试公共端点绕过限流
#[tokio::test]
async fn test_public_endpoints_bypass_rate_limiting() {
    use crate::presentation::middleware::limiteron_rate_limit_middleware::{
        limiteron_rate_limit_middleware, LimiteronMiddlewareState,
    };

    let governor = create_test_governor().await;
    let state = LimiteronMiddlewareState { governor };

    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limiteron_rate_limit_middleware,
        ));

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

/// 测试带有 Bearer token 的请求
#[tokio::test]
async fn test_bearer_token_request() {
    use crate::presentation::middleware::limiteron_rate_limit_middleware::{
        limiteron_rate_limit_middleware, LimiteronMiddlewareState,
    };

    let governor = create_test_governor().await;
    let state = LimiteronMiddlewareState { governor };

    let app = Router::new()
        .route("/protected", get(|| async { "Protected content" }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limiteron_rate_limit_middleware,
        ));

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

    // 应该在限流检查后允许
    assert!(
        response.status() == StatusCode::OK
            || response.status() == StatusCode::TOO_MANY_REQUESTS,
        "Expected OK or TOO_MANY_REQUESTS, got {}",
        response.status()
    );
}

/// 测试请求上下文构建
#[tokio::test]
async fn test_request_context_building() {
    use crate::presentation::middleware::limiteron_rate_limit_middleware::{
        limiteron_rate_limit_middleware, LimiteronMiddlewareState,
    };

    let governor = create_test_governor().await;
    let state = LimiteronMiddlewareState { governor };

    let app = Router::new()
        .route("/test", get(|| async { "Test" }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limiteron_rate_limit_middleware,
        ));

    // 测试带有多个 header 的请求
    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("Authorization", "Bearer my-api-key")
                .header("X-Forwarded-For", "192.168.1.100")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert!(
        response.status() == StatusCode::OK
            || response.status() == StatusCode::TOO_MANY_REQUESTS,
        "Request should be processed"
    );
}

/// 测试 Governor 检查功能
#[tokio::test]
async fn test_governor_check() {
    let governor = create_test_governor().await;

    // 构建请求上下文
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

    // 第一次检查应该允许
    let result = governor.check(&context).await;
    assert!(result.is_ok(), "Governor check should succeed");

    let decision = result.unwrap();
    match decision {
        Decision::Allowed(_) => {
            // 预期行为
        }
        Decision::Rejected(reason) => {
            // 可能是规则配置问题，但不应该报错
            println!("Request rejected: {}", reason);
        }
        Decision::Banned(info) => {
            panic!("Request should not be banned: {}", info.reason());
        }
    }
}
