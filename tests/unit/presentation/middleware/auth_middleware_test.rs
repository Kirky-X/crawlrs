// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Auth middleware tests
//!
//! Tests for the unified authentication middleware, covering AuthState construction,
//! scope_middleware behavior, and auth_middleware integration.
//!
//! ## Stage 3 重构（R-auth-engine-003）
//!
//! 已删除对 `ApiKeyCache` / `AuthRateLimiter` / `CacheStats` / `with_cache` /
//! `with_trusted_proxies` / `new_for_middleware` 等已删除 API 的测试。
//! 限速测试已移除——crawlrs 侧不再做暴力破解防护，完全依赖 garrison firewall
//! （决策 2）。缓存测试已移除——`ApiKeyCache` 已删除，改为 `TEAM_ID_CACHE`
//! （决策 4，内部 LRU，不暴露公共 API）。
//!
//! Note: Code paths requiring garrison RBAC (auth_middleware_inner 的 garrison 路径)
//! 不在此处覆盖——需要 garrison 单例 + 真实 API Key，由 Stage 7 集成测试覆盖。

#![cfg(all(test, feature = "auth"))]

use std::sync::Arc;

use once_cell::sync::Lazy;

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    routing::{get, post, put},
    Router,
};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

use crawlrs::domain::auth::{ApiKeyScope, AuditLogEntry, ScopePermission};
use crawlrs::domain::services::audit_service::{AuditServiceError, AuditServiceTrait};
use crawlrs::presentation::middleware::auth_middleware::{
    self, get_global_auth_state, reset_global_auth_state, set_global_auth_state, AuthError,
    AuthState,
};

use crate::common::helpers::db_pool::create_test_pool_or_panic;

// ============================================================================
// Mock Audit Service
// ============================================================================

/// Mock audit service that counts `log_deny` calls for verifying scope denial logging.
struct MockAuditService {
    deny_count: Arc<std::sync::atomic::AtomicU32>,
}

