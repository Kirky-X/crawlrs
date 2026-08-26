// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task 查询/取消路由的 sdforge 直注注册。
//!
//! 泛型 handler 以 turbofish 具体实例化原样提交（与迁移前
//! `task_routes()` 的注册完全一致）；team 并发信号量由装配层的路径条件
//! 中间件在 `/v1/tasks/*` 上保序执行（auth 之后、handler 之前）。

use inventory::submit;
use sdforge::prelude::{ApiMetadata, HttpRoute, RouteRegistration};

use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::presentation::handlers::task_handler;
use crate::presentation::forge_api::route_metadata;

fn query_tasks_route() -> HttpRoute {
    HttpRoute::new(
        "/v1/tasks/_query".to_string(),
        axum::routing::post(task_handler::query_tasks::<TaskRepositoryImpl>),
        route_metadata("tasks_query", "v1", "Query crawl tasks"),
        None,
    )
}

fn query_tasks_metadata() -> ApiMetadata {
    route_metadata("tasks_query", "v1", "Query crawl tasks")
}

submit!(RouteRegistration::new(
    "tasks_query",
    "v1",
    query_tasks_route,
    query_tasks_metadata,
));

fn cancel_tasks_route() -> HttpRoute {
    HttpRoute::new(
        "/v1/tasks/_cancel".to_string(),
        axum::routing::post(task_handler::cancel_tasks::<TaskRepositoryImpl>),
        route_metadata("tasks_cancel", "v1", "Cancel crawl tasks"),
        None,
    )
}

fn cancel_tasks_metadata() -> ApiMetadata {
    route_metadata("tasks_cancel", "v1", "Cancel crawl tasks")
}

submit!(RouteRegistration::new(
    "tasks_cancel",
    "v1",
    cancel_tasks_route,
    cancel_tasks_metadata,
));

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    /// 迁移自 `presentation/routes/task.rs` 内联测试的等价断言（属性不弱化）：
    /// 两条路由可经直注工厂装配；POST 已注册（非 405/404），GET 未注册（405）。
    #[tokio::test]
    async fn task_routes_registered_via_inventory_factories() {
        let app = axum::Router::new()
            .route(query_tasks_route().path(), query_tasks_route().handler().clone())
            .route(cancel_tasks_route().path(), cancel_tasks_route().handler().clone());

        // POST /v1/tasks/_query：方法已注册（缺 Extension 的 500 亦证明进入 handler 链）
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/tasks/_query")
                    .header("content-type", "application/json")
                    .body(String::default())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "POST /v1/tasks/_query must be registered"
        );
        assert_ne!(
            resp.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "POST must be an allowed method on /v1/tasks/_query"
        );

        // GET /v1/tasks/_query：仅注册了 POST，GET 应 405
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/tasks/_query")
                    .body(String::default())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "GET must not be allowed on /v1/tasks/_query (POST only)"
        );
    }
}
