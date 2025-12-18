// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::Extension;
use axum_test::TestServer;
use crawlrs::application::usecases::create_scrape::CreateScrapeUseCase;
use crawlrs::config::settings::Settings;
use crawlrs::engines::playwright_engine::PlaywrightEngine;
use crawlrs::engines::reqwest_engine::ReqwestEngine;
use crawlrs::engines::router::EngineRouter;
use crawlrs::engines::traits::ScraperEngine;
use crawlrs::infrastructure::cache::redis_client::RedisClient;
use crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl;
use crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::infrastructure::repositories::webhook_event_repo_impl::WebhookEventRepoImpl;
use crawlrs::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl;
use crawlrs::infrastructure::storage::LocalStorage;
use crawlrs::presentation::middleware::auth_middleware::{auth_middleware, AuthState};
use crawlrs::presentation::middleware::distributed_rate_limit_middleware::distributed_rate_limit_middleware;
use crawlrs::presentation::middleware::rate_limit_middleware::RateLimiter;
use crawlrs::presentation::routes;
use crawlrs::workers::manager::WorkerManager;
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use std::process::{Child, Command};
use std::sync::Arc;
use uuid::Uuid;

use crawlrs::utils::robots::RobotsChecker;

use crawlrs::queue::task_queue::{TaskQueue, PostgresTaskQueue};

#[allow(dead_code)]
pub struct TestApp {
    pub server: TestServer,
    pub db_pool: Arc<DatabaseConnection>,
    pub api_key: String,
    pub task_repo: Arc<TaskRepositoryImpl>,
    pub worker_manager: Option<
        WorkerManager<
            Arc<dyn TaskQueue>,
            TaskRepositoryImpl,
            ScrapeResultRepositoryImpl,
            CrawlRepositoryImpl,
            LocalStorage,
        >,
    >,
    // Keep redis process alive
    pub redis_process: Option<Child>,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        if let Some(mut child) = self.redis_process.take() {
            let _ = child.kill();
        }
    }
}

pub async fn create_test_app() -> TestApp {
    create_test_app_with_options(true).await
}

pub async fn create_test_app_no_worker() -> TestApp {
    create_test_app_with_options(false).await
}

async fn create_test_app_with_options(start_worker: bool) -> TestApp {
    create_test_app_with_rate_limit_options(start_worker, false).await
}

pub async fn create_test_app_with_rate_limit_options(start_worker: bool, enable_rate_limiting: bool) -> TestApp {
    // 1. Setup SQLite
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let db_pool = Arc::new(db);
    
    // 2. Setup Redis
    let start_port = 6379;
    let result = crawlrs::utils::port_sniffer::PortSniffer::find_available_port(start_port, true).unwrap();
    let redis_port = result.port;
    let redis_process = Command::new("redis-server")
        .arg("--port")
        .arg(redis_port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to start redis-server");
        
    // Wait for redis to start
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    let redis_url = format!("redis://127.0.0.1:{}", redis_port);

    // Run migrations
    Migrator::up(db_pool.as_ref(), None).await.unwrap();

    // Create a test team and API key
    let api_key = Uuid::new_v4().to_string();
    let team_id = Uuid::new_v4();

    // Insert data (SQLite syntax)
    db_pool
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, 'test-team', datetime('now'), datetime('now'))",
            vec![team_id.into()],
        ))
        .await
        .unwrap();

    db_pool
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES (?, ?, ?, datetime('now'), datetime('now'))",
            vec![Uuid::new_v4().into(), api_key.clone().into(), team_id.into()],
        ))
        .await
        .unwrap();

    // Initialize Redis client
    let redis_client = RedisClient::new(&redis_url).await.unwrap();

    // Initialize Rate Limiter
    let rate_limiter = Arc::new(RateLimiter::new(redis_client.clone(), 100)); // 100 RPM for tests

    // Initialize other components
    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_pool.clone(),
        chrono::Duration::seconds(10),
    ));
    
    // Use PostgresTaskQueue for proper task processing
    let queue: Arc<dyn TaskQueue> = Arc::new(PostgresTaskQueue::new(task_repo.clone()));

    // Initialize dependencies for WorkerManager
    let result_repo = Arc::new(ScrapeResultRepositoryImpl::new(db_pool.clone()));
    let crawl_repo = Arc::new(CrawlRepositoryImpl::new(db_pool.clone()));
    let webhook_event_repo = Arc::new(WebhookEventRepoImpl::new(db_pool.clone()));
    let webhook_repo = Arc::new(WebhookRepoImpl::new(db_pool.clone()));
    let storage_repo = Some(Arc::new(LocalStorage::new("test_storage".to_string())));

    let reqwest_engine = Arc::new(ReqwestEngine);
    let playwright_engine = Arc::new(PlaywrightEngine);
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine, playwright_engine];
    let router = Arc::new(EngineRouter::new(engines));

    let create_scrape_use_case = Arc::new(CreateScrapeUseCase::new(router.clone()));
    let robots_checker = Arc::new(RobotsChecker::new());

    // Create custom settings for tests
    let mut settings = Settings::new().unwrap();
    settings.rate_limiting.enabled = enable_rate_limiting; // 根据参数决定是否启用速率限制
    let settings = Arc::new(settings);

    let mut worker_manager: WorkerManager<
        Arc<dyn TaskQueue>,
        TaskRepositoryImpl,
        ScrapeResultRepositoryImpl,
        CrawlRepositoryImpl,
        LocalStorage,
    > = WorkerManager::new(
        queue.clone(),
        task_repo.clone(),
        result_repo.clone(),
        crawl_repo.clone(),
        storage_repo.clone(),
        webhook_event_repo.clone(),
        router.clone(),
        create_scrape_use_case.clone(),
        redis_client.clone(),
        robots_checker.clone(),
        settings.clone(),
        10,
    );

    // Start 1 worker in the background
    if start_worker {
        worker_manager.start_workers(1).await;
    }

    // AuthState
    let auth_state = AuthState {
        db: db_pool.clone(),
        team_id: Uuid::nil(), // Placeholder, will be set by middleware
    };

    // Build the app router
    let app = routes::routes()
        .layer(axum::middleware::from_fn_with_state(
            rate_limiter.clone(),
            distributed_rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            auth_middleware,
        ))
        .layer(Extension(queue))
        .layer(Extension(task_repo.clone()))
        .layer(Extension(crawl_repo))
        .layer(Extension(result_repo))
        .layer(Extension(webhook_repo))
        .layer(Extension(redis_client))
        .layer(Extension(rate_limiter))
        .layer(Extension(settings)); // Use default settings for tests

    let server = TestServer::new(app).unwrap();

    TestApp {
        server,
        db_pool,
        api_key,
        task_repo,
        worker_manager: if start_worker {
            Some(worker_manager)
        } else {
            None
        },
        redis_process: Some(redis_process),
    }
}
