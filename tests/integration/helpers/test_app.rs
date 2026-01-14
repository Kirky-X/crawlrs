// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(deprecated)]

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::response::Response;
use axum::routing::{delete, get, post, put};
use axum_test::TestServer;
use futures::future::BoxFuture;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::Layer;
use tower::Service;

#[derive(Clone)]
pub struct ConnectInfoService<S> {
    inner: S,
    addr: SocketAddr,
}

impl<S> ConnectInfoService<S>
where
    S: Service<axum::http::Request<Body>> + Clone + Send + 'static,
    S::Future: Send,
{
    fn new(inner: S, addr: SocketAddr) -> Self {
        Self { inner, addr }
    }
}

impl<S> Service<axum::http::Request<Body>> for ConnectInfoService<S>
where
    S: Service<axum::http::Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: axum::http::Request<Body>) -> Self::Future {
        let conn_info = ConnectInfo(self.addr);
        req.extensions_mut().insert(conn_info);
        let inner = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, inner);
        Box::pin(async move { inner.call(req).await })
    }
}

#[derive(Clone)]
pub struct ConnectInfoLayer {
    addr: SocketAddr,
}

impl ConnectInfoLayer {
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }
}

impl<S: Clone + Send + 'static> Layer<S> for ConnectInfoLayer
where
    S: Service<axum::http::Request<Body>>,
    S::Future: Send,
{
    type Service = ConnectInfoService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ConnectInfoService::new(inner, self.addr)
    }
}

use crawlrs::config::settings::Settings;
use crawlrs::domain::auth::ApiKeyScope;
use crawlrs::domain::repositories::task_repository::TaskRepository;
use crawlrs::domain::services::rate_limiting_service::RateLimitConfig;
use crawlrs::engines::client::fire_cdp::FireEngineCdp;
use crawlrs::engines::client::playwright::PlaywrightEngine;
use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::engines::engine_client::EngineClient;
use crawlrs::engines::traits::ScraperEngine;
use crawlrs::infrastructure::cache::redis_client::RedisClient;
use crawlrs::infrastructure::geolocation::{GeoLocationService, GeoLocationServiceTrait};
use crawlrs::infrastructure::repositories::credits_repo_impl::CreditsRepositoryImpl;
use crawlrs::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::infrastructure::repositories::tasks_backlog_repo_impl::TasksBacklogRepositoryImpl;
use crawlrs::infrastructure::services::rate_limiting_service_impl::RateLimitingConfig;
use crawlrs::infrastructure::services::rate_limiting_service_impl::RateLimitingServiceImpl;
use crawlrs::presentation::handlers;
use crawlrs::presentation::middleware::auth_middleware::AuthState;
use crawlrs::queue::task_queue::TaskQueue;
use crawlrs::search::client::baidu::BaiduSearchEngine;
use crawlrs::search::client::bing::BingSearchEngine;
use crawlrs::search::client::google::GoogleSearchEngine;
use crawlrs::search::client::sogou::SogouSearchEngine;
use crawlrs::workers::expiration_worker::ExpirationWorker;
use crawlrs::workers::scrape_worker::ScrapeWorker;
use crawlrs::workers::worker::AbstractWorker;
use crawlrs::workers::worker::Worker;
use migration::{Migrator, MigratorTrait};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement,
};
use std::process::Command;
use std::time::Duration;
use tokio::sync::broadcast;
use uuid::Uuid;

#[allow(dead_code)]
struct InMemoryQueue {
    tasks: Arc<std::sync::Mutex<Vec<crawlrs::domain::models::task::Task>>>,
    task_repo: Arc<TaskRepositoryImpl>,
}

impl InMemoryQueue {
    #[allow(dead_code)]
    fn new(task_repo: Arc<TaskRepositoryImpl>) -> Self {
        Self {
            tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            task_repo,
        }
    }
}

#[async_trait::async_trait]
impl TaskQueue for InMemoryQueue {
    #[allow(dead_code)]
    async fn enqueue(
        &self,
        task: crawlrs::domain::models::task::Task,
    ) -> Result<crawlrs::domain::models::task::Task, crawlrs::queue::task_queue::QueueError> {
        let task_repo = self.task_repo.as_ref();
        task_repo.create(&task).await?;
        let mut queue = self.tasks.lock().expect("Failed to lock task queue");
        queue.push(task.clone());
        Ok(task)
    }

