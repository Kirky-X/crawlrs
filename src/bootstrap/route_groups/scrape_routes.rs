// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

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
pub fn register_scrape_routes() -> Router {
    Router::new()
        .route("/v1/scrape", post(scrape_handler::create_scrape))
        .route("/v1/scrape/{id}", get(scrape_handler::get_scrape_status))
}
