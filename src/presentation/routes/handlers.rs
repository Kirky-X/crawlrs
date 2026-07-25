// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 路由处理函数
//!
//! 从 mod.rs 拆出的 routes/health_check/version 函数实现。

// R-teams-004 / T014：teams feature 关闭时不导入 teams 相关类型
#[cfg(feature = "teams")]
use crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crate::infrastructure::database::repositories::task_repo_impl::TaskRepositoryImpl;
// R-wh-001 / T028：webhook feature 关闭时不导入 WebhookRepoImpl
#[cfg(feature = "webhook")]
use crate::infrastructure::database::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::presentation::handlers::{
    audit_handler, crawl_handler, extract_handler, metrics_handler, scrape_handler, search_handler,
    task_handler,
};
// R-wh-001 / T028：webhook-off 时 webhook_handler 模块不编译
#[cfg(feature = "webhook")]
use crate::presentation::handlers::webhook_handler;
// R-teams-002 / T012：teams-off 时 team_handler 模块不编译
#[cfg(feature = "teams")]
use crate::presentation::handlers::team_handler;
use axum::{
    routing::{get, post},
    Json, Router,
};
// R-teams-002 / T012：put 仅在 teams-on 时被使用（/v1/teams/geo-restrictions PUT）
#[cfg(feature = "teams")]
use axum::routing::put;
use serde_json::json;

/// 创建应用路由
///
/// # 返回值
///
/// 返回配置好的路由
///
/// R-teams-004 / R-wh-003：feature-off 时跳过对应路由注册。
/// `bootstrap/routes.rs::build_api_app_with_state` 是主装配入口，
/// 此函数保留用于 routes/mod.rs 单元测试。
pub fn routes() -> Router {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_handler::metrics))
        .route("/v1/version", get(version))
        .route("/v1/scrape", post(scrape_handler::create_scrape))
        .route("/v1/scrape/{id}", get(scrape_handler::get_scrape_status))
        .route(
            "/v1/scrape/{id}/_cancel",
            post(scrape_handler::cancel_scrape),
        )
        .route("/v1/crawl", post(crawl_handler::create_crawl))
        .route("/v1/crawl/{id}", get(crawl_handler::get_crawl))
        .route(
            "/v1/crawl/{id}/results",
            get(crawl_handler::get_crawl_results),
        )
        .route("/v1/crawl/{id}/_cancel", post(crawl_handler::cancel_crawl))
        .route("/v1/search", post(search_handler::search));

    // R-teams-003 / T013：extract 路由按 teams feature 分裂
    #[cfg(feature = "teams")]
    let app = app.route(
        "/v1/extract",
        post(extract_handler::extract::<DatabaseGeoRestrictionRepository>),
    );
    #[cfg(not(feature = "teams"))]
    let app = app.route("/v1/extract", post(extract_handler::extract));

    // R-wh-001 / T028：/v1/webhooks 路由按 webhook feature 分裂
    #[cfg(feature = "webhook")]
    let app = app.route(
        "/v1/webhooks",
        post(webhook_handler::create_webhook::<WebhookRepoImpl>),
    );

    // R-teams-002 / T012：/v1/teams/* 路由按 teams feature 分裂
    #[cfg(feature = "teams")]
    let app = app
        .route(
            "/v1/teams/geo-restrictions",
            get(team_handler::get_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        )
        .route(
            "/v1/teams/geo-restrictions",
            put(team_handler::update_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        );

    app.route("/v1/audit/logs", get(audit_handler::get_audit_logs))
        .route("/v1/audit/denied", get(audit_handler::get_denied_requests))
        .route(
            "/v1/tasks/_query",
            post(task_handler::query_tasks::<TaskRepositoryImpl>),
        )
        .route(
            "/v1/tasks/_cancel",
            post(task_handler::cancel_tasks::<TaskRepositoryImpl>),
        )
}

/// 健康检查端点（liveness probe）
///
/// 这是 Kubernetes liveness probe — 总是返回 200 OK + "healthy"，
/// 表示进程存活。不检查依赖（数据库、缓存）— 避免依赖短暂故障导致 pod 重启。
/// 如需 readiness probe（检查依赖是否就绪），应单独实现 /ready endpoint
/// 注入 AppState 检查数据库连接池、缓存可用性等。
///
/// # 返回值
///
/// 返回JSON格式的健康状态 + 版本号
pub async fn health_check() -> Json<serde_json::Value> {
    Json(json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// 版本信息端点
///
/// # 返回值
///
/// 返回应用版本号
pub async fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
