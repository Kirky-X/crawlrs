// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

pub mod task;

use crate::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl;
use crate::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crate::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::presentation::handlers::{
    audit_handler, crawl_handler, extract_handler, metrics_handler, scrape_handler, search_handler,
    team_handler, webhook_handler,
};
use crate::presentation::routes::task::task_routes;
use crate::di::AppState;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::utils::regex_cache::RegexCache;
use axum::{
    extract::State,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// 创建应用路由
///
/// # 返回值
///
/// 返回配置好的路由
pub fn routes() -> Router {
    let public_routes = Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_handler::metrics))
        .route("/v1/version", get(version));

    let protected_routes = Router::new()
        .route("/v1/scrape", post(scrape_handler::create_scrape))
        .route("/v1/scrape/{id}", get(scrape_handler::get_scrape_status))
        .route("/v1/scrape/{id}", delete(scrape_handler::cancel_scrape))
        .route(
            "/v1/extract",
            post(extract_handler::extract::<DatabaseGeoRestrictionRepository>),
        )
        .route(
            "/v1/webhooks",
            post(webhook_handler::create_webhook::<WebhookRepoImpl>),
        )
        .route(
            "/v1/crawl",
            post(
                crawl_handler::create_crawl::<
                    CrawlRepositoryImpl,
                    TaskRepositoryImpl,
                    WebhookRepoImpl,
                    ScrapeResultRepositoryImpl,
                    DatabaseGeoRestrictionRepository,
                >,
            ),
        )
        .route(
            "/v1/crawl/{id}",
            get(crawl_handler::get_crawl::<
                CrawlRepositoryImpl,
                TaskRepositoryImpl,
                WebhookRepoImpl,
                ScrapeResultRepositoryImpl,
                DatabaseGeoRestrictionRepository,
            >),
        )
        .route(
            "/v1/crawl/{id}/results",
            get(crawl_handler::get_crawl_results::<
                CrawlRepositoryImpl,
                TaskRepositoryImpl,
                WebhookRepoImpl,
                ScrapeResultRepositoryImpl,
                DatabaseGeoRestrictionRepository,
            >),
        )
        .route(
            "/v1/crawl/{id}",
            delete(
                crawl_handler::cancel_crawl::<
                    CrawlRepositoryImpl,
                    TaskRepositoryImpl,
                    WebhookRepoImpl,
                    ScrapeResultRepositoryImpl,
                    DatabaseGeoRestrictionRepository,
                >,
            ),
        )
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
        .route("/v1/audit/denied", get(audit_handler::get_denied_requests));

    let v2_routes = task_routes();

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(v2_routes)
}

/// 健康检查端点
///
/// # 返回值
///
/// 返回JSON格式的健康状态，包括数据库和Redis连接状态
pub async fn health_check(
    State(app_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let mut health_status = json!({
        "status": "healthy",
        "checks": {}
    });

    let checks = health_status.as_object_mut().unwrap().get_mut("checks").unwrap();

    // 检查数据库连接
    let db_healthy = check_database_health(&app_state).await;
    checks["database"] = json!({
        "status": if db_healthy { "healthy" } else { "unhealthy" }
    });

    // 检查Redis连接
    let redis_healthy = check_redis_health(&app_state).await;
    checks["redis"] = json!({
        "status": if redis_healthy { "healthy" } else { "unhealthy" }
    });

    // 更新整体状态
    if !db_healthy || !redis_healthy {
        health_status["status"] = json!("degraded");
    }

    Json(health_status)
}

/// 检查数据库健康状态
async fn check_database_health(app_state: &Arc<AppState>) -> bool {
    let db = &app_state.db;
    match timeout(Duration::from_secs(5), db.ping()).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}

/// 检查Redis健康状态
async fn check_redis_health(app_state: &Arc<AppState>) -> bool {
    let redis = &app_state.redis_client;
    match timeout(Duration::from_secs(5), redis.ping()).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}

/// 版本信息端点
///
/// # 返回值
///
/// 返回应用版本号
pub async fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
