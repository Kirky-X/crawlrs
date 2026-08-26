// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scrape 路由组 — 已迁移至 `presentation::forge_api::scrape`（sdforge 注册）。
//!
//! 本函数保留空实现以维持 `create_protected_routes_with_state` 的装配点，
//! 由 migrate-routes-to-sdforge T013 随装配收缩一并移除。

use axum::Router;

/// 注册 scrape 相关路由到给定 Router。
///
/// - `POST /v1/scrape` — 创建抓取任务（现由 forge_api::scrape 直注）
/// - `GET /v1/scrape/{id}` — 查询抓取状态（现由 forge_api::scrape 宏注册）
pub fn register_scrape_routes() -> Router {
    Router::new()
}
