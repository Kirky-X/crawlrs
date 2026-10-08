// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Scrape 路由组 — `/v1/scrape` 相关路由注册。

use crate::presentation::handlers::scrape_handler;
use axum::{
    routing::{get, post},
    Router,
};

/// 注册 scrape 相关路由到给定 Router。
///
/// - `POST /v1/scrape` — 创建抓取任务
/// - `GET /v1/scrape/{id}` — 查询抓取状态
/// - `POST /v1/scrape/{id}/_cancel` — 取消单个抓取任务
///   （与 `POST /v1/tasks/_cancel` 批量取消互补；handler 为
///   `scrape_handler::cancel_scrape`，见 docs/API_REFERENCE.md）
pub fn register_scrape_routes() -> Router {
    Router::new()
        .route("/v1/scrape", post(scrape_handler::create_scrape))
        .route("/v1/scrape/{id}", get(scrape_handler::get_scrape_status))
        .route(
            "/v1/scrape/{id}/_cancel",
            post(scrape_handler::cancel_scrape),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use tower::ServiceExt;

    /// 生产装配必须注册单任务取消端点：此前仅存在于测试路由
    /// （presentation/routes/handlers.rs）导致生产 404（文档审计缺陷 #2）。
    #[tokio::test]
    async fn test_scrape_cancel_route_is_registered() {
        let app = register_scrape_routes();
        // handler 需要 Extension 依赖；带认证中间件前直接调用会 500，
        // 因此只验证路由存在（非 404）而非完整业务流。
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/v1/scrape/01936b70-8c5f-7cc0-9c4a-000000000001/_cancel")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(
            response.status(),
            StatusCode::NOT_FOUND,
            "POST /v1/scrape/{{id}}/_cancel 必须在生产装配中注册"
        );
    }
}
