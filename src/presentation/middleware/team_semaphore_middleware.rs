// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use std::sync::Arc;

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Extension,
};

use crate::domain::services::team_semaphore::TeamSemaphore;
use crate::presentation::middleware::auth_types::AuthState;

/// 从请求扩展中提取 team_id。
///
/// `auth_middleware` 注入 `AuthState`（包含 team_id/api_key_id/scope），
/// 此处从 `AuthState` 读取 team_id（不再读取裸 `Uuid` 扩展——
/// garrison-auth-migration 后 `inject_auth_state` 不再注入裸 `Uuid`）。
fn extract_team_id(request: &Request) -> Option<uuid::Uuid> {
    request.extensions().get::<AuthState>().map(|s| s.team_id)
}

pub async fn team_semaphore_middleware(
    Extension(semaphore): Extension<Arc<TeamSemaphore>>,
    request: Request,
    next: Next,
) -> Response {
    // 从请求扩展中获取team_id（由认证中间件注入）
    let team_id = match extract_team_id(&request) {
        Some(id) => id,
        None => {
            log::warn!("No team_id found in request extensions - authentication may have failed");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    // 使用真实的team_id获取并发许可
    match semaphore.acquire(team_id).await {
        Ok(_permit) => next.run(request).await,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::create_test_db_pool;
    use crate::domain::auth::ApiKeyScope;
    use axum::{body::Body, http::StatusCode, routing::get, Router};
    use std::sync::Arc;
    use tower::ServiceExt;
    use uuid::Uuid;

    fn skip_if_no_test_db() -> bool {
        crate::common::test_helpers::skip_if_no_test_db()
    }

    /// 构造带 `AuthState`（含 team_id）的测试请求。
    fn build_request_with_auth_state(team_id: Option<Uuid>) -> Request {
        let mut builder = Request::builder()
            .uri("/test")
            .body(Body::empty())
            .expect("body should build");
        if let Some(id) = team_id {
            let pool = create_test_db_pool();
            let auth_state = AuthState::new(pool, id, Uuid::new_v4(), ApiKeyScope::full_access());
            builder.extensions_mut().insert(auth_state);
        }
        builder
    }

    #[test]
    fn test_extract_team_id_returns_some_when_present() {
        if skip_if_no_test_db() {
            return;
        }
        let team_id = Uuid::new_v4();
        let request = build_request_with_auth_state(Some(team_id));
        assert_eq!(extract_team_id(&request), Some(team_id));
    }

    #[test]
    fn test_extract_team_id_returns_none_when_absent() {
        if skip_if_no_test_db() {
            return;
        }
        let request = build_request_with_auth_state(None);
        assert_eq!(extract_team_id(&request), None);
    }

    #[test]
    fn test_extract_team_id_ignores_other_extension_types() {
        if skip_if_no_test_db() {
            return;
        }
        // Extensions with non-AuthState types should not satisfy the lookup
        let mut request = Request::builder()
            .uri("/test")
            .body(Body::empty())
            .expect("body should build");
        request
            .extensions_mut()
            .insert("not-an-auth-state".to_string());
        assert_eq!(extract_team_id(&request), None);
    }

    fn test_router(semaphore: Arc<TeamSemaphore>) -> Router {
        // Layer order matters: Extension must be outermost (added last) so the
        // middleware can read it. In axum, the last .layer() is outermost.
        Router::new()
            .route("/test", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(team_semaphore_middleware))
            .layer(Extension(semaphore))
    }

    #[tokio::test]
    async fn test_middleware_unauthorized_when_no_team_id() {
        if skip_if_no_test_db() {
            return;
        }
        let semaphore = Arc::new(TeamSemaphore::new(1));
        let app = test_router(semaphore);
        let request = build_request_with_auth_state(None);
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_middleware_passes_through_when_team_id_present() {
        if skip_if_no_test_db() {
            return;
        }
        let semaphore = Arc::new(TeamSemaphore::new(1));
        let app = test_router(semaphore);
        let team_id = Uuid::new_v4();
        let request = build_request_with_auth_state(Some(team_id));
        let response = app.oneshot(request).await.expect("oneshot should succeed");
        assert_eq!(response.status(), StatusCode::OK);
    }
}
