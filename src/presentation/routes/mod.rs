// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

pub mod task;

use crate::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl;
use crate::infrastructure::repositories::credits_repo_impl::CreditsRepositoryImpl;
use crate::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::presentation::handlers::{
    crawl_handler, extract_handler, metrics_handler, scrape_handler, search_handler,
    webhook_handler,
};
use crate::presentation::routes::task::task_routes;
use axum::{
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::json;

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
        .route("/v1/extract", post(extract_handler::extract))
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
            >),
        )
        .route(
            "/v1/crawl/{id}/results",
            get(crawl_handler::get_crawl_results::<
                CrawlRepositoryImpl,
                TaskRepositoryImpl,
                WebhookRepoImpl,
                ScrapeResultRepositoryImpl,
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
                >,
            ),
        )
        .route(
            "/v1/search",
            post(
                search_handler::search::<
                    CrawlRepositoryImpl,
                    TaskRepositoryImpl,
                    CreditsRepositoryImpl,
                >,
            ),
        );

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