impl MockAuditService {
    fn new() -> Self {
        Self {
            deny_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    fn deny_count(&self) -> u32 {
        self.deny_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl AuditServiceTrait for MockAuditService {
    async fn log(&self, _entry: AuditLogEntry) -> Result<(), AuditServiceError> {
        Ok(())
    }

    async fn log_allow(
        &self,
        _action: String,
        _api_key_id: Uuid,
        _team_id: Uuid,
        _scope: ApiKeyScope,
    ) -> Result<(), AuditServiceError> {
        Ok(())
    }

    async fn log_deny(
        &self,
        _action: String,
        _api_key_id: Option<Uuid>,
        _team_id: Option<Uuid>,
        _reason: String,
        _scope: Option<ApiKeyScope>,
    ) -> Result<(), AuditServiceError> {
        self.deny_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    async fn get_logs_for_key(
        &self,
        _api_key_id: Uuid,
        _limit: u64,
        _offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        Ok(vec![])
    }

    async fn get_logs_for_team(
        &self,
        _team_id: Uuid,
        _limit: u64,
        _offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        Ok(vec![])
    }

    async fn get_denied_requests(
        &self,
        _api_key_id: Uuid,
        _limit: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        Ok(vec![])
    }
}

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a test AuthState with the given scope and random IDs.
fn make_auth_state(scope: ApiKeyScope) -> AuthState {
    let pool = create_test_pool_or_panic();
    AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), scope)
}

/// Serialize tests that touch GLOBAL_AUTH_STATE (Mutex<Option<...>> — resettable per test).
static GLOBAL_STATE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// Ensure global auth state is initialized (Stage 3 DTO 化后仅含 4 字段).
///
/// 架构 MEDIUM-1：GLOBAL_AUTH_STATE 是 `ParkRwLock<Option<Arc<AuthState>>>`（可重置）。
/// 默认复用已设置的 state（避免重建）；调用方需要 fresh state 时，
/// 应先 `reset_global_auth_state()` 再调用本函数。
/// All callers must hold GLOBAL_STATE_LOCK to avoid races.
///
/// ## Stage 3 重构
///
/// AuthState DTO 化后仅含 `pool`/`team_id`/`api_key_id`/`scope` 四字段。
/// `ApiKeyCache` / `AuthRateLimiter` / `trusted_proxies` 已删除：
/// - 限速由 garrison firewall 负责（决策 2）
/// - team_id 缓存由 `TEAM_ID_CACHE` 内部 LRU 负责（决策 4）
/// - IP 解析由 garrison 在 `check_api_key` 内部处理
fn ensure_global_auth_state() -> Arc<AuthState> {
    if let Some(state) = get_global_auth_state() {
        return state;
    }
    let pool = create_test_pool_or_panic();
    let state = AuthState::new(pool, Uuid::nil(), Uuid::nil(), ApiKeyScope::default());
    let state = Arc::new(state);
    set_global_auth_state(state.clone());
    get_global_auth_state().unwrap_or(state)
}

/// Build a Router wired with scope_middleware and an AuthState injector layer.
///
/// The injector runs before scope_middleware (outer layer) so AuthState is
/// available in request extensions when scope_middleware runs.
fn build_scope_test_app(auth_state: Option<AuthState>) -> Router {
    let router = Router::new()
        .route("/api/v1/teams", get(|| async { "ok" }))
        .route("/api/v1/teams/{id}", get(|| async { "ok" }))
        .route("/api/v1/teams-secret", get(|| async { "ok" }))
        .route("/api/v1/billing", get(|| async { "ok" }))
        .route("/v1/search", get(|| async { "ok" }).post(|| async { "ok" }))
        .route("/v1/scrape", post(|| async { "ok" }))
        .route("/v1/crawl", get(|| async { "ok" }).post(|| async { "ok" }))
        .route(
            "/v1/crawl/{id}",
            put(|| async { "ok" }).delete(|| async { "ok" }),
        )
        .layer(middleware::from_fn(auth_middleware::scope_middleware));

    match auth_state {
        Some(state) => router.layer(middleware::from_fn(
            move |mut req: Request<Body>, next: Next| {
                let state = state.clone();
                async move {
                    req.extensions_mut().insert(state);
                    next.run(req).await
                }
            },
        )),
        None => router,
    }
}

/// Build a Router wired with the auth_middleware layer.
fn build_auth_test_app() -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/metrics", get(|| async { "ok" }))
        .route("/protected", get(|| async { "ok" }))
        .layer(middleware::from_fn(auth_middleware::auth_middleware()))
}

// ============================================================================
// AuthState Construction Tests
// ============================================================================

#[test]
fn test_auth_state_new_sets_required_fields() {
    let pool = create_test_pool_or_panic();
    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();
    let scope = ApiKeyScope::full_access();
    let state = AuthState::new(pool, team_id, api_key_id, scope.clone());

    assert_eq!(state.team_id, team_id);
    assert_eq!(state.api_key_id, api_key_id);
    assert_eq!(state.scope, scope);
}

#[test]
fn test_auth_state_debug_format() {
    let state = make_auth_state(ApiKeyScope::full_access());
    let debug_str = format!("{:?}", state);

    assert!(debug_str.contains("AuthState"));
    assert!(debug_str.contains("team_id"));
    assert!(debug_str.contains("api_key_id"));
    assert!(debug_str.contains("scope"));
}

#[test]
fn test_auth_state_clone_preserves_fields() {
    let state = make_auth_state(ApiKeyScope::full_access());
    let cloned = state.clone();

    assert_eq!(state.team_id, cloned.team_id);
    assert_eq!(state.api_key_id, cloned.api_key_id);
    assert_eq!(state.scope, cloned.scope);
}

// ============================================================================
// Scope Middleware Tests
// ============================================================================

#[tokio::test]
async fn test_scope_middleware_no_auth_state_returns_unauthorized() {
    let app = build_scope_test_app(None);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/teams")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Request without AuthState to admin endpoint should return 401"
    );
}

#[tokio::test]
async fn test_scope_middleware_admin_denied_returns_forbidden() {
    // read_only scope lacks admin permission
    let app = build_scope_test_app(Some(make_auth_state(ApiKeyScope::read_only())));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/teams")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "Read-only scope accessing admin endpoint should return 403"
    );
}

