// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl 路由组 — 已迁移至 `presentation::forge_api::crawl`（sdforge 注册）。
//!
//! 本函数保留空实现以维持 `create_protected_routes_with_state` 的装配点，
//! 由 migrate-routes-to-sdforge T013 随装配收缩一并移除。

use axum::Router;

/// 注册 crawl 相关路由到给定 Router。
///
/// - `POST /v1/crawl` — 创建爬取任务（现由 forge_api::crawl 直注）
/// - `GET/DELETE /v1/crawl/{id}` — 查询/取消（现由 forge_api::crawl 单条目直注）
/// - `GET /v1/crawl/{id}/results` — 查询结果（现由 forge_api::crawl 宏注册）
pub fn register_crawl_routes() -> Router {
    Router::new()
}
