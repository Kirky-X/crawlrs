// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::Extension;
use axum::{
    routing::{delete, get, post},
    Router,
};
use crawlrs::config::settings::Settings;
use crawlrs::domain::services::rate_limiting_service::{RateLimitConfig, RateLimitStrategy};
use crawlrs::engines::fire_engine_cdp::FireEngineCdp;
use crawlrs::engines::fire_engine_tls::FireEngineTls;
use crawlrs::engines::playwright_engine::PlaywrightEngine;
use crawlrs::engines::reqwest_engine::ReqwestEngine;
use crawlrs::engines::router::EngineRouter;
use crawlrs::engines::traits::ScraperEngine;
use crawlrs::infrastructure::cache::redis_client::RedisClient;
use crawlrs::infrastructure::database::connection;
use crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl;
use crawlrs::infrastructure::repositories::credits_repo_impl::CreditsRepositoryImpl;
use crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::infrastructure::storage::LocalStorage;

use crawlrs::domain::services::rate_limiting_service::{ConcurrencyConfig, ConcurrencyStrategy};
use crawlrs::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crawlrs::infrastructure::repositories::tasks_backlog_repo_impl::TasksBacklogRepositoryImpl;
use crawlrs::infrastructure::repositories::webhook_event_repo_impl::WebhookEventRepoImpl;
use crawlrs::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl;
use crawlrs::infrastructure::services::rate_limiting_service_impl::{
    RateLimitingConfig, RateLimitingServiceImpl,
};
use crawlrs::infrastructure::services::webhook_service_impl::WebhookServiceImpl;
use crawlrs::presentation::handlers::{
    crawl_handler, extract_handler, scrape_handler, search_handler, webhook_handler,
};
use crawlrs::presentation::middleware::auth_middleware::AuthState;
use crawlrs::presentation::middleware::rate_limit_middleware::RateLimiter;
use crawlrs::presentation::middleware::team_semaphore::TeamSemaphore;
use crawlrs::presentation::middleware::team_semaphore_middleware::team_semaphore_middleware;
use crawlrs::presentation::routes;
use crawlrs::presentation::routes::task::task_routes;
use crawlrs::queue::task_queue::{PostgresTaskQueue, TaskQueue};
use crawlrs::utils::retry_policy::RetryPolicy;
use crawlrs::workers::backlog_worker::BacklogWorker;
use crawlrs::workers::manager::WorkerManager;
use crawlrs::workers::webhook_worker::WebhookWorker;
use crawlrs::workers::Worker;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

use crawlrs::domain::search::engine::SearchEngine;
use crawlrs::infrastructure::search::aggregator::SearchAggregator;

use crawlrs::infrastructure::search::smart_search;

use crawlrs::utils::telemetry;
use migration::{Migrator, MigratorTrait};