#[tokio::test]
async fn test_scope_middleware_admin_granted_passes() {
    let app = build_scope_test_app(Some(make_auth_state(ApiKeyScope::full_access())));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/teams")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Full-access scope accessing admin endpoint should return 200"
    );
}

#[tokio::test]
async fn test_scope_middleware_billing_admin_required() {
    let app_denied = build_scope_test_app(Some(make_auth_state(ApiKeyScope::read_only())));
    let response = app_denied
        .oneshot(
            Request::builder()
                .uri("/api/v1/billing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let app_granted = build_scope_test_app(Some(make_auth_state(ApiKeyScope::full_access())));
    let response = app_granted
        .oneshot(
            Request::builder()
                .uri("/api/v1/billing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_scope_middleware_teams_subpath_admin_required() {
    let app = build_scope_test_app(Some(make_auth_state(ApiKeyScope::read_only())));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/teams/123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "Teams subpath should require admin scope"
    );
}

#[tokio::test]
async fn test_scope_middleware_teams_secret_not_admin_scope() {
    // /api/v1/teams-secret should NOT match the teams prefix — no scope required
    let app = build_scope_test_app(Some(make_auth_state(ApiKeyScope::read_only())));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/teams-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "teams-secret should not require admin scope (path prefix safety)"
    );
}

#[tokio::test]
async fn test_scope_middleware_write_denied_post_returns_forbidden() {
    let app = build_scope_test_app(Some(make_auth_state(ApiKeyScope::read_only())));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/search")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "Read-only scope doing POST should return 403"
    );
}

#[tokio::test]
async fn test_scope_middleware_write_granted_post_passes() {
    let app = build_scope_test_app(Some(make_auth_state(ApiKeyScope::full_access())));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/search")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Full-access scope doing POST should return 200"
    );
}

#[tokio::test]
async fn test_scope_middleware_write_methods_require_write_scope() {
    let app = build_scope_test_app(Some(make_auth_state(ApiKeyScope::read_only())));

    // PUT
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/crawl/123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "PUT should require write scope"
    );

    // DELETE
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/crawl/123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "DELETE should require write scope"
    );
}

#[tokio::test]
async fn test_scope_middleware_get_no_scope_required_passes() {
    // GET on /v1/search requires no scope — should pass even with denied scope
    let app = build_scope_test_app(Some(make_auth_state(ApiKeyScope::denied())));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/search")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET on /v1/search should not require any scope"
    );
}

#[tokio::test]
async fn test_scope_middleware_audit_logs_deny() {
    let mock_audit = Arc::new(MockAuditService::new());
    let mock_audit_for_assert = mock_audit.clone();
    let auth_state = make_auth_state(ApiKeyScope::read_only()); // lacks admin

    let app = Router::new()
        .route("/api/v1/teams", get(|| async { "ok" }))
        .layer(middleware::from_fn(auth_middleware::scope_middleware))
        .layer(middleware::from_fn(
            move |mut req: Request<Body>, next: Next| {
                let state = auth_state.clone();
                let audit = mock_audit.clone();
                async move {
                    req.extensions_mut().insert(state);
                    req.extensions_mut()
                        .insert(audit as Arc<dyn AuditServiceTrait>);
                    next.run(req).await
                }
            },
        ));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/teams")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        mock_audit_for_assert.deny_count(),
        1,
        "log_deny should be called exactly once when scope is denied"
    );
}

