// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl 路由组 — `/v1/crawl` 相关路由注册。

use crate::presentation::handlers::crawl_handler;
use axum::{
    routing::{delete, get, post},
    Router,
};

/// 注册 crawl 相关路由到给定 Router。
///
/// - `POST /v1/crawl` — 创建爬取任务
/// - `GET /v1/crawl/{id}` — 查询爬取状态
/// - `GET /v1/crawl/{id}/results` — 查询爬取结果
/// - `DELETE /v1/crawl/{id}` — 取消爬取任务
pub fn register_crawl_routes() -> Router {
    Router::new()
        .route("/v1/crawl", post(crawl_handler::create_crawl))
        .route("/v1/crawl/{id}", get(crawl_handler::get_crawl))
        .route(
            "/v1/crawl/{id}/results",
            get(crawl_handler::get_crawl_results),
        )
        .route("/v1/crawl/{id}", delete(crawl_handler::cancel_crawl))
}