    async fn dequeue(
        &self,
        _worker_id: Uuid,
    ) -> Result<Option<crawlrs::domain::models::task::Task>, crawlrs::queue::task_queue::QueueError>
    {
        let mut queue = self.tasks.lock().expect("Failed to lock task queue");
        Ok(queue.pop())
    }

    async fn complete(&self, task_id: Uuid) -> Result<(), crawlrs::queue::task_queue::QueueError> {
        let task_repo = self.task_repo.as_ref();
        task_repo.mark_completed(task_id).await?;
        Ok(())
    }

    async fn fail(&self, task_id: Uuid) -> Result<(), crawlrs::queue::task_queue::QueueError> {
        let task_repo = self.task_repo.as_ref();
        task_repo.mark_failed(task_id).await?;
        Ok(())
    }

    async fn cancel(&self, task_id: Uuid) -> Result<(), crawlrs::queue::task_queue::QueueError> {
        let task_repo = self.task_repo.as_ref();
        task_repo.mark_cancelled(task_id).await?;
        Ok(())
    }
}

pub struct TestApp {
    pub server: TestServer,
    pub api_key: String,
    pub api_key_id: uuid::Uuid,
    pub team_id: uuid::Uuid,
    pub db_pool: Arc<DatabaseConnection>,
    pub task_repo: Arc<TaskRepositoryImpl>,
    pub redis: RedisClient,
    pub redis_url: String,
    pub redis_process: Option<std::process::Child>,
    pub _shutdown_tx: Option<broadcast::Sender<()>>,
    pub _worker_join_handles: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        // Signal workers to shutdown
        if let Some(tx) = self._shutdown_tx.take() {
            let _ = tx.send(());
        }
        // Just abort handles without blocking
        for handle in self._worker_join_handles.drain(..) {
            handle.abort();
        }
        if let Some(mut process) = self.redis_process.take() {
            let _ = process.kill();
        }
    }
}

impl TestApp {
    /// 检测数据库后端类型
    fn get_db_backend(&self) -> DbBackend {
        if self.db_pool.get_database_backend() == sea_orm::DatabaseBackend::Postgres {
            DbBackend::Postgres
        } else {
            DbBackend::Sqlite
        }
    }

    /// 执行数据库插入操作（根据数据库类型自动选择语法）
    async fn execute_insert(
        &self,
        sql: &str,
        values: Vec<sea_orm::Value>,
    ) -> Result<(), sea_orm::DbErr> {
        let db_backend = self.get_db_backend();
        self.db_pool
            .execute(Statement::from_sql_and_values(db_backend, sql, values))
            .await
            .map(|_| ())
    }

    /// 插入 team 记录
    async fn insert_team(&self, team_id: Uuid, team_name: &str) -> Result<(), sea_orm::DbErr> {
        let db_backend = self.get_db_backend();
        let (sql, values) = if db_backend == DbBackend::Postgres {
            (
                "INSERT INTO teams (id, name, created_at, updated_at) VALUES ($1, $2, NOW(), NOW())",
                vec![team_id.into(), team_name.into()],
            )
        } else {
            (
                "INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, ?, datetime('now'), datetime('now'))",
                vec![team_id.into(), team_name.into()],
            )
        };
        self.execute_insert(sql, values).await
    }

    /// 插入 api_key 记录
    async fn insert_api_key(
        &self,
        api_key_id: Uuid,
        api_key: &str,
        team_id: Uuid,
    ) -> Result<(), sea_orm::DbErr> {
        let db_backend = self.get_db_backend();
        let (sql, values) = if db_backend == DbBackend::Postgres {
            (
                "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES ($1, $2, $3, NOW(), NOW())",
                vec![api_key_id.into(), api_key.into(), team_id.into()],
            )
        } else {
            (
                "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES (?, ?, ?, datetime('now'), datetime('now'))",
                vec![api_key_id.into(), api_key.into(), team_id.into()],
            )
        };
        self.execute_insert(sql, values).await
    }

