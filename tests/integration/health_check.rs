// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use axum::Extension;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::presentation::middleware::auth_middleware::{auth_middleware, AuthState};
use crawlrs::presentation::routes;
use crawlrs::queue::task_queue::PostgresTaskQueue;
use sea_orm::MockDatabase;
use std::sync::Arc;
use tower::util::ServiceExt;

/// 健康检查测试
///
/// 验证健康检查端点是否正常工作
#[tokio::test]
async fn health_check_works() {
    let app = routes::routes();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

/// 未授权爬取端点测试
///
/// 验证爬取端点在没有认证时返回401状态码
#[tokio::test]
async fn scrape_endpoint_returns_401_without_auth() {
    // Mock DB for AuthState
    let db = MockDatabase::new(sea_orm::DatabaseBackend::Postgres).into_connection();
    let db_arc = Arc::new(db);
    // Initialize AuthState with dummy team_id as it's required but not used for 401 check structure
    let auth_state = AuthState {
        db: db_arc.clone(),
        team_id: uuid::Uuid::nil(),
    };

    // Mock Queue (requires real DB connection usually, so we might need to mock repository or use a test DB)
    // For this test, we are checking 401, which happens BEFORE queue access.
    // However, the route setup requires the extension.
    // We'll create a dummy queue with the mock DB.
    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_arc.clone(),
        chrono::Duration::seconds(10),
    ));
    let queue = Arc::new(PostgresTaskQueue::new(task_repo.clone()));

    let app = routes::routes()
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            auth_middleware,
        ))
        .layer(Extension(queue));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/scrape")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"url": "https://example.com"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
