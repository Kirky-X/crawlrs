// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(dead_code)]
#![allow(deprecated)]

/// TestApp 测试固件
///
/// 提供测试应用程序的创建和管理功能
use crate::common::fixtures::database::DatabaseFixture;
use crate::common::fixtures::database::DatabaseOptions;
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
use uuid::Uuid;

use crawlrs::config::settings::Settings;
use crawlrs::engines::client::playwright::PlaywrightEngine;
use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::engines::engine_client::EngineClient;
use crawlrs::engines::traits::ScraperEngine;
use crawlrs::infrastructure::cache::redis_client::RedisClient;
use crawlrs::infrastructure::geolocation::GeoLocationService;
use crawlrs::infrastructure::repositories::credits_repo_impl::CreditsRepositoryImpl;
use crawlrs::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::infrastructure::repositories::tasks_backlog_repo_impl::TasksBacklogRepositoryImpl;
use crawlrs::infrastructure::services::rate_limiting_service_impl::RateLimitingConfig;
use crawlrs::infrastructure::services::rate_limiting_service_impl::RateLimitingServiceImpl;
use crawlrs::presentation::handlers;
use crawlrs::presentation::middleware::auth_middleware::AuthState;
use crawlrs::search::client::baidu::BaiduSearchEngine;
use crawlrs::search::client::bing::BingSearchEngine;
use crawlrs::search::client::google::GoogleSearchEngine;
use crawlrs::search::client::sogou::SogouSearchEngine;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

// === ConnectInfoService ===

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

// === TestApp ===

pub struct TestApp {
    pub server: TestServer,
    pub api_key: String,
    pub team_id: uuid::Uuid,
    pub db_pool: Arc<DatabaseConnection>,
    pub task_repo: Arc<TaskRepositoryImpl>,
    pub redis: RedisClient,
    pub redis_url: String,
}

impl TestApp {
    pub async fn create_team(&self, team_name: &str) -> (String, uuid::Uuid) {
        let team_id = Uuid::new_v4();
        let api_key = Uuid::new_v4().to_string();

        let db_backend =
            if self.db_pool.get_database_backend() == sea_orm::DatabaseBackend::Postgres {
                DbBackend::Postgres
            } else {
                DbBackend::Sqlite
            };

        if db_backend == DbBackend::Postgres {
            self.db_pool
                .execute(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "INSERT INTO teams (id, name, created_at, updated_at) VALUES ($1, $2, NOW(), NOW())",
                    vec![team_id.into(), team_name.into()],
                ))
                .await
                .expect("Failed to insert team");
        } else {
            self.db_pool
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, ?, datetime('now'), datetime('now'))",
                    vec![team_id.into(), team_name.into()],
                ))
                .await
                .expect("Failed to insert team");
        }

        (api_key, team_id)
    }
}

/// TestApp 固件选项
#[derive(Debug, Clone)]
pub struct TestAppOptions {
    /// 速率限制是否启用
    pub rate_limit_enabled: bool,
    /// 是否使用Redis
    pub use_redis: bool,
    /// Redis端口
    pub redis_port: u16,
    /// 数据库选项
    pub database_options: DatabaseOptions,
}

impl Default for TestAppOptions {
    fn default() -> Self {
        Self {
            rate_limit_enabled: true,
            use_redis: true,
            redis_port: 6381,
            database_options: DatabaseOptions::default(),
        }
    }
}

/// TestApp 固件
pub struct TestAppFixture {
    /// TestApp 实例
    pub app: TestApp,
    /// 数据库固件
    pub db_fixture: DatabaseFixture,
}

impl TestAppFixture {
    /// 创建新的 TestApp 固件（使用默认配置）
    pub async fn new() -> Self {
        Self::with_options(TestAppOptions::default()).await
    }

