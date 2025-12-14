// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl;
use crate::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::presentation::handlers::{
    crawl_handler, scrape_handler, search_handler, webhook_handler,
};
use axum::{
    routing::{delete, get, post},
    Router,
};

/// 创建应用路由
///
/// # 返回值
///
/// 返回配置好的路由
pub fn routes() -> Router {
    let public_routes = Router::new()
        .route("/health", get(health_check))
        .route("/v1/version", get(version));

    let protected_routes = Router::new()
        .route("/v1/scrape", post(scrape_handler::create_scrape))
        .route("/v1/scrape/:id", get(scrape_handler::get_scrape_status))
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
            "/v1/crawl/:id",
            get(crawl_handler::get_crawl::<
                CrawlRepositoryImpl,
                TaskRepositoryImpl,
                WebhookRepoImpl,
                ScrapeResultRepositoryImpl,
            >),
        )
        .route(
            "/v1/crawl/:id/results",
            get(crawl_handler::get_crawl_results::<
                CrawlRepositoryImpl,
                TaskRepositoryImpl,
                WebhookRepoImpl,
                ScrapeResultRepositoryImpl,
            >),
        )
        .route(
            "/v1/crawl/:id",
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
            post(search_handler::search::<CrawlRepositoryImpl, TaskRepositoryImpl>),
        );

    Router::new().merge(public_routes).merge(protected_routes)
}

/// 健康检查端点
///
/// # 返回值
///
/// 返回"OK"字符串
pub async fn health_check() -> &'static str {
    "OK"
}

/// 版本信息端点
///
/// # 返回值
///
/// 返回应用版本号
pub async fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
