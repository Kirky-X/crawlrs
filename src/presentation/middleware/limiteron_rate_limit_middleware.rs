// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Limiteron 速率限制中间件
//!
//! 直接使用 limiteron::Governor 进行速率限制检查

use std::net::SocketAddr;
use std::sync::Arc;

use ahash::AHashMap;
use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::IntoResponse,
};
use limiteron::prelude::{Decision, Governor, RequestContext};
use log::{debug, error, warn};

use crate::infrastructure::security::secure_ip::{SecureIpExtractor, TrustedProxyConfig};
use crate::presentation::middleware::RATE_LIMIT_EXCLUDED_ENDPOINTS;

/// Limiteron 速率限制中间件状态
#[derive(Clone)]
pub struct LimiteronMiddlewareState {
    /// Limiteron Governor 实例
    pub governor: Arc<Governor>,
}

/// 提取客户端 IP 地址
///
/// # 安全说明
///
/// 此函数使用安全的 IP 提取逻辑来防止 X-Forwarded-For 头伪造攻击。
/// 只有当请求来自可信代理时，才信任转发头中的 IP 地址。
fn extract_client_ip(request: &Request, remote_addr: Option<SocketAddr>) -> Option<String> {
    let extractor = SecureIpExtractor::new(TrustedProxyConfig::default());
    let direct_ip = remote_addr.map(|addr| addr.ip());
    extractor.extract_client_ip_with_override(request, direct_ip)
}

/// Limiteron 分布式速率限制中间件
///
/// 直接使用 limiteron::Governor 进行分布式速率限制检查
///
/// # 参数
///
/// * `State(governor)` - Limiteron Governor 状态
/// * `request` - HTTP请求
/// * `next` - 下一个中间件
///
/// # 返回值
///
/// * `Ok(impl IntoResponse)` - 处理成功的响应
/// * `Err(StatusCode)` - 处理失败的状态码
pub async fn limiteron_rate_limit_middleware(
    State(state): State<LimiteronMiddlewareState>,
    request: Request,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    let path = request.uri().path();
    debug!("LimiteronMiddleware: Path = {}", path);

    // Allow public endpoints (no rate limiting for these)
    if RATE_LIMIT_EXCLUDED_ENDPOINTS
        .iter()
        .any(|&endpoint| path == endpoint || path.starts_with(endpoint))
    {
        debug!("LimiteronMiddleware: Skipping public endpoint {}", path);
        return Ok(next.run(request).await);
    }

    debug!("LimiteronMiddleware: Building request context");

    // 尝试获取 API key
    let api_key = if let Some(token_str) = request.extensions().get::<String>().cloned() {
        debug!(
            "LimiteronMiddleware: Found API key: {}...",
            &token_str[..std::cmp::min(8, token_str.len())]
        );
        Some(token_str)
    } else {
        None
    };

    // 尝试获取用户 ID
    let user_id = request
        .extensions()
        .get::<crate::presentation::middleware::auth_middleware::AuthState>()
        .map(|state| state.api_key_id.to_string());

    // 提取客户端 IP（尝试从 ConnectInfo 获取）
    let remote_addr: Option<SocketAddr> = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|conn| conn.0);

    let client_ip = extract_client_ip(&request, remote_addr);

    // 构建请求头映射
    let mut headers: AHashMap<String, String> = AHashMap::new();
    if let Some(ref api_key) = api_key {
        headers.insert("x-api-key".to_string(), api_key.clone());
    }
    if let Some(ref user_id) = user_id {
        headers.insert("x-user-id".to_string(), user_id.clone());
    }

    // 构建请求上下文
    let context = RequestContext {
        ip: client_ip.clone(),
        user_id,
        api_key,
        path: path.to_string(),
        method: request.method().to_string(),
        headers,
        query_params: AHashMap::new(),
        client_ip,
        mac: None,
        device_id: None,
    };

    debug!(
        "LimiteronMiddleware: Checking rate limit for path: {}, method: {}",
        path,
        request.method()
    );

    // 使用 Governor 检查限流
    match state.governor.check(&context).await {
        Ok(Decision::Allowed(_)) => {
            debug!("LimiteronMiddleware: Rate limit check passed");
            Ok(next.run(request).await)
        }
        Ok(Decision::Rejected(reason)) => {
            warn!(
                "LimiteronMiddleware: Rate limit exceeded for path {}: {:?}",
                path, reason
            );
            Ok((
                StatusCode::TOO_MANY_REQUESTS,
                format!("Rate limit exceeded: {}", reason.reason),
            )
                .into_response())
        }
        Ok(Decision::Banned(ban_info)) => {
            warn!(
                "LimiteronMiddleware: Request banned for path {}: {}",
                path,
                ban_info.reason()
            );
            Ok((
                StatusCode::FORBIDDEN,
                format!("Access forbidden: {}", ban_info.reason()),
            )
                .into_response())
        }
        Err(e) => {
            // SEC-003: 可配置的 fail-open/fail-closed 行为
            if RATE_LIMIT_FAIL_OPEN {
                // Fail-open: 允许请求通过，但记录严重警告
                error!("LimiteronMiddleware: SEC-003 Rate limiting service error - failing open. \
                     Consider setting RATE_LIMIT_FAIL_OPEN=false for stricter security. error={} path={:?}", e, path);
                Ok(next.run(request).await)
            } else {
                // Fail-closed: 拒绝请求以确保安全
                error!("LimiteronMiddleware: SEC-003 Rate limiting service error - failing closed error={} path={:?}", e, path);
                Ok((
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Rate limiting service temporarily unavailable".to_string(),
                )
                    .into_response())
            }
        }
    }
}