    /// 使用指定选项创建 TestApp 固件
    pub async fn with_options(options: TestAppOptions) -> Self {
        let db_fixture = DatabaseFixture::with_options(options.database_options.clone()).await;
        let db_pool = db_fixture.db_pool.clone();
        let db_backend = db_fixture.db_backend;

        let redis_url = if options.use_redis {
            format!("redis://127.0.0.1:{}", options.redis_port)
        } else {
            "redis://127.0.0.1:6379".to_string()
        };
        let redis_client = RedisClient::new(&redis_url)
            .await
            .expect("Failed to create Redis client");

        let api_key = Uuid::new_v4().to_string();
        let team_id = Uuid::new_v4();

        // 插入测试数据
        match db_backend {
            DbBackend::Postgres => {
                db_pool
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Postgres,
                        "INSERT INTO teams (id, name, created_at, updated_at) VALUES ($1, 'test-team', NOW(), NOW())",
                        vec![team_id.into()],
                    ))
                    .await
                    .expect("Failed to insert team");

                db_pool
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Postgres,
                        "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES ($1, $2, $3, NOW(), NOW())",
                        vec![Uuid::new_v4().into(), api_key.clone().into(), team_id.into()],
                    ))
                    .await
                    .expect("Failed to insert API key");

                db_pool
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Postgres,
                        "INSERT INTO credits (id, team_id, balance, created_at, updated_at) VALUES ($1, $2, 1000, NOW(), NOW())",
                        vec![Uuid::new_v4().into(), team_id.into()],
                    ))
                    .await
                    .expect("Failed to insert credits");
            }
            DbBackend::Sqlite => {
                db_pool
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, 'test-team', datetime('now'), datetime('now'))",
                        vec![team_id.into()],
                    ))
                    .await
                    .expect("Failed to insert team");

                db_pool
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES (?, ?, ?, datetime('now'), datetime('now'))",
                        vec![Uuid::new_v4().into(), api_key.clone().into(), team_id.into()],
                    ))
                    .await
                    .expect("Failed to insert API key");

                db_pool
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "INSERT INTO credits (id, team_id, balance, created_at, updated_at) VALUES (?, ?, 1000, datetime('now'), datetime('now'))",
                        vec![Uuid::new_v4().into(), team_id.into()],
                    ))
                    .await
                    .expect("Failed to insert credits");
            }
            _ => {}
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

        let engines_for_client: Vec<Arc<dyn ScraperEngine>> =
            vec![reqwest_engine.clone(), playwright_engine.clone()];
        let engine_client = Arc::new(EngineClient::with_engines(engines_for_client));

        let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine, playwright_engine];
        let router = Arc::new(crawlrs::engines::router::EngineRouter::new(engines));

        let robots_checker = Arc::new(crawlrs::utils::robots::RobotsChecker::new(Some(Arc::new(
            redis_client.clone(),
        ))));

        let search_engine_service: Arc<dyn crawlrs::search::engine_trait::SearchEngine> =
            Arc::new(crawlrs::search::aggregator::SearchAggregator::new(
                vec![
                    Arc::new(GoogleSearchEngine::new(engine_client)),
                    Arc::new(BingSearchEngine::new()),
                    Arc::new(BaiduSearchEngine::new()),
                    Arc::new(SogouSearchEngine::new()),
                ],
                10000,
            ));

        let geo_location_service = GeoLocationService::new();
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
            options.rate_limit_enabled,
        );

        let mock_addr: SocketAddr = "127.0.0.1:8080"
            .parse()
            .expect("Failed to parse socket address");
        let app = app.layer(ConnectInfoLayer::new(mock_addr));
        let server = TestServer::new(app).expect("Failed to create test server");
        let app = TestApp {
            server,
            api_key,
            team_id,
            db_pool,
            task_repo: task_repo.clone(),
            redis: redis_client,
            redis_url,
        };

        Self { app, db_fixture }
    }

    /// 创建低速率限制的 TestApp
    pub async fn with_low_rate_limit() -> Self {
        let fixture = Self::with_options(TestAppOptions {
            rate_limit_enabled: true,
            use_redis: true,
            redis_port: 6381,
            database_options: DatabaseOptions::default(),
        })
        .await;

        // 设置低速率限制：每分钟1个请求
        let rate_limit_key = format!("rate_limit_config:{}", fixture.app.api_key);
        let rate_limit_value = json!({"requests_per_minute": 1, "capacity": 1});
        let _ = fixture
            .app
            .redis
            .set(&rate_limit_key, &rate_limit_value.to_string(), 60)
            .await;

        fixture
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
    search_engine_service: Arc<dyn crawlrs::search::engine_trait::SearchEngine>,
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
    let settings = Arc::new(Settings::new().expect("Failed to load settings"));
    let queue: Arc<dyn crawlrs::queue::task_queue::TaskQueue> = Arc::new(
        crawlrs::queue::task_queue::PostgresTaskQueue::new(task_repo.clone()),
    );
    let auth_state = AuthState {
        db: db_pool.clone(),
        auth_scope_service: None,
        team_id,
        api_key_id: uuid::Uuid::nil(),
        scope: crawlrs::domain::auth::ApiKeyScope::default(),
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
        .layer(axum::Extension(team_id))
        .layer(axum::Extension(api_key))
        .layer(axum::Extension(redis_client))
        .layer(axum::Extension(rate_limiter))
}
