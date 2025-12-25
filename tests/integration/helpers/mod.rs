// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use std::process::Command;
use std::sync::Arc;

use rand::Rng;

use axum::Extension;
use axum_test::TestServer;
use crawlrs::application::use_cases::create_scrape::CreateScrapeUseCase;
use crawlrs::config::settings::Settings;
use crawlrs::domain::search::engine::SearchEngine;
use crawlrs::domain::services::team_service::TeamService;
use crawlrs::engines::playwright_engine::PlaywrightEngine;
use crawlrs::engines::reqwest_engine::ReqwestEngine;
use crawlrs::engines::router::EngineRouter;
use crawlrs::engines::traits::ScraperEngine;
use crawlrs::infrastructure::cache::redis_client::RedisClient;
use crawlrs::infrastructure::geolocation::GeoLocationService;
use crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl;
use crawlrs::infrastructure::repositories::credits_repo_impl::CreditsRepositoryImpl;
use crawlrs::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::infrastructure::repositories::tasks_backlog_repo_impl::TasksBacklogRepositoryImpl;
use crawlrs::infrastructure::repositories::webhook_event_repo_impl::WebhookEventRepoImpl;
use crawlrs::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl;
use crawlrs::infrastructure::search::aggregator::SearchAggregator;
use crawlrs::infrastructure::search::google::GoogleSearchEngine;
use crawlrs::infrastructure::services::rate_limiting_service_impl::{
    RateLimitingConfig, RateLimitingServiceImpl,
};
use crawlrs::presentation::middleware::auth_middleware::{auth_middleware, AuthState};
use crawlrs::presentation::middleware::distributed_rate_limit_middleware::distributed_rate_limit_middleware;
use crawlrs::presentation::middleware::rate_limit_middleware::RateLimiter;
use crawlrs::presentation::routes;
use crawlrs::queue::task_queue::{PostgresTaskQueue, TaskQueue};
use crawlrs::utils::robots::RobotsChecker;
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
use uuid::Uuid;

pub mod search_engine_helpers;
pub mod browser_helpers;
pub mod google_helpers;

pub struct TestApp {
    pub server: TestServer,
    pub db_pool: Arc<sea_orm::DatabaseConnection>,
    pub api_key: String,
    pub team_id: uuid::Uuid,
    pub task_repo: Arc<TaskRepositoryImpl>,
    pub worker_manager: Option<Arc<dyn std::any::Any + Send + Sync>>,
    pub redis_process: Option<std::process::Child>,
    pub redis_url: String,
    pub redis: Arc<RedisClient>,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        if let Some(mut redis_process) = self.redis_process.take() {
            let _ = redis_process.kill();
            let _ = redis_process.wait();
        }
    }
}

impl TestApp {
    pub async fn create_team(&self, name: &str) -> (String, uuid::Uuid) {
        let team_id = Uuid::new_v4();
        let api_key = Uuid::new_v4().to_string();

        self.db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, ?, datetime('now'), datetime('now'))",
                vec![team_id.into(), name.into()],
            ))
            .await
            .unwrap();

        self.db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES (?, ?, ?, datetime('now'), datetime('now'))",
                vec![Uuid::new_v4().into(), api_key.clone().into(), team_id.into()],
            ))
            .await
            .unwrap();

        self.db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO credits (id, team_id, balance, created_at, updated_at) VALUES (?, ?, 1000, datetime('now'), datetime('now'))",
                vec![Uuid::new_v4().into(), team_id.into()],
            ))
            .await
            .unwrap();

        (api_key, team_id)
    }
}

