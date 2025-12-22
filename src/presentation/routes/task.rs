// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::routing::{delete, post};
use axum::Router;

use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::presentation::handlers::task_handler;

/// 创建v2任务相关路由
///
/// # 返回值
///
/// 返回配置好的v2任务路由
pub fn task_routes() -> Router {
    Router::new()
        .route(
            "/v2/tasks/query",
            post(task_handler::query_tasks::<TaskRepositoryImpl>),
        )
        .route(
            "/v2/tasks/cancel",
            delete(task_handler::cancel_tasks::<TaskRepositoryImpl>),
        )
}
