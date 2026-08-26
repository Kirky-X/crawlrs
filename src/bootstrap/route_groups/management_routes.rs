// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Management 路由组 — 全部路由已迁移至 `presentation::forge_api::management`
//! （sdforge 注册）。本文件仅存空装配点，由 migrate-routes-to-sdforge T013 移除。

use axum::Router;

// R-teams-002：teams 路由已迁移至 `presentation::forge_api::management`（sdforge 注册）

/// 注册 teams 路由组 — 已迁移，保留空实现以维持装配点（T013 移除）。
#[cfg(feature = "teams")]
pub fn register_teams_routes() -> Router {
    Router::new()
}

#[cfg(not(feature = "teams"))]
pub fn register_teams_routes() -> Router {
    Router::new()
}

/// 注册 webhook 相关路由 — 已迁移至 `presentation::forge_api::management`（sdforge 直注）。
///
/// 保留空实现以维持装配点（T013 移除）。
pub fn register_webhook_routes() -> Router {
    Router::new()
}

/// 注册 extract 路由 — 已迁移至 `presentation::forge_api::management`（sdforge 直注）。
///
/// 保留空实现以维持装配点，由 migrate-routes-to-sdforge T013 一并移除。
pub fn register_extract_routes() -> Router {
    Router::new()
}

/// 注册 audit 路由 — 已迁移至 `presentation::forge_api::management`（sdforge 直注）。
///
/// 保留空实现以维持装配点，由 migrate-routes-to-sdforge T013 一并移除。
pub fn register_audit_routes() -> Router {
    Router::new()
}
