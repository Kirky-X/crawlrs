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

use axum::Extension;
use axum::{
    routing::{delete, get, post},
    Router,
};
use crawlrs::config::settings::Settings;
use crawlrs::engines::fetch_engine::FetchEngine;
use crawlrs::engines::playwright_engine::PlaywrightEngine;
use crawlrs::engines::router::EngineRouter;
use crawlrs::engines::traits::ScraperEngine;
use crawlrs::infrastructure::cache::redis_client::RedisClient;
use crawlrs::infrastructure::database::connection;
use crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl;
use crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::infrastructure::storage::local::LocalStorage;

use crawlrs::infrastructure::repositories::webhook_event_repo_impl::WebhookEventRepoImpl;
use crawlrs::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl;
use crawlrs::presentation::handlers::{
    crawl_handler, scrape_handler, search_handler, webhook_handler,
};
use crawlrs::presentation::middleware::auth_middleware::{auth_middleware, AuthState};
use crawlrs::presentation::middleware::distributed_rate_limit_middleware::distributed_rate_limit_middleware;
use crawlrs::presentation::middleware::rate_limit_middleware::RateLimiter;
use crawlrs::presentation::routes;
use crawlrs::queue::task_queue::PostgresTaskQueue;
use crawlrs::workers::manager::WorkerManager;
use crawlrs::workers::webhook_worker::WebhookWorker;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

use crawlrs::utils::telemetry;
use migration::{Migrator, MigratorTrait};

/// 主函数
///
/// 应用程序入口点，负责初始化所有组件并启动服务
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize logging
    telemetry::init_telemetry();
    info!("Starting crawlrs...");

    // Initialize Prometheus Metrics
    crawlrs::infrastructure::metrics::init_metrics();

    // 2. Load configuration
    let settings = Arc::new(Settings::new()?);
    info!("Configuration loaded");

    // 3. Connect to database
    let db = connection::create_pool(&settings.database).await?;
    let db = Arc::new(db);
    info!("Database connection established");

    // Run database migrations
    info!("Running database migrations...");
    Migrator::up(db.as_ref(), None).await?;
    info!("Database migrations applied");

    // 4. Initialize Redis Client
    let redis_client = RedisClient::new(&settings.redis.url).await?;
    info!("Redis client initialized");

    // 5. Initialize Rate Limiter
    let rate_limiter = Arc::new(RateLimiter::new(
        redis_client.clone(),
        settings.rate_limiting.default_rpm,
    ));
    info!("Rate limiter initialized");

    // 6. Initialize Components
    let task_repo = Arc::new(TaskRepositoryImpl::new(db.clone()));
    let queue = Arc::new(PostgresTaskQueue::new(task_repo.clone()));
    let result_repo = Arc::new(ScrapeResultRepositoryImpl::new(db.clone()));
    let crawl_repo = Arc::new(CrawlRepositoryImpl::new(db.clone()));

    // Initialize Storage
    let storage_repo = if settings.storage.storage_type == "local" {
        let path = settings
            .storage
            .local_path
            .clone()
            .unwrap_or_else(|| "storage".to_string());
        Some(Arc::new(LocalStorage::new(path)))
    } else {
        // Placeholder for S3 or other storage types
        None
    };

    // Initialize Engines
    let fetch_engine = Arc::new(FetchEngine);
    let playwright_engine = Arc::new(PlaywrightEngine);
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![fetch_engine, playwright_engine];
    let router = Arc::new(EngineRouter::new(engines));

    let webhook_event_repository = Arc::new(WebhookEventRepoImpl::new(db.clone()));
    let webhook_repository = Arc::new(WebhookRepoImpl::new(db.clone()));

    // 7. Start Workers
    let mut worker_manager = WorkerManager::new(
        queue.clone(),
        task_repo.clone(),
        result_repo.clone(),
        crawl_repo.clone(),
        storage_repo.clone(),
        webhook_event_repository.clone(),
        router.clone(),
        redis_client.clone(),
    );
    worker_manager.start_workers(5).await;

    let webhook_worker = WebhookWorker::new(
        webhook_event_repository.clone(),
        settings.webhook.secret.clone(),
    );
    tokio::spawn(async move {
        webhook_worker.run().await;
    });

    // 8. Setup Auth State
    // Initial AuthState doesn't have a valid team_id yet, but it will be populated in the middleware
    // We use Uuid::nil() as a placeholder
    let auth_state = AuthState {
        db: db.clone(),
        team_id: uuid::Uuid::nil(),
    };

    // 9. Start HTTP server
    let public_routes = Router::new()
        .route("/health", get(routes::health_check))
        .route("/v1/version", get(routes::version));

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
        )
        .layer(axum::middleware::from_fn_with_state(
            rate_limiter.clone(),
            distributed_rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            auth_state.clone(),
            auth_middleware,
        ))
        .layer(Extension(task_repo.clone()))
        .layer(Extension(result_repo.clone()))
        .layer(Extension(crawl_repo.clone()))
        .layer(Extension(webhook_repository.clone()))
        .layer(Extension(webhook_event_repository.clone()));

    let app = Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(Extension(queue))
        .layer(Extension(task_repo.clone()))
        .layer(Extension(webhook_event_repository))
        .layer(Extension(webhook_repository.clone()))
        .layer(Extension(redis_client))
        .layer(Extension(rate_limiter))
        .layer(Extension(crawl_repo.clone()))
        .layer(Extension(settings.clone()));

    let addr = format!("{}:{}", settings.server.host, settings.server.port);
    let listener = TcpListener::bind(&addr).await?;
    info!("Server listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
