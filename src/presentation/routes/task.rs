// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::routing::post;
use axum::Router;

use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::presentation::handlers::task_handler;

/// 创建任务相关路由
///
/// # 返回值
///
/// 返回配置好的任务路由
///
/// # RESTful 规范说明
///
/// - POST /v1/tasks/_query - 复杂查询使用 POST + _query 后缀
/// - POST /v1/tasks/_cancel - 批量取消操作使用 POST + _cancel 后缀
pub fn task_routes() -> Router {
    Router::new()
        .route(
            "/v1/tasks/_query",
            post(task_handler::query_tasks::<TaskRepositoryImpl>),
        )
        .route(
            "/v1/tasks/_cancel",
            post(task_handler::cancel_tasks::<TaskRepositoryImpl>),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_routes_returns_router_without_panic() {
        // task_routes() should construct a Router with two POST routes
        // without requiring any state or panicking.
        let router = task_routes();
        // Router<()> is Clone; verify it is usable
        let _clone = router.clone();
    }

    #[test]
    fn test_task_routes_clone_is_independent() {
        let router = task_routes();
        let clone = router.clone();
        // Both should be usable (no shared mutable state issues)
        drop(router);
        drop(clone);
    }
}
