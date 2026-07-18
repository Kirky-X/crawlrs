// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::services::rate_limiting_service::{RateLimitResult, RateLimitingService};
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::middleware::RATE_LIMIT_EXCLUDED_ENDPOINTS;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::IntoResponse,
};
use log::{debug, error, warn};
use std::sync::Arc;

/// 分布式速率限制中间件
///
/// 基于API密钥应用分布式速率限制
///
/// # 参数
///
/// * `rate_limiting_service` - 速率限制服务状态
/// * `request` - HTTP请求
/// * `next` - 下一个中间件
///
/// # 返回值
///
/// * `Ok(impl IntoResponse)` - 处理成功的响应
/// * `Err(StatusCode)` - 处理失败的状态码
pub async fn distributed_rate_limit_middleware(
    State(rate_limiting_service): State<Arc<dyn RateLimitingService>>,
    request: Request,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    let path = request.uri().path();
    debug!("DistributedRateLimitMiddleware: Path = {}", path);

    // Allow public endpoints (no rate limiting for these)
    if RATE_LIMIT_EXCLUDED_ENDPOINTS
        .iter()
        .any(|&endpoint| path == endpoint || path.starts_with(endpoint))
    {
        debug!(
            "DistributedRateLimitMiddleware: Skipping public endpoint {}",
            path
        );
        return Ok(next.run(request).await);
    }

    debug!("DistributedRateLimitMiddleware: Checking for API key in extensions");

    // Try to get API key from token_str first (set by auth middleware - this is the raw API key)
    // Fall back to using api_key_id from AuthState if token is not available
    let api_key = if let Some(token_str) = request.extensions().get::<String>().cloned() {
        debug!(
            "Found API key token: {}...",
            &token_str[..std::cmp::min(8, token_str.len())]
        );
        token_str // This is the raw API key from Authorization header
    } else if let Some(auth_state) = request.extensions().get::<AuthState>() {
        debug!("Using api_key_id from AuthState");
        auth_state.api_key_id.to_string() // This is the database ID
    } else {
        error!("DistributedRateLimitMiddleware: No API key found in request extensions.");
        return Err(StatusCode::UNAUTHORIZED);
    };

    debug!(
        "DistributedRateLimitMiddleware: Rate limiting check for API key: {}...",
        &api_key[..std::cmp::min(8, api_key.len())]
    );

    let api_key_prefix = &api_key[..std::cmp::min(8, api_key.len())];
    debug!(
        "DistributedRateLimitMiddleware: Checking rate limit for API Key starting with: {}",
        api_key_prefix
    );

    debug!("DistributedRateLimitMiddleware: Calling rate_limiting_service.check_rate_limit()");
    match rate_limiting_service.check_rate_limit(&api_key, path).await {
        Ok(RateLimitResult::Allowed) => {
            debug!("DistributedRateLimitMiddleware: Rate limit check passed");
            Ok(next.run(request).await)
        }
        Ok(RateLimitResult::Denied { reason }) => {
            warn!(
                "Rate limit exceeded for API Key starting with {}: {}",
                api_key_prefix, reason
            );
            Ok((
                StatusCode::TOO_MANY_REQUESTS,
                format!("Rate limit exceeded: {}", reason),
            )
                .into_response())
        }
        Ok(RateLimitResult::RetryAfter {
            retry_after_seconds,
        }) => {
            warn!(
                "Rate limit exceeded for API Key starting with {}: retry after {} seconds",
                api_key_prefix, retry_after_seconds
            );
            Ok((
                StatusCode::TOO_MANY_REQUESTS,
                format!(
                    "Rate limit exceeded, retry after {} seconds",
                    retry_after_seconds
                ),
            )
                .into_response())
        }
        Err(e) => {
            warn!(
                "Rate limit check failed for API Key starting with {}: {}",
                api_key_prefix, e
            );
            // Fail open - allow request if rate limiting service fails
            Ok(next.run(request).await)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::CreditsTransactionType;
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult,
        QuotaService, RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError,
    };
    use async_trait::async_trait;
    use axum::{body::Body, http::StatusCode, routing::get, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tower::ServiceExt;
    use uuid::Uuid;

    /// Mock `RateLimitingService` with configurable `check_rate_limit` result.
    ///
    /// `call_count` tracks how many times `check_rate_limit` was invoked,
    /// enabling tests to assert whether the middleware consulted the service.
    struct MockRateLimitingService {
        call_count: Arc<AtomicUsize>,
        result: RateLimitResult,
        /// When `true`, `check_rate_limit` returns `RateLimitingError::DatabaseError`.
        should_error: bool,
    }

    impl MockRateLimitingService {
        fn new(result: RateLimitResult) -> Self {
            Self {
                call_count: Arc::new(AtomicUsize::new(0)),
                result,
                should_error: false,
            }
        }

        fn with_error() -> Self {
            Self {
                call_count: Arc::new(AtomicUsize::new(0)),
                result: RateLimitResult::Allowed,
                should_error: true,
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
            if self.should_error {
                return Err(RateLimitingError::DatabaseError);
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
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_quota_balance(&self, _team_id: Uuid) -> Result<i64, RateLimitingError> {
            Ok(1000)
        }
    }

    #[async_trait]
    impl RateLimitingService for MockRateLimitingService {}

    fn build_request(path: &str) -> Request {
        Request::builder()
            .uri(path)
            .body(Body::empty())
            .expect("body should build")
    }

    fn build_request_with_token(path: &str, token: &str) -> Request {
        let mut req = Request::builder()
            .uri(path)
            .body(Body::empty())
            .expect("body should build");
        req.extensions_mut().insert(token.to_string());
        req
    }

    fn build_request_with_auth_state(path: &str) -> Request {
        use crate::domain::auth::ApiKeyScope;
        use dbnexus::{DbConfig, DbPool};

        // Construct a lazy (non-connecting) DbPool on a dedicated thread to
        // avoid runtime-in-runtime panics — see webhook_handler tests.
        let pool = std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool construction");
                let _guard = rt.enter();
                rt.block_on(DbPool::with_config({
                    let mut cfg = DbConfig::default();
                    cfg.url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
                        "postgres://crawlrs:password@localhost:5443/crawlrs_test".to_string()
                    });
                    cfg
                }))
                .expect("failed to create DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool construction thread panicked"))
        });

        let auth_state =
            AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());
        let mut req = Request::builder()
            .uri(path)
            .body(Body::empty())
            .expect("body should build");
        req.extensions_mut().insert(auth_state);
        req
    }

    /// Build a test Router wired with the given mock service.
    ///
    /// Routes:
    /// - `/test` — non-excluded endpoint (rate limit applies)
    /// - `/health` — excluded public endpoint (no rate limit)
    fn test_router(service: Arc<dyn RateLimitingService>) -> Router {
        Router::new()
            .route("/test", get(|| async { "ok" }))
            .route("/health", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                service,
                distributed_rate_limit_middleware,
            ))
    }

    #[tokio::test]
    async fn test_excluded_endpoint_skips_rate_limit() {
        let service = Arc::new(MockRateLimitingService::new(RateLimitResult::Denied {
            reason: "should not be reached".to_string(),
        }));
        let app = test_router(service.clone());
        let request = build_request("/health");
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            service.call_count.load(Ordering::SeqCst),
            0,
            "excluded endpoint should not invoke check_rate_limit"
        );
    }

    #[tokio::test]
    async fn test_no_api_key_returns_unauthorized() {
        let service = Arc::new(MockRateLimitingService::new(RateLimitResult::Allowed));
        let app = test_router(service.clone());
        let request = build_request("/test");
        // No token_str extension and no AuthState extension → 401
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            service.call_count.load(Ordering::SeqCst),
            0,
            "service should not be called when no API key is present"
        );
    }

    #[tokio::test]
    async fn test_allowed_passes_through() {
        let service = Arc::new(MockRateLimitingService::new(RateLimitResult::Allowed));
        let app = test_router(service.clone());
        let request = build_request_with_token("/test", "test-api-key");
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(service.call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_denied_returns_429() {
        let service = Arc::new(MockRateLimitingService::new(RateLimitResult::Denied {
            reason: "per-minute quota exhausted".to_string(),
        }));
        let app = test_router(service.clone());
        let request = build_request_with_token("/test", "test-api-key");
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(service.call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_after_returns_429() {
        let service = Arc::new(MockRateLimitingService::new(RateLimitResult::RetryAfter {
            retry_after_seconds: 90,
        }));
        let app = test_router(service.clone());
        let request = build_request_with_token("/test", "test-api-key");
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(service.call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_service_error_fails_open() {
        // Fail-open policy: when the rate limiting service errors, the request
        // is allowed to proceed.
        let service = Arc::new(MockRateLimitingService::with_error());
        let app = test_router(service.clone());
        let request = build_request_with_token("/test", "test-api-key");
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(service.call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_falls_back_to_auth_state_api_key_id() {
        // When no token_str extension is present but AuthState is, the middleware
        // should use api_key_id from AuthState as the API key.
        let service = Arc::new(MockRateLimitingService::new(RateLimitResult::Allowed));
        let app = test_router(service.clone());
        let request = build_request_with_auth_state("/test");
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            service.call_count.load(Ordering::SeqCst),
            1,
            "service should be called with api_key_id from AuthState"
        );
    }

    #[tokio::test]
    async fn test_token_str_takes_precedence_over_auth_state() {
        // When both token_str and AuthState are present, token_str wins.
        let service = Arc::new(MockRateLimitingService::new(RateLimitResult::Allowed));
        let app = test_router(service.clone());
        let mut request = build_request_with_auth_state("/test");
        request.extensions_mut().insert("raw-api-key".to_string());
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(service.call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_denied_response_body_contains_reason() {
        let service = Arc::new(MockRateLimitingService::new(RateLimitResult::Denied {
            reason: "specific-reason".to_string(),
        }));
        let app = test_router(service);
        let request = build_request_with_token("/test", "k");
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.contains("specific-reason"),
            "response body should contain denial reason: {}",
            body_str
        );
    }

    #[tokio::test]
    async fn test_retry_after_response_body_contains_seconds() {
        let service = Arc::new(MockRateLimitingService::new(RateLimitResult::RetryAfter {
            retry_after_seconds: 77,
        }));
        let app = test_router(service);
        let request = build_request_with_token("/test", "k");
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.contains("77"),
            "response body should contain retry seconds: {}",
            body_str
        );
    }

    #[tokio::test]
    async fn test_short_token_does_not_panic() {
        // The middleware slices `&api_key[..min(8, api_key.len())]`. A token
        // shorter than 8 chars must not cause a panic.
        let service = Arc::new(MockRateLimitingService::new(RateLimitResult::Allowed));
        let app = test_router(service.clone());
        let request = build_request_with_token("/test", "short");
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(service.call_count.load(Ordering::SeqCst), 1);
    }
}
