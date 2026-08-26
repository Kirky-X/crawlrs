// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Management 路由组 — webhook / extract / teams / audit / admin 路由注册。
//!
//! 包含所有 feature-gated 路由（webhook、teams、auth）和通用管理路由（audit）。

use crate::presentation::handlers::extract_handler;
use axum::{routing::get, Router};

// R-wh-001 / T028：webhook-off 时不导入
#[cfg(feature = "webhook")]
use crate::infrastructure::database::repositories::webhook_repo_impl::WebhookRepoImpl;
#[cfg(feature = "webhook")]
use crate::presentation::handlers::webhook_handler;

// R-teams-002 / T012：teams-off 时不导入
#[cfg(feature = "teams")]
use crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
#[cfg(feature = "teams")]
use crate::presentation::handlers::team_handler;
#[cfg(feature = "teams")]
use axum::routing::put;

// R-key-lifecycle-001：auth-off 时不导入
#[cfg(feature = "auth")]
use crate::presentation::handlers::api_key_handler;

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
            get(webhook_handler::list_webhooks::<WebhookRepoImpl>),
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
    Router::new().route(
        "/v1/extract",
        axum::routing::post(extract_handler::extract::<DatabaseGeoRestrictionRepository>),
    )
}

#[cfg(not(feature = "teams"))]
pub fn register_extract_routes() -> Router {
    Router::new().route("/v1/extract", axum::routing::post(extract_handler::extract))
}

/// 注册 teams 路由组（feature-gated）。
#[cfg(feature = "teams")]
pub fn register_teams_routes() -> Router {
    Router::new()
        .route("/v1/teams/me", get(team_handler::get_team_info))
        .route("/v1/teams/me/usage", get(team_handler::get_team_usage))
        .route(
            "/v1/teams/geo-restrictions",
            get(team_handler::get_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        )
        .route(
            "/v1/teams/geo-restrictions",
            put(team_handler::update_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        )
}

#[cfg(not(feature = "teams"))]
pub fn register_teams_routes() -> Router {
    Router::new()
}

/// 注册 audit 路由 — 已迁移至 `presentation::forge_api::management`（sdforge 直注）。
///
/// 保留空实现以维持装配点，由 migrate-routes-to-sdforge T013 一并移除。
pub fn register_audit_routes() -> Router {
    Router::new()
}

/// 注册 admin 路由（auth feature-gated）。
#[cfg(feature = "auth")]
pub fn register_admin_routes() -> Router {
    Router::new().route(
        "/v1/admin/api-keys",
        axum::routing::post(api_key_handler::create_api_key),
    )
}

#[cfg(not(feature = "auth"))]
pub fn register_admin_routes() -> Router {
    Router::new()
}
