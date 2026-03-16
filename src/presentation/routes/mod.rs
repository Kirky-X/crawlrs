// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

pub mod task;

use crate::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::presentation::handlers::{
    audit_handler, crawl_handler, extract_handler, metrics_handler, scrape_handler, search_handler,
    task_handler, team_handler, webhook_handler,
};
use axum::{
    routing::{get, post, put},
    Json, Router,
};
use serde_json::json;

/// 创建应用路由
///
/// # 返回值
///
/// 返回配置好的路由
pub fn routes() -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_handler::metrics))
        .route("/v1/version", get(version))
        .route("/v1/scrape", post(scrape_handler::create_scrape))
        .route("/v1/scrape/{id}", get(scrape_handler::get_scrape_status))
        .route("/v1/scrape/{id}/_cancel", post(scrape_handler::cancel_scrape))
        .route(
            "/v1/extract",
            post(extract_handler::extract::<DatabaseGeoRestrictionRepository>),
        )
        .route(
            "/v1/webhooks",
            post(webhook_handler::create_webhook::<WebhookRepoImpl>),
        )
        .route("/v1/crawl", post(crawl_handler::create_crawl))
        .route("/v1/crawl/{id}", get(crawl_handler::get_crawl))
        .route(
            "/v1/crawl/{id}/results",
            get(crawl_handler::get_crawl_results),
        )
        .route("/v1/crawl/{id}/_cancel", post(crawl_handler::cancel_crawl))
        .route("/v1/search", post(search_handler::search))
        .route(
            "/v1/teams/geo-restrictions",
            get(team_handler::get_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        )
        .route(
            "/v1/teams/geo-restrictions",
            put(team_handler::update_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        )
        .route("/v1/audit/logs", get(audit_handler::get_audit_logs))
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

/// 健康检查端点
///
/// # 返回值
///
/// 返回JSON格式的健康状态
pub async fn health_check() -> Json<serde_json::Value> {
    Json(json!({ "status": "healthy" }))
}

/// 版本信息端点
///
/// # 返回值
///
/// 返回应用版本号
pub async fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
