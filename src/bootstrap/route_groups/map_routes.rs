// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Map 路由组 — `/v1/map`（F11 sitemap 流式站点图）。
//!
//! 本模块整体由 `route_groups::mod` 的 `#[cfg(feature = "sitemap")]` 门控。

use crate::presentation::handlers::map_handler;
use axum::{routing::post, Router};

/// 注册 map 相关路由到给定 Router。
///
/// - `POST /v1/map` — 从站点 sitemap 发现 URL
pub fn register_map_routes() -> Router {
    Router::new().route("/v1/map", post(map_handler::map))
}