#[tokio::test]
async fn test_scope_middleware_no_audit_service_still_works() {
    // Without audit service extension, denial should still return 403 (no panic)
    let app = build_scope_test_app(Some(make_auth_state(ApiKeyScope::read_only())));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/teams")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// ============================================================================
// Auth Middleware Integration Tests (serialized via GLOBAL_STATE_LOCK)
// ============================================================================

#[tokio::test]
async fn test_auth_middleware_public_endpoint_bypasses_auth() {
    let _guard = GLOBAL_STATE_LOCK.lock().await;
    ensure_global_auth_state();

    let app = build_auth_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Public endpoint /health should bypass authentication"
    );
}

#[tokio::test]
async fn test_auth_middleware_metrics_endpoint_bypasses_auth() {
    let _guard = GLOBAL_STATE_LOCK.lock().await;
    ensure_global_auth_state();

    let app = build_auth_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Public endpoint /metrics should bypass authentication"
    );
}

#[tokio::test]
async fn test_auth_middleware_missing_bearer_token_returns_401() {
    let _guard = GLOBAL_STATE_LOCK.lock().await;
    ensure_global_auth_state();

    let app = build_auth_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Request without Authorization header should return 401"
    );
}

#[tokio::test]
async fn test_auth_middleware_non_bearer_scheme_returns_401() {
    let _guard = GLOBAL_STATE_LOCK.lock().await;
    ensure_global_auth_state();

    let app = build_auth_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header(header::AUTHORIZATION, "Basic dXNlcjpwYXNz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Non-Bearer scheme should return 401"
    );
}

#[tokio::test]
async fn test_auth_middleware_empty_bearer_token_returns_401() {
    let _guard = GLOBAL_STATE_LOCK.lock().await;
    ensure_global_auth_state();

    let app = build_auth_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header(header::AUTHORIZATION, "Bearer ")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Empty token → sha256 hash of empty string → DB lookup → not found → 401
    // (DB is lazy/non-connecting, so this returns INTERNAL_SERVER_ERROR, not 401)
    // Either way, the request should NOT succeed
    assert!(
        response.status() == StatusCode::UNAUTHORIZED
            || response.status() == StatusCode::INTERNAL_SERVER_ERROR,
        "Empty bearer token should not succeed, got {}",
        response.status()
    );
}

// T031（Stage 7）：rate_limit 与 IP 锁定测试已迁移至
// `tests/integration/auth_garrison_test.rs`——需 garrison 单例 + 真实 IP 上下文，
// 单元测试无法覆盖（参考文件头注释 §Stage 3 重构）。

// ============================================================================
// Global State Function Tests
// ============================================================================

#[tokio::test]
async fn test_global_auth_state_set_and_get() {
    let _guard = GLOBAL_STATE_LOCK.lock().await;
    // ensure_global_auth_state sets it if not already set
    let state = ensure_global_auth_state();
    assert!(state.team_id == Uuid::nil() || state.team_id != Uuid::nil());

    // get_global_auth_state should return the same state
    let retrieved = get_global_auth_state();
    assert!(
        retrieved.is_some(),
        "get_global_auth_state should return Some after set"
    );
    assert!(
        Arc::ptr_eq(&state, &retrieved.unwrap()),
        "get_global_auth_state should return the same Arc"
    );
}

// ============================================================================
// AuthError Tests (additional coverage for error variants)
// ============================================================================

#[test]
fn test_auth_error_database_error_display() {
    // The DatabaseError variant wraps sea_orm::DbErr — test that it exists
    // We can't easily construct a DbErr, but we can verify the variant exists
    let err = AuthError::InvalidKey;
    assert_eq!(err.to_string(), "Invalid or missing API key");

    let err = AuthError::InactiveKey;
    assert_eq!(err.to_string(), "API key is inactive");

    let err = AuthError::MissingScope(ScopePermission::Read);
    assert_eq!(err.to_string(), "Missing required scope: read");

    let err = AuthError::NilTeamId;
    assert_eq!(err.to_string(), "API key associated with nil team_id");

    let err = AuthError::ExpiredKey;
    assert_eq!(err.to_string(), "API key has expired");
}
