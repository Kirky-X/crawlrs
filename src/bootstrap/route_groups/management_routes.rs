// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Management 路由组 — 剩余 webhook / extract 路由注册。
//!
//! audit / admin / teams 已迁移至 `presentation::forge_api::management`
//! （sdforge 注册）；本文件仅存 webhook 与 extract，由
//! migrate-routes-to-sdforge T011/T012 迁出后于 T013 整体移除。

use crate::presentation::handlers::extract_handler;
use axum::Router;

// R-wh-001 / T028：webhook-off 时不导入
#[cfg(feature = "webhook")]
use crate::infrastructure::database::repositories::webhook_repo_impl::WebhookRepoImpl;
#[cfg(feature = "webhook")]
use crate::presentation::handlers::webhook_handler;

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

/// 注册 webhook 相关路由（feature-gated）。
#[cfg(feature = "webhook")]
pub fn register_webhook_routes() -> Router {
    Router::new()
        .route(
            "/v1/webhooks",
            axum::routing::post(webhook_handler::create_webhook::<WebhookRepoImpl>),
        )
        .route(
            "/v1/webhooks",
            axum::routing::get(webhook_handler::list_webhooks::<WebhookRepoImpl>),
        )
}

/// webhook-off 时的空路由占位。
#[cfg(not(feature = "webhook"))]
pub fn register_webhook_routes() -> Router {
    Router::new()
}

/// 注册 extract 路由（teams feature 分裂）。
#[cfg(feature = "teams")]
pub fn register_extract_routes() -> Router {
    use crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;

    Router::new().route(
        "/v1/extract",
        axum::routing::post(extract_handler::extract::<DatabaseGeoRestrictionRepository>),
    )
}

#[cfg(not(feature = "teams"))]
pub fn register_extract_routes() -> Router {
    Router::new().route("/v1/extract", axum::routing::post(extract_handler::extract))
}

/// 注册 audit 路由 — 已迁移至 `presentation::forge_api::management`（sdforge 直注）。
///
/// 保留空实现以维持装配点，由 migrate-routes-to-sdforge T013 一并移除。
pub fn register_audit_routes() -> Router {
    Router::new()
}