    /// 插入 credits 记录
    async fn insert_credits(
        &self,
        credits_id: Uuid,
        team_id: Uuid,
        balance: i64,
    ) -> Result<(), sea_orm::DbErr> {
        let db_backend = self.get_db_backend();
        let (sql, values) = if db_backend == DbBackend::Postgres {
            (
                "INSERT INTO credits (id, team_id, balance, created_at, updated_at) VALUES ($1, $2, $3, NOW(), NOW())",
                vec![credits_id.into(), team_id.into(), balance.into()],
            )
        } else {
            (
                "INSERT INTO credits (id, team_id, balance, created_at, updated_at) VALUES (?, ?, ?, datetime('now'), datetime('now'))",
                vec![credits_id.into(), team_id.into(), balance.into()],
            )
        };
        self.execute_insert(sql, values).await
    }

    /// 创建测试团队
    pub async fn create_team(&self, team_name: &str) -> (String, uuid::Uuid) {
        let team_id = Uuid::new_v4();
        let api_key = Uuid::new_v4().to_string();
        let api_key_id = Uuid::new_v4();
        let credits_id = Uuid::new_v4();

        self.insert_team(team_id, team_name)
            .await
            .expect("Failed to insert team");
        self.insert_api_key(api_key_id, &api_key, team_id)
            .await
            .expect("Failed to insert API key");
        self.insert_credits(credits_id, team_id, 1000)
            .await
            .expect("Failed to insert credits");

        (api_key, team_id)
    }
}

pub async fn create_test_app() -> TestApp {
    create_test_app_with_rate_limit_options(true, true).await
}

pub async fn create_test_app_with_low_rate_limit() -> TestApp {
    let app = create_test_app_with_rate_limit_options(true, true).await;

    // Set a low rate limit of 1 request per minute for this API key
    // Must use the same key format as RateLimiter in rate_limit_middleware.rs: "rate_limit_config:{api_key}"
    let rate_limit_key = format!("rate_limit_config:{}", app.api_key);
    let rate_limit_value = json!({"requests_per_minute": 1, "capacity": 1});
    let _ = app
        .redis
        .set(&rate_limit_key, &rate_limit_value.to_string(), 60)
        .await;

    app
}

