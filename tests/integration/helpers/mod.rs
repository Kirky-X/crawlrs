// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::Extension;
use axum_test::TestServer;
use crawlrs::application::usecases::create_scrape::CreateScrapeUseCase;
use crawlrs::config::settings::{DatabaseSettings, Settings};
use crawlrs::engines::playwright_engine::PlaywrightEngine;
use crawlrs::engines::reqwest_engine::ReqwestEngine;
use crawlrs::engines::router::EngineRouter;
use crawlrs::engines::traits::ScraperEngine;
use crawlrs::infrastructure::cache::redis_client::RedisClient;
use crawlrs::infrastructure::database::connection;
use crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl;
use crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::infrastructure::repositories::webhook_event_repo_impl::WebhookEventRepoImpl;
use crawlrs::infrastructure::storage::local::LocalStorage;
use crawlrs::presentation::middleware::auth_middleware::{auth_middleware, AuthState};
use crawlrs::presentation::middleware::distributed_rate_limit_middleware::distributed_rate_limit_middleware;
use crawlrs::presentation::middleware::rate_limit_middleware::RateLimiter;
use crawlrs::presentation::routes;
use crawlrs::queue::task_queue::PostgresTaskQueue;
use crawlrs::workers::manager::WorkerManager;
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use std::sync::Arc;

use testcontainers::runners::AsyncRunner;
use uuid::Uuid;

use crawlrs::utils::robots::RobotsChecker;

#[allow(dead_code)]
pub struct TestApp {
    pub server: TestServer,
    pub db_pool: Arc<DatabaseConnection>,
    pub api_key: String,
    pub task_repo: Arc<TaskRepositoryImpl>,
    pub worker_manager: Option<
        WorkerManager<
            PostgresTaskQueue<TaskRepositoryImpl>,
            TaskRepositoryImpl,
            ScrapeResultRepositoryImpl,
            CrawlRepositoryImpl,
            LocalStorage,
        >,
    >,
    // Keep nodes alive
    pub postgres_node: testcontainers::ContainerAsync<testcontainers::GenericImage>,
    pub redis_node: testcontainers::ContainerAsync<testcontainers::GenericImage>,
}

pub async fn create_test_app() -> TestApp {
    create_test_app_with_options(true).await
}

pub async fn create_test_app_no_worker() -> TestApp {
    create_test_app_with_options(false).await
}

async fn create_test_app_with_options(start_worker: bool) -> TestApp {
    // Start PostgreSQL container
    // Note: testcontainers 0.17 with testcontainers-modules 0.5 automatically uses the default tag (usually latest)
    // or whatever is specified in the module.
    // If we can't change the tag easily, we should ensure the image it tries to pull is available.
    // However, the error message says it fails to pull postgres:11-alpine.
    // We want to force it to use a newer version if possible, or we need to pull 11-alpine manually.
    // Since we cannot easily change the tag in code with this version combination without a trait,
    // let's rely on the default and hope the environment can pull it if we fix the network or image name.
    // But wait, the error is about `postgres:11-alpine`. This implies the default in the library IS 11-alpine.
    // Let's try to instantiate a GenericImage instead to control the tag.

    let postgres_node = testcontainers::GenericImage::new("postgres", "15-alpine")
        .with_env_var("POSTGRES_DB", "postgres")
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .start()
        .await
        .expect("Failed to start Postgres");

    let postgres_port = postgres_node
        .get_host_port_ipv4(5432)
        .await
        .expect("Failed to get Postgres port");
    let db_url = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres_port
    );

    // Start Redis container
    let redis_node = testcontainers::GenericImage::new("redis", "7-alpine")
        .start()
        .await
        .expect("Failed to start Redis");
    let redis_port = redis_node
        .get_host_port_ipv4(6379)
        .await
        .expect("Failed to get Redis port");
    let redis_url = format!("redis://127.0.0.1:{}", redis_port);

    // Create database connection pool
    let db_settings = DatabaseSettings {
        url: db_url.clone(),
        max_connections: None,
        min_connections: None,
        connect_timeout: None,
        idle_timeout: None,
    };

    // Retry logic for database connection
    let mut db_pool = None;
    for _ in 0..20 {
        match connection::create_pool(&db_settings).await {
            Ok(pool) => {
                db_pool = Some(Arc::new(pool));
                break;
            }
            Err(_) => {
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            }
        }
    }
    let db_pool = db_pool.expect("Failed to connect to database");

    // Run migrations
    Migrator::up(db_pool.as_ref(), None).await.unwrap();

    // Create a test team and API key
    let api_key = Uuid::new_v4().to_string();
    let team_id = Uuid::new_v4();

    // This is a simplified insertion for testing purposes.
    // In a real scenario, you would use your repository layer.
    let insert_team_stmt = format!(
        "INSERT INTO teams (id, name, created_at, updated_at) VALUES ('{}', 'test-team', NOW(), NOW())",
        team_id
    );
    let insert_api_key_stmt = format!(
        "INSERT INTO api_keys (key, team_id, created_at, updated_at) VALUES ('{}', '{}', NOW(), NOW())",
        api_key, team_id
    );

    db_pool
        .execute(Statement::from_string(
            DbBackend::Postgres,
            insert_team_stmt,
        ))
        .await
        .unwrap();
    db_pool
        .execute(Statement::from_string(
            DbBackend::Postgres,
            insert_api_key_stmt,
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
    let queue = Arc::new(PostgresTaskQueue::new(task_repo.clone()));

    // Initialize dependencies for WorkerManager
    let result_repo = Arc::new(ScrapeResultRepositoryImpl::new(db_pool.clone()));
    let crawl_repo = Arc::new(CrawlRepositoryImpl::new(db_pool.clone()));
    let webhook_event_repo = Arc::new(WebhookEventRepoImpl::new(db_pool.clone()));
    let storage_repo = Some(Arc::new(LocalStorage::new("test_storage".to_string())));

    let reqwest_engine = Arc::new(ReqwestEngine);
    let playwright_engine = Arc::new(PlaywrightEngine);
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine, playwright_engine];
    let router = Arc::new(EngineRouter::new(engines));

    let create_scrape_use_case = Arc::new(CreateScrapeUseCase::new(router.clone()));
    let robots_checker = Arc::new(RobotsChecker::new());

    let mut worker_manager = WorkerManager::new(
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
        .layer(Extension(redis_client))
        .layer(Extension(rate_limiter))
        .layer(Extension(Arc::new(Settings::new().unwrap()))); // Use default settings for tests

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
        postgres_node,
        redis_node,
    }
}
