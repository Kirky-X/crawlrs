// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Search 路由组 — `/v1/search` 相关路由注册。

use crate::presentation::handlers::search_handler;
use axum::{routing::post, Router};

/// 注册 search 相关路由到给定 Router。
///
/// - `POST /v1/search` — 执行搜索查询
pub fn register_search_routes() -> Router {
    Router::new().route("/v1/search", post(search_handler::search))
}