/// 主函数
///
/// 应用程序入口点，负责初始化所有组件并启动服务
///
/// # 参数
///
/// 无
///
/// # 返回值
///
/// 返回 `anyhow::Result<()>`，成功时返回 Ok(())，失败时返回错误
///
/// # 功能
///
/// 1. 初始化日志和遥测系统
/// 2. 加载应用程序配置
/// 3. 建立数据库连接并运行迁移
/// 4. 初始化 Redis 客户端
/// 5. 设置速率限制器
/// 6. 初始化团队信号量
/// 7. 创建和配置所有组件（仓库、队列、存储、引擎等）
/// 8. 启动工作器进程
/// 9. 配置 HTTP 路由和中间件
/// 10. 启动 HTTP 服务器
///
/// # 错误
///
/// 可能在以下情况下返回错误：
/// - 配置加载失败
/// - 数据库连接失败
/// - 数据库迁移失败
/// - Redis 连接失败
/// - HTTP 服务器启动失败
///
/// # 示例
///
/// ```rust
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     // 调用主函数逻辑
///     crawlrs::main().await
/// }
/// ```
use tracing::error;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 初始化日志和遥测系统
    telemetry::init_telemetry();
    info!("Starting crawlrs...");

    // 初始化 Prometheus 指标收集
    crawlrs::infrastructure::metrics::init_metrics();

    // 2. 加载应用程序配置
    let mut settings = Settings::new()?;
    info!("Configuration loaded");

    // 端口嗅探
    let port_result = crawlrs::utils::port_sniffer::PortSniffer::find_available_port(
        settings.server.port,
        settings.server.enable_port_detection,
    );

    match port_result {
        Ok(result) => {
            if result.port != settings.server.port {
                info!(
                    "默认端口 {} 被占用，切换到端口 {}",
                    settings.server.port, result.port
                );
                settings.server.port = result.port;
            }
            for log in result.logs {
                info!("{}", log);
            }
        }
        Err(e) => {
            error!("端口检测失败: {}", e);
            return Err(anyhow::anyhow!("Failed to find available port: {}", e));
        }
    }

    let settings = Arc::new(settings);

    // 初始化搜索引擎将在EngineRouter创建之后进行

    // 3. 建立数据库连接
    let db = connection::create_pool(&settings.database).await?;
    let db = Arc::new(db);
    info!("Database connection established");

    // 运行数据库迁移
    info!("Running database migrations...");
    Migrator::up(db.as_ref(), None).await?;
    info!("Database migrations applied");

    // 4. 初始化 Redis 客户端
    let redis_client = Arc::new(RedisClient::new(&settings.redis.url).await?);
    info!("Redis client initialized");

    // 5. 初始化速率限制器
    let rate_limiter = Arc::new(RateLimiter::new(
        (*redis_client).clone(),
        settings.rate_limiting.default_rpm,
    ));
    info!("Rate limiter initialized");

    // 6. 初始化团队信号量
    let team_semaphore = Arc::new(TeamSemaphore::new(
        settings.concurrency.default_team_limit as usize,
    ));
    info!("Team semaphore initialized");

    // 7. 初始化核心组件
    let _credits_repo = Arc::new(CreditsRepositoryImpl::new(db.clone()));
    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db.clone(),
        chrono::Duration::seconds(settings.concurrency.task_lock_duration_seconds),
    ));
    let _tasks_backlog_repo = Arc::new(TasksBacklogRepositoryImpl::new(db.clone()));
    let queue: Arc<dyn TaskQueue> = Arc::new(PostgresTaskQueue::new(task_repo.clone()));
    let result_repo = Arc::new(ScrapeResultRepositoryImpl::new(db.clone()));
    let crawl_repo = Arc::new(CrawlRepositoryImpl::new(db.clone()));
    let _webhook_event_repository = Arc::new(WebhookEventRepoImpl::new(db.clone()));
    let storage_repo = if settings.storage.storage_type == "local" {
        let path = settings
            .storage
            .local_path
            .clone()
            .unwrap_or_else(|| "storage".to_string());
        Some(Arc::new(LocalStorage::new(path)))
    } else {
        None
    };
    let reqwest_engine = Arc::new(ReqwestEngine);
    let playwright_engine = Arc::new(PlaywrightEngine);
    let fire_engine_tls = Arc::new(FireEngineTls::new());
    let fire_engine_cdp = Arc::new(FireEngineCdp::new());
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![
        reqwest_engine,
        playwright_engine,
        fire_engine_tls,
        fire_engine_cdp,
    ];
    let router = Arc::new(EngineRouter::new(engines));

    // 初始化智能搜索引擎（使用EngineRouter进行智能路由）
    let mut search_engines: Vec<Arc<dyn SearchEngine>> = Vec::new();

    // 创建Google智能搜索引擎
    search_engines.push(smart_search::create_google_smart_search(router.clone()));

    // 如果有Bing API密钥，创建Bing智能搜索引擎
    if let Some(key) = settings.bing_search.api_key.clone() {
        if !key.is_empty() {
            search_engines.push(smart_search::create_bing_smart_search(router.clone()));
        }
    }

    // 创建百度智能搜索引擎
    search_engines.push(smart_search::create_baidu_smart_search(router.clone()));

    let search_aggregator = Arc::new(SearchAggregator::new(search_engines, 10000));

    // 集成 A/B 测试引擎 (TASK-036)
    // 假设我们将 B 变体设置为 aggregator，A 变体也设置为 aggregator (实际应用中 A/B 应该是不同的实现)
    // 这里为了演示框架集成，我们配置一个 10% 流量到 variant_b 的 A/B 测试
    let search_engine_service: Arc<dyn SearchEngine> = if settings.search.ab_test_enabled {
        info!(
            "Search A/B testing enabled, weight: {}",
            settings.search.variant_b_weight
        );
        Arc::new(
            crawlrs::infrastructure::search::ab_test::SearchABTestEngine::new(
                search_aggregator.clone(),
                search_aggregator, // 实际应用中这里应该是不同的引擎实现
                settings.search.variant_b_weight,
            ),
        )
    } else {
        search_aggregator
    };

    let create_scrape_use_case = Arc::new(
        crawlrs::application::usecases::create_scrape::CreateScrapeUseCase::new(router.clone()),
    );
    let webhook_event_repository = Arc::new(WebhookEventRepoImpl::new(db.clone()));
    let webhook_repository = Arc::new(WebhookRepoImpl::new(db.clone()));
    let credits_repo = Arc::new(CreditsRepositoryImpl::new(db.clone()));
    let _credits_repo_unused = credits_repo.clone();
    let geo_restriction_repo = Arc::new(DatabaseGeoRestrictionRepository::new((*db).clone()));
    let robots_checker = Arc::new(crawlrs::utils::robots::RobotsChecker::new(Some(
        redis_client.clone(),
    )));

    // 初始化限流与并发控制组件
    let tasks_backlog_repo = Arc::new(TasksBacklogRepositoryImpl::new(db.clone()));
    let rate_limiting_config = RateLimitingConfig {
        redis_key_prefix: "crawlrs".to_string(),
        rate_limit: RateLimitConfig {
            strategy: RateLimitStrategy::TokenBucket,
            requests_per_second: settings.rate_limiting.default_rpm / 60,
            requests_per_minute: settings.rate_limiting.default_rpm,
            requests_per_hour: settings.rate_limiting.default_rpm * 60,
            bucket_capacity: Some(settings.rate_limiting.default_rpm),
            enabled: settings.rate_limiting.enabled,
        },
        concurrency: ConcurrencyConfig {
            strategy: ConcurrencyStrategy::DistributedSemaphore,
            max_concurrent_tasks: settings.concurrency.default_team_limit as u32,
            max_concurrent_per_team: settings.concurrency.default_team_limit as u32,
            lock_timeout_seconds: settings.concurrency.task_lock_duration_seconds as u64,
            enabled: true,
        },
        backlog_process_interval_seconds: 30,
        rate_limit_ttl_seconds: 3600,
    };
    let rate_limiting_service = Arc::new(RateLimitingServiceImpl::new(
        redis_client.clone(),
        task_repo.clone(),
        tasks_backlog_repo.clone(),
        credits_repo.clone(),
        rate_limiting_config,
    ));
    info!("Rate limiting service initialized");

    // 8. 根据启动参数选择服务类型
    let args: Vec<String> = std::env::args().collect();
    let service_type = args.get(1).map(String::as_str).unwrap_or("api");

    match service_type {
        "api" => {
            info!("Starting API service...");

            // 启动 Webhook 工作器 (也需要在 API 服务中运行以处理事件)
            let webhook_service =
                Arc::new(WebhookServiceImpl::new(settings.webhook.secret.clone()));
            let webhook_worker = WebhookWorker::new(
                webhook_event_repository.clone(),
                webhook_service,
                RetryPolicy::default(),
            );
            tokio::spawn(async move {
                let _ = webhook_worker.run().await;
            });

            // 启动积压任务处理Worker
            let backlog_worker = BacklogWorker::new(
                tasks_backlog_repo.clone(),
                task_repo.clone(),
                rate_limiting_service.clone(),
                std::time::Duration::from_secs(30), // 每30秒处理一次积压任务
                settings.concurrency.default_team_limit as usize,
            );
            tokio::spawn(async move {
                let _ = backlog_worker.run().await;
            });

            let _auth_state = AuthState {
                db: db.clone(),
                team_id: uuid::Uuid::nil(),
            };

            let public_routes = Router::new()
                .route("/health", get(routes::health_check))
                .route("/v1/version", get(routes::version));

            let protected_routes = Router::new()
                .route("/v1/scrape", post(scrape_handler::create_scrape))
                .route("/v1/scrape/{id}", get(scrape_handler::get_scrape_status))
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

            let v2_routes = task_routes()
                .layer(Extension(task_repo.clone()))
                .layer(Extension(result_repo.clone()))
                //
                .layer(axum::middleware::from_fn_with_state(
                    team_semaphore.clone(),
                    team_semaphore_middleware,
                ))
                .layer(Extension(task_repo.clone()))
                .layer(Extension(result_repo.clone()))
                .layer(Extension(crawl_repo.clone()))
                .layer(Extension(webhook_repository.clone()))
                .layer(Extension(webhook_event_repository.clone()));

            let app = Router::new()
                .merge(public_routes)
                .merge(protected_routes)
                .merge(v2_routes)
                .layer(Extension(team_semaphore.clone()))
                .layer(Extension(queue))
                .layer(Extension(task_repo.clone()))
                .layer(Extension(result_repo.clone()))
                .layer(Extension(crawl_repo.clone()))
                .layer(Extension(webhook_event_repository))
                .layer(Extension(webhook_repository.clone()))
                .layer(Extension(redis_client))
                .layer(Extension(rate_limiter))
                .layer(Extension(crawl_repo.clone()))
                .layer(Extension(credits_repo))
                .layer(Extension(geo_restriction_repo))
                .layer(Extension(settings.clone()))
                .layer(Extension(search_engine_service))
                .layer(Extension(tasks_backlog_repo.clone()))
                .layer(Extension(rate_limiting_service.clone()))
                .layer(Extension(uuid::Uuid::nil())); // TODO: Real team_id from auth

            let addr = format!("{}:{}", settings.server.host, settings.server.port);
            let listener = TcpListener::bind(&addr).await?;
            info!("Server listening on {}", addr);
            axum::serve(listener, app).await?;
        }
        "worker" => {
            info!("Starting Worker service...");
            let mut worker_manager = WorkerManager::new(
                queue.clone(),
                task_repo.clone(),
                result_repo.clone(),
                crawl_repo.clone(),
                storage_repo.clone(),
                webhook_event_repository.clone(),
                credits_repo.clone(),
                router.clone(),
                create_scrape_use_case.clone(),
                (*redis_client).clone(),
                robots_checker.clone(),
                settings.clone(),
                settings.concurrency.default_team_limit as usize,
            );

            // 启动 N 个工作器进程
            worker_manager.start_workers(5).await;

            // 等待关闭信号
            worker_manager.wait_for_shutdown().await;
        }
        _ => {
            error!(
                "Invalid service type: '{}'. Use 'api' or 'worker'.",
                service_type
            );
            std::process::exit(1);
        }
    }

    Ok(())
}