/// Limiteron rate limiting fail-open behavior
///
/// Controls what happens when the rate limiting service encounters an error.
/// Can be overridden via environment variable.
const RATE_LIMIT_FAIL_OPEN: bool = true;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn build_request() -> Request {
        Request::builder()
            .uri("/test")
            .body(Body::empty())
            .expect("body should build")
    }

    fn build_request_with_forwarded(forwarded_for: Option<&str>, real_ip: Option<&str>) -> Request {
        let mut builder = Request::builder().uri("/test");
        if let Some(xff) = forwarded_for {
            builder = builder.header("x-forwarded-for", xff);
        }
        if let Some(xri) = real_ip {
            builder = builder.header("x-real-ip", xri);
        }
        builder.body(Body::empty()).expect("body should build")
    }

    fn socket_addr(ip: &str, port: u16) -> SocketAddr {
        SocketAddr::new(
            IpAddr::V4(ip.parse::<Ipv4Addr>().expect("valid ipv4")),
            port,
        )
    }

    #[test]
    fn test_extract_client_ip_none_without_remote_addr() {
        // No remote_addr and no ConnectInfo extension → cannot determine IP
        let request = build_request();
        assert_eq!(extract_client_ip(&request, None), None);
    }

    #[test]
    fn test_extract_client_ip_uses_direct_public_ip() {
        // A public (non-trusted) remote address should be used directly
        let request = build_request();
        let remote = socket_addr("8.8.8.8", 443);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("8.8.8.8".to_string())
        );
    }

    #[test]
    fn test_extract_client_ip_ignores_forwarded_from_untrusted() {
        // Security: X-Forwarded-For must be ignored when the direct connection
        // is NOT from a trusted proxy.
        let request = build_request_with_forwarded(Some("203.0.113.5"), None);
        let remote = socket_addr("8.8.8.8", 443);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("8.8.8.8".to_string())
        );
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_falls_back_to_direct() {
        // 127.0.0.1 is a trusted proxy; with no forwarded headers it falls
        // back to the direct IP.
        let request = build_request();
        let remote = socket_addr("127.0.0.1", 8080);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("127.0.0.1".to_string())
        );
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_uses_x_forwarded_for() {
        // From a trusted proxy, X-Forwarded-For should be honored.
        let request = build_request_with_forwarded(Some("203.0.113.5"), None);
        let remote = socket_addr("127.0.0.1", 8080);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("203.0.113.5".to_string())
        );
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_uses_x_real_ip() {
        // From a trusted proxy, X-Real-IP should be honored as a fallback.
        let request = build_request_with_forwarded(None, Some("203.0.113.9"));
        let remote = socket_addr("127.0.0.1", 8080);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("203.0.113.9".to_string())
        );
    }

    #[test]
    fn test_extract_client_ip_x_forwarded_for_takes_first_ip() {
        // X-Forwarded-For: client, proxy1, proxy2 → first IP (client) wins.
        let request = build_request_with_forwarded(Some("203.0.113.5, 10.0.0.1, 10.0.0.2"), None);
        let remote = socket_addr("127.0.0.1", 8080);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("203.0.113.5".to_string())
        );
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_10_x() {
        // 10.x.x.x is a trusted proxy range by default.
        let request = build_request_with_forwarded(Some("198.51.100.7"), None);
        let remote = socket_addr("10.0.0.1", 8080);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("198.51.100.7".to_string())
        );
    }

    #[test]
    fn test_rate_limit_fail_open_constant() {
        // The fail-open default must be true (availability over strictness).
        assert!(RATE_LIMIT_FAIL_OPEN);
    }

    #[tokio::test]
    async fn test_limiteron_middleware_state_clone_preserves_governor() {
        // Clone must share the same Governor Arc.
        use limiteron::config::{Action, ActionConfig, GlobalConfig, LimiterConfig, Matcher, Rule};

        let storage: Arc<dyn limiteron::storage::Storage> =
            Arc::new(limiteron::storage::MemoryStorage::new());
        let ban_storage: Arc<dyn limiteron::storage::BanStorage> =
            Arc::new(limiteron::storage::MemoryBanStorage::new());

        let flow_config = limiteron::prelude::FlowControlConfig {
            version: "0.1.0".to_string(),
            global: GlobalConfig::default(),
            rules: vec![Rule {
                id: "test_clone_rule".to_string(),
                name: "Test Clone Rule".to_string(),
                priority: 100,
                matchers: vec![Matcher::User {
                    user_ids: vec!["*".to_string()],
                }],
                limiters: vec![LimiterConfig::TokenBucket {
                    capacity: 10,
                    refill_rate: 1,
                }],
                action: ActionConfig {
                    on_exceed: Action::Reject,
                    ban: None,
                },
            }],
        };

        let governor = Governor::builder()
            .with_config(flow_config)
            .with_storage(storage)
            .with_ban_storage(ban_storage)
            .with_l1_cache_enabled(false)
            .build()
            .await
            .expect("governor should build");
        let governor = Arc::new(governor);
        let state = LimiteronMiddlewareState {
            governor: governor.clone(),
        };
        let cloned = state.clone();
        assert!(Arc::ptr_eq(&state.governor, &cloned.governor));
    }

    // =========================================================================
    // Handler-level tests (migrated from integration tests)
    // =========================================================================

    use axum::{middleware, response::Response, routing::get, Router};
    use limiteron::ban::BanSource;
    use limiteron::config::{
        Action, ActionConfig, BanConfig, BanScope, GlobalConfig, LimiterConfig, Matcher, Rule,
    };
    use limiteron::prelude::{FlowControlConfig, Identifier};
    use limiteron::storage::{BanStorage, MemoryBanStorage, MemoryStorage, Storage};
    use tower::ServiceExt;

    /// Build a Governor with generous capacity (used for "allow" scenarios).
    async fn create_test_governor() -> Arc<Governor> {
        create_governor_with_capacity(100, 10, 50, 5).await
    }

    /// Build a Governor whose IP rule has the given (capacity, refill_rate).
    async fn create_governor_with_ip_capacity(
        ip_capacity: u64,
        ip_refill_rate: u64,
    ) -> Arc<Governor> {
        create_governor_with_capacity(10_000, 10_000, ip_capacity, ip_refill_rate).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_governor_with_capacity(
        user_capacity: u64,
        user_refill_rate: u64,
        ip_capacity: u64,
        ip_refill_rate: u64,
    ) -> Arc<Governor> {
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

    /// Build a Governor with a rule that bans any request matching 0.0.0.0/0.
    async fn create_banning_governor() -> Arc<Governor> {
        let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::new());
        let ban_storage: Arc<dyn BanStorage> = Arc::new(MemoryBanStorage::new());

        let flow_config = FlowControlConfig {
            version: "0.1.0".to_string(),
            global: GlobalConfig::default(),
            rules: vec![Rule {
                id: "test_ban_rule".to_string(),
                name: "Test Ban Rule".to_string(),
                priority: 100,
                matchers: vec![Matcher::Ip {
                    ip_ranges: vec!["0.0.0.0/0".to_string()],
                }],
                limiters: vec![LimiterConfig::TokenBucket {
                    capacity: 1,
                    refill_rate: 1,
                }],
                action: ActionConfig {
                    on_exceed: Action::Reject,
                    ban: Some(BanConfig {
                        threshold: 1,
                        initial_duration: "60s".to_string(),
                        backoff_multiplier: 1.0,
                        max_duration: "3600s".to_string(),
                        scope: BanScope::Ip,
                    }),
                },
            }],
        };

        let governor = Governor::builder()
            .with_config(flow_config)
            .with_storage(storage)
            .with_ban_storage(ban_storage)
            .with_l1_cache_enabled(false)
            .build()
            .await
            .expect("Failed to build banning governor for tests");

        Arc::new(governor)
    }

    /// Build a request to `uri`, optionally attaching a `ConnectInfo<SocketAddr>`
    /// extension and/or an API-key `String` extension.
    fn build_handler_request(
        uri: &str,
        connect_info: Option<SocketAddr>,
        api_key_ext: Option<&str>,
    ) -> Request {
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

    /// Create a test DbPool for AuthState construction (lazy pool, no real DB).
    fn create_test_db_pool() -> Arc<dbnexus::DbPool> {
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool");
                let _guard = rt.enter();
                rt.block_on(dbnexus::DbPool::with_config({
                    let mut cfg = dbnexus::DbConfig::default();
                    cfg.url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
                        "postgres://crawlrs:password@localhost:5443/crawlrs_test".to_string()
                    });
                    cfg
                }))
                .expect("failed to create DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool thread panicked"))
        })
    }

    #[tokio::test]
    async fn test_public_endpoints_bypass_rate_limiting() {
        let governor = create_test_governor().await;
        let state = LimiteronMiddlewareState { governor };

        let app = Router::new()
            .route("/health", get(|| async { "OK" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                limiteron_rate_limit_middleware,
            ));

        let response = app
            .oneshot(build_handler_request("/health", None, None))
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_public_endpoint_prefix_bypass_rate_limiting() {
        let governor = create_test_governor().await;
        let state = LimiteronMiddlewareState { governor };

        let app = Router::new()
            .route("/v1/extract", get(|| async { "extracted" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                limiteron_rate_limit_middleware,
            ));

        let response = app
            .oneshot(build_handler_request("/v1/extract", None, None))
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_protected_request_passes_through_when_allowed() {
        let governor = create_test_governor().await;
        let state = LimiteronMiddlewareState { governor };

        let app = Router::new()
            .route("/protected", get(|| async { "protected-content" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                limiteron_rate_limit_middleware,
            ));

        let response = app
            .oneshot(build_handler_request("/protected", None, None))
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_rate_limit_exceeded_returns_429() {
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
                .oneshot(build_handler_request("/protected", Some(addr), None))
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
                .oneshot(build_handler_request("/protected", Some(addr), None))
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

    #[tokio::test]
    async fn test_api_key_extension_is_read_into_context() {
        let governor = create_test_governor().await;
        let state = LimiteronMiddlewareState { governor };

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                limiteron_rate_limit_middleware,
            ));

        let response = app
            .oneshot(build_handler_request(
                "/protected",
                None,
                Some("test-api-key-123"),
            ))
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_connect_info_provides_client_ip() {
        let governor = create_test_governor().await;
        let state = LimiteronMiddlewareState { governor };

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                limiteron_rate_limit_middleware,
            ));

        let response = app
            .oneshot(build_handler_request(
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
        req.extensions_mut().insert("test-api-key-123".to_string());

        let response = app.oneshot(req).await.expect("Failed to get response");

        assert!(
            response.status() == StatusCode::OK
                || response.status() == StatusCode::TOO_MANY_REQUESTS,
            "Expected OK or TOO_MANY_REQUESTS, got {}",
            response.status()
        );
    }

    #[tokio::test]
    async fn test_governor_check_allows_first_request() {
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

    #[tokio::test]
    async fn tc_banned_request_returns_403_forbidden() {
        let governor = create_banning_governor().await;
        governor
            .ban_identifier(
                &Identifier::Ip("203.0.113.99".to_string()),
                "automated ban for test",
                Some(BanSource::Manual {
                    operator: "test".to_string(),
                }),
            )
            .await
            .expect("Failed to manually ban identifier");

        let state = LimiteronMiddlewareState { governor };

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                limiteron_rate_limit_middleware,
            ));

        let addr = socket_addr("203.0.113.99", 443);
        let response = app
            .oneshot(build_handler_request("/protected", Some(addr), None))
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("Failed to read response body");
        let body = String::from_utf8(bytes.to_vec()).expect("body must be valid UTF-8");
        assert!(
            body.contains("Access forbidden"),
            "403 body should contain 'Access forbidden', got: {}",
            body
        );
    }

    #[tokio::test]
    async fn tc_banned_response_body_contains_ban_reason() {
        let governor = create_banning_governor().await;
        governor
            .ban_identifier(
                &Identifier::Ip("198.51.100.42".to_string()),
                "rate limit threshold exceeded",
                Some(BanSource::Manual {
                    operator: "test".to_string(),
                }),
            )
            .await
            .expect("Failed to manually ban identifier");

        let state = LimiteronMiddlewareState { governor };

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                limiteron_rate_limit_middleware,
            ));

        let addr = socket_addr("198.51.100.42", 80);
        let response = app
            .oneshot(build_handler_request("/protected", Some(addr), None))
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("Failed to read response body");
        let body = String::from_utf8(bytes.to_vec()).expect("body must be valid UTF-8");
        assert!(
            body.starts_with("Access forbidden:"),
            "403 body should start with 'Access forbidden:', got: {}",
            body
        );
        assert!(
            body.len() > "Access forbidden:".len(),
            "403 body should include a non-empty ban reason after the prefix, got: {}",
            body
        );
    }

    #[tokio::test]
    async fn tc_auth_state_extension_provides_user_id() {
        use crate::domain::auth::ApiKeyScope;
        use crate::presentation::middleware::auth_middleware::AuthState;
        use uuid::Uuid;

        let governor = create_test_governor().await;
        let state = LimiteronMiddlewareState { governor };

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                limiteron_rate_limit_middleware,
            ));

        let pool = create_test_db_pool();
        let auth_state =
            AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());

        let mut req = Request::builder()
            .uri("/protected")
            .body(Body::empty())
            .expect("Failed to build request");
        req.extensions_mut().insert(auth_state);

        let response = app.oneshot(req).await.expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn tc_auth_state_and_api_key_both_present_still_allowed() {
        use crate::domain::auth::ApiKeyScope;
        use crate::presentation::middleware::auth_middleware::AuthState;
        use uuid::Uuid;

        let governor = create_test_governor().await;
        let state = LimiteronMiddlewareState { governor };

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                limiteron_rate_limit_middleware,
            ));

        let pool = create_test_db_pool();
        let auth_state =
            AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());

        let mut req = Request::builder()
            .uri("/protected")
            .body(Body::empty())
            .expect("Failed to build request");
        req.extensions_mut().insert(auth_state);
        req.extensions_mut().insert("test-api-key-456".to_string());

        let response = app.oneshot(req).await.expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn tc_api_key_alone_populates_headers_without_user_id() {
        let governor = create_test_governor().await;
        let state = LimiteronMiddlewareState { governor };

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                limiteron_rate_limit_middleware,
            ));

        let response = app
            .oneshot(build_handler_request(
                "/protected",
                None,
                Some("solo-api-key-789"),
            ))
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
    }
}