async fn setup_test_app_internal(
    enable_rate_limiting: bool,
    enable_distributed_rate_limiting: bool,
    rate_limit_rpm: Option<u32>,
) -> TestApp {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let db_pool = Arc::new(db);

    let start_port = rand::thread_rng().gen_range(10000..60000);
    let result =
        crawlrs::utils::port_sniffer::PortSniffer::find_available_port(start_port, true, 5).unwrap();
    let redis_port = result.port;
    let redis_process = Command::new("redis-server")
        .arg("--port")
        .arg(redis_port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to start redis-server");

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    let redis_url = format!("redis://127.0.0.1:{}", redis_port);

    Migrator::up(db_pool.as_ref(), None).await.unwrap();

    let api_key = Uuid::new_v4().to_string();
    let team_id = Uuid::new_v4();

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

    db_pool
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO credits (id, team_id, balance, created_at, updated_at) VALUES (?, ?, 1000, datetime('now'), datetime('now'))",
            vec![Uuid::new_v4().into(), team_id.into()],
        ))
        .await
        .unwrap();

    let redis_client = RedisClient::new(&redis_url).await.unwrap();

    let rpm = rate_limit_rpm.unwrap_or(1000);
    let rate_limiter = Arc::new(RateLimiter::new(redis_client.clone(), rpm));

    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_pool.clone(),
        chrono::Duration::seconds(10),
    ));
    let credits_repo = Arc::new(CreditsRepositoryImpl::new(db_pool.clone()));
    let backlog_repo = Arc::new(TasksBacklogRepositoryImpl::new(db_pool.clone()));

    let rate_limiting_service: Arc<
        dyn crawlrs::domain::services::rate_limiting_service::RateLimitingService,
    > = Arc::new(RateLimitingServiceImpl::new(
        Arc::new(redis_client.clone()),
        task_repo.clone(),
        backlog_repo,
        credits_repo.clone(),
        RateLimitingConfig::default(),
    ));

    let queue: Arc<dyn TaskQueue> = Arc::new(PostgresTaskQueue::new(task_repo.clone()));
    let result_repo = Arc::new(ScrapeResultRepositoryImpl::new(db_pool.clone()));
    let crawl_repo = Arc::new(CrawlRepositoryImpl::new(db_pool.clone()));
    let _webhook_event_repo = Arc::new(WebhookEventRepoImpl::new(db_pool.clone()));
    let webhook_repo = Arc::new(WebhookRepoImpl::new(db_pool.clone()));
    let geo_restriction_repo = Arc::new(DatabaseGeoRestrictionRepository::new(db_pool.clone()));

    let reqwest_engine = Arc::new(ReqwestEngine);
    let playwright_engine = Arc::new(PlaywrightEngine);
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine, playwright_engine];
    let router = Arc::new(EngineRouter::new(engines));

    let _create_scrape_use_case = Arc::new(CreateScrapeUseCase::new(router.clone()));
    let _robots_checker = Arc::new(RobotsChecker::new(Some(Arc::new(redis_client.clone()))));

    let mut search_engines: Vec<Arc<dyn SearchEngine>> = Vec::new();
    search_engines.push(Arc::new(GoogleSearchEngine::new()));
    let search_engine_service: Arc<dyn SearchEngine> =
        Arc::new(SearchAggregator::new(search_engines, 10000));

    let geolocation_service = GeoLocationService::new();
    let team_service = Arc::new(TeamService::new(
        geolocation_service,
        geo_restriction_repo.clone(),
    ));

    let mut settings = Settings::new().unwrap();
    settings.rate_limiting.enabled = enable_rate_limiting;
    let settings = Arc::new(settings);

    let auth_state = AuthState {
        db: db_pool.clone(),
        team_id: Uuid::nil(),
    };

    let mut app = routes::routes();

    if enable_distributed_rate_limiting {
        app = app.layer(axum::middleware::from_fn_with_state(
            rate_limiter.clone(),
            distributed_rate_limit_middleware,
        ));
    }

    let app = app
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            auth_middleware,
        ))
        .layer(Extension(queue))
        .layer(Extension(task_repo.clone()))
        .layer(Extension(rate_limiting_service))
        .layer(Extension(crawl_repo))
        .layer(Extension(credits_repo))
        .layer(Extension(result_repo))
        .layer(Extension(webhook_repo))
        .layer(Extension(geo_restriction_repo.clone()))
        .layer(Extension(redis_client.clone()))
        .layer(Extension(rate_limiter))
        .layer(Extension(settings))
        .layer(Extension(search_engine_service))
        .layer(Extension(team_service))
        .layer(axum::middleware::from_fn(
            |mut req: axum::extract::Request, next: axum::middleware::Next| async move {
                req.extensions_mut().insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                    [127, 0, 0, 1],
                    8080,
                ))));
                next.run(req).await
            },
        ));

    let server = TestServer::new(app).unwrap();

    TestApp {
        server,
        db_pool,
        api_key,
        team_id,
        task_repo,
        worker_manager: None,
        redis_process: Some(redis_process),
        redis_url,
        redis: Arc::new(redis_client),
    }
}

pub async fn create_test_app() -> TestApp {
    setup_test_app_internal(true, true, None).await
}

pub async fn create_test_app_with_rate_limit_options(
    enable_rate_limiting: bool,
    enable_distributed_rate_limiting: bool,
) -> TestApp {
    setup_test_app_internal(enable_rate_limiting, enable_distributed_rate_limiting, None).await
}

pub async fn create_test_app_with_low_rate_limit() -> TestApp {
    setup_test_app_internal(true, true, Some(1)).await
}

pub async fn create_test_app_no_worker() -> TestApp {
    setup_test_app_internal(false, false, None).await
}