pub async fn create_test_app_with_rate_limit_options(
    _rate_limit_enabled: bool,
    use_redis: bool,
) -> TestApp {
    // 强制使用PostgreSQL数据库，与Worker共享
    let db_password =
        std::env::var("TEST_DATABASE_PASSWORD").unwrap_or_else(|_| "password".to_string());
    let db_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        format!(
            "postgres://crawlrs:{}@localhost:5443/crawlrs_test",
            db_password
        )
    });

    let mut opt = ConnectOptions::new(db_url.clone());
    // 增加最大连接数，防止在高并发测试时耗尽连接
    opt.max_connections(20)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(10))
        .sqlx_logging(false);

    let db = Database::connect(opt)
        .await
        .expect("Failed to connect to database");
    let db_pool = Arc::new(db);

    Migrator::up(db_pool.as_ref(), None)
        .await
        .expect("Failed to run database migrations");

    let redis_url;
    let redis_client: RedisClient;
    let redis_process: Option<std::process::Child>;

    if use_redis {
        // Use external Redis instance - try different ports if default fails
        let redis_port = std::env::var("TEST_REDIS_PORT").unwrap_or_else(|_| "6380".to_string());
        redis_url = format!("redis://127.0.0.1:{}", redis_port);
        redis_client = RedisClient::new(&redis_url)
            .await
            .expect("Failed to connect to Redis");
        redis_process = None;
    } else {
        redis_url = "redis://127.0.0.1:6380".to_string();
        redis_client = RedisClient::new(&redis_url)
            .await
            .expect("Failed to connect to Redis");
        redis_process = None;
    }

    let api_key = Uuid::new_v4().to_string();
    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();

    // 根据数据库URL确定数据库后端类型
    let db_backend = if db_url.starts_with("postgres://") {
        DbBackend::Postgres
    } else {
        DbBackend::Sqlite
    };

    // 使用适合当前数据库后端的语法插入测试数据
    if db_backend == DbBackend::Postgres {
        // PostgreSQL语法
        db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "INSERT INTO teams (id, name, created_at, updated_at) VALUES ($1, 'test-team', NOW(), NOW())",
                vec![team_id.into()],
            ))
            .await
            .unwrap();

        db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES ($1, $2, $3, NOW(), NOW())",
                vec![api_key_id.into(), api_key.clone().into(), team_id.into()],
            ))
            .await
            .unwrap();

        db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "INSERT INTO credits (id, team_id, balance, created_at, updated_at) VALUES ($1, $2, 1000, NOW(), NOW())",
                vec![Uuid::new_v4().into(), team_id.into()],
            ))
            .await
            .unwrap();
    } else {
        // SQLite语法
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
    }

    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_pool.clone(),
        chrono::Duration::seconds(300),
    ));
    let credits_repo = Arc::new(CreditsRepositoryImpl::new(db_pool.clone()));
    let backlog_repo = Arc::new(TasksBacklogRepositoryImpl::new(db_pool.clone()));

    let rate_limiting_service: Arc<
        dyn crawlrs::domain::services::rate_limiting_service::RateLimitingService,
    > = Arc::new(RateLimitingServiceImpl::new(
        Arc::new(redis_client.clone()),
        task_repo.clone(),
        backlog_repo.clone(),
        credits_repo.clone(),
        RateLimitingConfig {
            rate_limit: RateLimitConfig {
                requests_per_minute: 1000,
                requests_per_second: 1000,
                bucket_capacity: Some(1000),
                ..RateLimitConfig::default()
            },
            ..RateLimitingConfig::default()
        },
    ));

    let reqwest_engine = Arc::new(ReqwestEngine::new());
    let playwright_engine = Arc::new(PlaywrightEngine);
    let fire_engine = Arc::new(FireEngineCdp::new());
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine, playwright_engine, fire_engine];
    let router = Arc::new(crawlrs::engines::router::EngineRouter::new(engines));
    let engine_client = Arc::new(EngineClient::with_router(router.clone()));

    let robots_checker = Arc::new(crawlrs::utils::robots::RobotsChecker::new(Some(Arc::new(
        redis_client.clone(),
    ))));

    let search_engine_service: Arc<dyn crawlrs::search::engine_trait::SearchEngine> =
        Arc::new(crawlrs::search::aggregator::SearchAggregator::new(
            vec![
                Arc::new(GoogleSearchEngine::new(engine_client.clone())),
                Arc::new(BingSearchEngine::new()),
                Arc::new(BaiduSearchEngine::new()),
                Arc::new(SogouSearchEngine::new()),
            ],
            10000,
        ));

    let geo_location_service =
        Arc::new(GeoLocationService::new()) as Arc<dyn GeoLocationServiceTrait>;
    let geo_restriction_repo = Arc::new(DatabaseGeoRestrictionRepository::new(db_pool.clone()));
    let team_service = Arc::new(crawlrs::domain::services::team_service::TeamService::new(
        geo_location_service,
        geo_restriction_repo.clone(),
    ));

    let rate_limiter = Arc::new(
        crawlrs::presentation::middleware::rate_limit_middleware::RateLimiter::new(
            redis_client.clone(),
            1000,
        ),
    );

    let app = create_router(
        db_pool.clone(),
        task_repo.clone(),
        backlog_repo.clone(),
        credits_repo.clone(),
        rate_limiting_service,
        router.clone(),
        robots_checker.clone(),
        search_engine_service,
        geo_restriction_repo,
        team_service,
        rate_limiter,
        team_id,
        Arc::new(redis_client.clone()),
        api_key.clone(),
        _rate_limit_enabled,
    );

    let mock_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let app = app.layer(ConnectInfoLayer::new(mock_addr));
    let server = TestServer::new(app).unwrap();

    // 启动 WorkerManager 来处理任务
    let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);
    let crawl_repo = Arc::new(
        crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl::new(
            db_pool.clone(),
        ),
    );
    let result_repo = Arc::new(
        crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl::new(db_pool.clone())
    );
    let webhook_event_repo = Arc::new(
        crawlrs::infrastructure::repositories::webhook_event_repo_impl::WebhookEventRepoImpl::new(
            db_pool.clone(),
        ),
    );
    let storage_repo: Option<
        Arc<dyn crawlrs::domain::repositories::storage_repository::StorageRepository + Send + Sync>,
    > = None;
    let settings = Arc::new(Settings::new().unwrap());

    // 创建 LLM 服务（用于提取功能）
    let llm_service = Box::new(crawlrs::domain::services::llm_service::LLMService::new(
        &settings,
    ));
    let _extraction_service = Arc::new(
        crawlrs::domain::services::extraction_service::ExtractionService::new(llm_service),
    );
    let create_scrape_use_case = Arc::new(
        crawlrs::application::use_cases::create_scrape::CreateScrapeUseCase::new(router.clone()),
    );

    // 创建 Webhook 服务
    let webhook_secret =
        std::env::var("TEST_WEBHOOK_SECRET").unwrap_or_else(|_| "test-secret".to_string());
    let _webhook_service = Arc::new(
        crawlrs::infrastructure::services::webhook_service_impl::WebhookServiceImpl::new(
            webhook_secret,
        ),
    );

    let queue: Arc<dyn TaskQueue> = Arc::new(crawlrs::queue::task_queue::PostgresTaskQueue::new(
        task_repo.clone(),
    ));

    // 启动 worker（在后台）- 直接 spawn workers 而不是通过 start_workers
    let worker_task_repo = task_repo.clone();
    let worker_result_repo = result_repo.clone();
    let worker_crawl_repo = crawl_repo.clone();
    let worker_webhook_event_repo = webhook_event_repo.clone();
    let worker_credits_repo = credits_repo.clone();
    let worker_router = router.clone();
    let worker_create_scrape_use_case = create_scrape_use_case.clone();
    let worker_redis = redis_client.clone();
    let worker_robots_checker = robots_checker.clone();
    let worker_settings = settings.clone();
    let worker_queue = queue.clone();
    let worker_default_concurrency_limit = 5;

    // Clone storage_repo for worker
    let worker_storage_repo = storage_repo.clone();

    let worker_join_handle = tokio::spawn(async move {
        // Spawn workers directly
        for i in 0..2 {
            let queue = worker_queue.clone();
            let task_repo = worker_task_repo.clone();
            let result_repo = worker_result_repo.clone();
            let crawl_repo = worker_crawl_repo.clone();
            let storage_repo = worker_storage_repo.clone();
            let webhook_event_repo = worker_webhook_event_repo.clone();
            let credits_repo = worker_credits_repo.clone();
            let router = worker_router.clone();
            let create_scrape_use_case = worker_create_scrape_use_case.clone();
            let redis = worker_redis.clone();
            let robots_checker = worker_robots_checker.clone();
            let settings = worker_settings.clone();

            tokio::spawn(async move {
                // Use default engine client which will eventually be replaced by the one from factory
                let engine_client = Arc::new(EngineClient::with_router(router));
                let worker = ScrapeWorker::new(
                    task_repo,
                    result_repo,
                    crawl_repo,
                    storage_repo,
                    webhook_event_repo,
                    credits_repo,
                    engine_client,
                    create_scrape_use_case,
                    redis,
                    robots_checker,
                    settings,
                    worker_default_concurrency_limit,
                );
                eprintln!("DEBUG: Worker {} started", i);
                worker.run(queue).await;
            });
        }

        // Also start expiration worker
        let expiration_worker = Arc::new(ExpirationWorker::new(worker_task_repo.clone()));
        let expiration_worker = Arc::new(AbstractWorker::new(
            expiration_worker,
            Duration::from_secs(60),
        ));
        let exp_worker_clone = expiration_worker.clone();
        tokio::spawn(async move {
            exp_worker_clone.run().await;
        });

        // Wait for shutdown signal
        shutdown_rx.recv().await.ok();
        tracing::info!("Test worker manager workers completed");
    });

    TestApp {
        server,
        api_key,
        api_key_id,
        team_id,
        db_pool,
        task_repo: task_repo.clone(),
        redis: redis_client,
        redis_url,
        redis_process,
        _shutdown_tx: Some(shutdown_tx),
        _worker_join_handles: vec![worker_join_handle],
    }
}

