// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Search 路由组 — 已迁移至 `presentation::forge_api::search`（sdforge 注册）。
//!
//! 本函数保留空实现以维持 `create_protected_routes_with_state` 的装配点，
//! 由 migrate-routes-to-sdforge T013 随装配收缩一并移除。

use axum::Router;

/// 注册 search 相关路由到给定 Router。
///
/// - `POST /v1/search` — 执行搜索查询（现由 forge_api::search 注册）
pub fn register_search_routes() -> Router {
    Router::new()
}