#[allow(clippy::too_many_arguments)]
fn create_router(
    db_pool: Arc<DatabaseConnection>,
    task_repo: Arc<TaskRepositoryImpl>,
    backlog_repo: Arc<TasksBacklogRepositoryImpl>,
    credits_repo: Arc<CreditsRepositoryImpl>,
    rate_limiting_service: Arc<
        dyn crawlrs::domain::services::rate_limiting_service::RateLimitingService,
    >,
    router: Arc<crawlrs::engines::router::EngineRouter>,
    robots_checker: Arc<crawlrs::utils::robots::RobotsChecker>,
    search_engine_service: Arc<dyn crawlrs::search::SearchEngine>,
    geo_restriction_repo: Arc<DatabaseGeoRestrictionRepository>,
    team_service: Arc<crawlrs::domain::services::team_service::TeamService>,
    rate_limiter: Arc<crawlrs::presentation::middleware::rate_limit_middleware::RateLimiter>,
    team_id: Uuid,
    redis_client: Arc<RedisClient>,
    api_key: String,
    rate_limit_enabled: bool,
) -> axum::Router<()> {
    let crawl_repo = Arc::new(
        crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl::new(
            db_pool.clone(),
        ),
    );
    let result_repo = Arc::new(crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl::new(db_pool.clone()));
    let webhook_repo = Arc::new(
        crawlrs::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl::new(
            db_pool.clone(),
        ),
    );
    let webhook_event_repo = Arc::new(
        crawlrs::infrastructure::repositories::webhook_event_repo_impl::WebhookEventRepoImpl::new(
            db_pool.clone(),
        ),
    );
    let settings = Arc::new(Settings::new().unwrap());
    let queue: Arc<dyn TaskQueue> = Arc::new(crawlrs::queue::task_queue::PostgresTaskQueue::new(
        task_repo.clone(),
    ));
    let auth_state = AuthState {
        db: db_pool.clone(),
        auth_scope_service: None,
        team_id,
        api_key_id: Uuid::new_v4(),
        scope: ApiKeyScope::default(),
    };

    let public_routes = axum::Router::new()
        .route("/health", get(crawlrs::presentation::routes::health_check))
        .route(
            "/metrics",
            get(crawlrs::presentation::handlers::metrics_handler::metrics),
        )
        .route("/v1/version", get(crawlrs::presentation::routes::version));

    let team_semaphore = Arc::new(tokio::sync::Semaphore::new(100));

    let mut protected_routes = axum::Router::new()
        .route(
            "/v1/scrape",
            post(handlers::scrape_handler::create_scrape),
        )
        .route(
            "/v1/scrape/{id}",
            get(handlers::scrape_handler::get_scrape_status),
        )
        .route(
            "/v1/scrape/{id}",
            delete(handlers::scrape_handler::cancel_scrape),
        )
        .route(
            "/v1/extract",
            post(handlers::extract_handler::extract::<DatabaseGeoRestrictionRepository>),
        )
        .route(
            "/v1/webhooks",
            post(handlers::webhook_handler::create_webhook::<crawlrs::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl>),
        )
        .route(
            "/v1/crawl",
            post(handlers::crawl_handler::create_crawl::<
                crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl,
                crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl,
                crawlrs::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl,
                crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl,
                crawlrs::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository,
            >),
        )
        .route(
            "/v1/crawl/{id}",
            get(handlers::crawl_handler::get_crawl::<
                crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl,
                crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl,
                crawlrs::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl,
                crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl,
                crawlrs::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository,
            >),
        )
        .route(
            "/v1/crawl/{id}/results",
            get(handlers::crawl_handler::get_crawl_results::<
                crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl,
                crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl,
                crawlrs::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl,
                crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl,
                crawlrs::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository,
            >),
        )
        .route(
            "/v1/crawl/{id}",
            delete(handlers::crawl_handler::cancel_crawl::<
                crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl,
                crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl,
                crawlrs::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl,
                crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl,
                crawlrs::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository,
            >),
        )
        .route(
            "/v1/search",
            post(handlers::search_handler::search::<
                crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl,
                crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl,
                crawlrs::infrastructure::repositories::credits_repo_impl::CreditsRepositoryImpl,
            >),
        )
        .route(
            "/v1/teams/geo-restrictions",
            get(handlers::team_handler::get_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        )
        .route(
            "/v1/teams/geo-restrictions",
            put(handlers::team_handler::update_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        )
        .layer(axum::middleware::from_fn_with_state(
            auth_state.clone(),
            crawlrs::presentation::middleware::auth_middleware::auth_middleware,
        ));

    if rate_limit_enabled {
        protected_routes = protected_routes.layer(axum::middleware::from_fn_with_state(
            rate_limiter.clone(),
            crawlrs::presentation::middleware::distributed_rate_limit_middleware::distributed_rate_limit_middleware,
        ));
    }

    axum::Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(axum::Extension(db_pool))
        .layer(axum::Extension(task_repo))
        .layer(axum::Extension(backlog_repo))
        .layer(axum::Extension(credits_repo))
        .layer(axum::Extension(rate_limiting_service))
        .layer(axum::Extension(router))
        .layer(axum::Extension(robots_checker))
        .layer(axum::Extension(search_engine_service))
        .layer(axum::Extension(geo_restriction_repo))
        .layer(axum::Extension(team_service))
        .layer(axum::Extension(crawl_repo))
        .layer(axum::Extension(result_repo))
        .layer(axum::Extension(webhook_repo))
        .layer(axum::Extension(webhook_event_repo))
        .layer(axum::Extension(settings))
        .layer(axum::Extension(team_semaphore))
        .layer(axum::Extension(queue))
        .layer(axum::Extension(auth_state))
        .layer(axum::Extension(redis_client))
        .layer(axum::Extension(rate_limiter))
}

pub async fn create_test_app_no_worker() -> TestApp {
    // 强制使用PostgreSQL数据库，与Worker共享
    let db_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        let db_password =
            std::env::var("TEST_DATABASE_PASSWORD").unwrap_or_else(|_| "password".to_string());
        format!(
            "postgres://crawlrs:{}@localhost:5443/crawlrs_test",
            db_password
        )
    });
    let db = Database::connect(&db_url).await.unwrap();
    let db_pool = Arc::new(db);

    Migrator::up(db_pool.as_ref(), None).await.unwrap();

    // 清理测试相关的表数据
    let db_backend = if db_url.starts_with("postgres://") {
        DbBackend::Postgres
    } else {
        DbBackend::Sqlite
    };

    if db_backend == DbBackend::Postgres {
        // PostgreSQL语法 - 清理所有任务和相关数据
        db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "DELETE FROM tasks",
                vec![],
            ))
            .await
            .unwrap();

        db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "DELETE FROM tasks_backlog",
                vec![],
            ))
            .await
            .unwrap();

        db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "DELETE FROM scrape_results",
                vec![],
            ))
            .await
            .unwrap();
    } else {
        // SQLite语法 - 清理所有任务和相关数据
        db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM tasks",
                vec![],
            ))
            .await
            .unwrap();

        db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM tasks_backlog",
                vec![],
            ))
            .await
            .unwrap();

        db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM scrape_results",
                vec![],
            ))
            .await
            .unwrap();
    }

    let start_port = 8000;
    let result =
        crawlrs::utils::port_sniffer::PortSniffer::find_available_port(start_port, true, 100)
            .unwrap();
    let redis_port = result.port;
    eprintln!("DEBUG: Starting Redis on port {}", redis_port);
    let redis_process = Some(
        Command::new("redis-server")
            .arg("--port")
            .arg(redis_port.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("Failed to start redis-server"),
    );

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    let redis_url = format!("redis://127.0.0.1:{}", redis_port);
    eprintln!("DEBUG: Connecting to Redis at {}", redis_url);
    let redis_client = RedisClient::new(&redis_url).await.unwrap();
    eprintln!("DEBUG: Redis connection established");

    let api_key = Uuid::new_v4().to_string();
    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();

    // 根据数据库URL确定数据库后端类型
    let db_backend = if db_url.starts_with("postgres://") {
        DbBackend::Postgres
    } else {
        DbBackend::Sqlite
    };

    // 使用适合当前数据库后端的语法插入测试数据
    if db_backend == DbBackend::Postgres {
        // PostgreSQL语法
        db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "INSERT INTO teams (id, name, created_at, updated_at) VALUES ($1, 'test-team', NOW(), NOW())",
                vec![team_id.into()],
            ))
            .await
            .unwrap();

        db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES ($1, $2, $3, NOW(), NOW())",
                vec![api_key_id.into(), api_key.clone().into(), team_id.into()],
            ))
            .await
            .unwrap();

        db_pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "INSERT INTO credits (id, team_id, balance, created_at, updated_at) VALUES ($1, $2, 1000, NOW(), NOW())",
                vec![Uuid::new_v4().into(), team_id.into()],
            ))
            .await
            .unwrap();
    } else {
        // SQLite语法
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
    }

    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_pool.clone(),
        chrono::Duration::seconds(300),
    ));
    let credits_repo = Arc::new(CreditsRepositoryImpl::new(db_pool.clone()));
    let backlog_repo = Arc::new(TasksBacklogRepositoryImpl::new(db_pool.clone()));

    let rate_limiting_service: Arc<
        dyn crawlrs::domain::services::rate_limiting_service::RateLimitingService,
    > = Arc::new(RateLimitingServiceImpl::new(
        Arc::new(redis_client.clone()),
        task_repo.clone(),
        backlog_repo.clone(),
        credits_repo.clone(),
        RateLimitingConfig::default(),
    ));

    let reqwest_engine = Arc::new(ReqwestEngine::new());
    let playwright_engine = Arc::new(PlaywrightEngine);
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine, playwright_engine];
    let router = Arc::new(crawlrs::engines::router::EngineRouter::new(engines));

    let robots_checker = Arc::new(crawlrs::utils::robots::RobotsChecker::new(Some(Arc::new(
        redis_client.clone(),
    ))));

    // 创建搜索引擎实例
    let _google_engine_client = Arc::new(EngineClient::default());
    let google_engine_client = Arc::new(EngineClient::default());
    let search_engine_service: Arc<dyn crawlrs::search::engine_trait::SearchEngine> =
        Arc::new(crawlrs::search::aggregator::SearchAggregator::new(
            vec![
                Arc::new(GoogleSearchEngine::new(google_engine_client)),
                Arc::new(BingSearchEngine::new()),
                Arc::new(BaiduSearchEngine::new()),
                Arc::new(SogouSearchEngine::new()),
            ],
            10000,
        ));

    let geo_location_service =
        Arc::new(GeoLocationService::new()) as Arc<dyn GeoLocationServiceTrait>;
    let geo_restriction_repo = Arc::new(DatabaseGeoRestrictionRepository::new(db_pool.clone()));
    let team_service = Arc::new(crawlrs::domain::services::team_service::TeamService::new(
        geo_location_service,
        geo_restriction_repo.clone(),
    ));

    let rate_limiter = Arc::new(
        crawlrs::presentation::middleware::rate_limit_middleware::RateLimiter::new(
            redis_client.clone(),
            1000,
        ),
    );

    let app = create_router(
        db_pool.clone(),
        task_repo.clone(),
        backlog_repo.clone(),
        credits_repo,
        rate_limiting_service,
        router,
        robots_checker,
        search_engine_service,
        geo_restriction_repo,
        team_service,
        rate_limiter,
        team_id,
        Arc::new(redis_client.clone()),
        api_key.clone(),
        true,
    );

    let mock_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let app = app.layer(ConnectInfoLayer::new(mock_addr));
    let server = TestServer::new(app).unwrap();

    TestApp {
        server,
        api_key,
        api_key_id,
        team_id,
        db_pool,
        task_repo: task_repo.clone(),
        redis: redis_client,
        redis_url,
        redis_process,
        _shutdown_tx: None,
        _worker_join_handles: vec![],
    }
}
