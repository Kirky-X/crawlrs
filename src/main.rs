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
use crawlrs::engines::fire_engine_cdp::FireEngineCdp;
use crawlrs::engines::fire_engine_tls::FireEngineTls;
use crawlrs::engines::playwright_engine::PlaywrightEngine;
use crawlrs::engines::reqwest_engine::ReqwestEngine;
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
use crawlrs::presentation::middleware::rate_limit_middleware::RateLimiter;
use crawlrs::presentation::middleware::team_semaphore::TeamSemaphore;
use crawlrs::presentation::middleware::team_semaphore_middleware::team_semaphore_middleware;
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
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 初始化日志和遥测系统
    // 设置应用程序的日志记录和分布式追踪
    telemetry::init_telemetry();
    info!("Starting crawlrs...");

    // 初始化 Prometheus 指标收集
    // 用于监控应用程序性能和健康状况
    crawlrs::infrastructure::metrics::init_metrics();

    // 2. 加载应用程序配置
    // 从环境变量和配置文件加载所有设置
    let settings = Arc::new(Settings::new()?);
    info!("Configuration loaded");

    // 3. 建立数据库连接
    // 创建数据库连接池，用于高效的数据库操作
    let db = connection::create_pool(&settings.database).await?;
    let db = Arc::new(db);
    info!("Database connection established");

    // 运行数据库迁移
    // 确保数据库模式是最新的
    info!("Running database migrations...");
    Migrator::up(db.as_ref(), None).await?;
    info!("Database migrations applied");

    // 4. 初始化 Redis 客户端
    // 用于缓存、速率限制和分布式锁
    let redis_client = RedisClient::new(&settings.redis.url).await?;
    info!("Redis client initialized");

    // 5. 初始化速率限制器
    // 防止 API 被滥用，控制请求频率
    let rate_limiter = Arc::new(RateLimiter::new(
        redis_client.clone(),
        settings.rate_limiting.default_rpm,
    ));
    info!("Rate limiter initialized");

    // 6. 初始化团队信号量
    // 控制每个团队的最大并发任务数
    let team_semaphore = Arc::new(TeamSemaphore::new(
        settings.concurrency.default_team_limit as usize,
    ));
    info!("Team semaphore initialized");

    // 6. 初始化核心组件
    // 创建任务仓库，负责任务的持久化和管理
    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db.clone(),
        chrono::Duration::seconds(settings.concurrency.task_lock_duration_seconds),
    ));

    // 创建基于 PostgreSQL 的任务队列
    let queue = Arc::new(PostgresTaskQueue::new(task_repo.clone()));

    // 创建抓取结果仓库，用于存储爬取结果
    let result_repo = Arc::new(ScrapeResultRepositoryImpl::new(db.clone()));

    // 创建爬取任务仓库，管理爬取任务的生命周期
    let crawl_repo = Arc::new(CrawlRepositoryImpl::new(db.clone()));

    // 初始化存储
    // 根据配置选择本地存储或其他存储类型（如 S3）
    let storage_repo = if settings.storage.storage_type == "local" {
        let path = settings
            .storage
            .local_path
            .clone()
            .unwrap_or_else(|| "storage".to_string());
        Some(Arc::new(LocalStorage::new(path)))
    } else {
        // 预留 S3 或其他存储类型的占位符
        None
    };

    // 初始化爬取引擎
    // 创建不同类型的网页爬取引擎，按优先级排序
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

    // 创建引擎路由器，根据 URL 选择合适的引擎
    let router = Arc::new(EngineRouter::new(engines));

    // 初始化用例
    // 创建抓取用例，协调抓取任务的执行
    let create_scrape_use_case = Arc::new(
        crawlrs::application::usecases::create_scrape::CreateScrapeUseCase::new(router.clone()),
    );

    // 创建 Webhook 相关仓库
    let webhook_event_repository = Arc::new(WebhookEventRepoImpl::new(db.clone()));
    let webhook_repository = Arc::new(WebhookRepoImpl::new(db.clone()));

    // 初始化 Robots.txt 检查器
    // 用于检查网站是否允许爬取
    let robots_checker = Arc::new(crawlrs::utils::robots::RobotsChecker::new());

    // 7. 启动工作器
    // 创建工作管理器，负责协调所有后台任务
    let mut worker_manager = WorkerManager::new(
        queue.clone(),
        task_repo.clone(),
        result_repo.clone(),
        crawl_repo.clone(),
        storage_repo.clone(),
        webhook_event_repository.clone(),
        router.clone(),
        create_scrape_use_case.clone(),
        redis_client.clone(),
        robots_checker.clone(),
        settings.concurrency.default_team_limit as usize,
    );

    // 启动 5 个工作器进程
    worker_manager.start_workers(5).await;

    // 创建 Webhook 工作器
    // 负责处理 Webhook 事件的发送
    let webhook_worker = WebhookWorker::new(
        webhook_event_repository.clone(),
        settings.webhook.secret.clone(),
    );

    // 在后台异步运行 Webhook 工作器
    tokio::spawn(async move {
        webhook_worker.run().await;
    });

    // 8. 设置认证状态
    // 初始的 AuthState 还没有有效的 team_id，但会在中间件中填充
    // 我们使用 Uuid::nil() 作为占位符
    let auth_state = AuthState {
        db: db.clone(),
        team_id: uuid::Uuid::nil(),
    };

    // 9. 配置 HTTP 服务器
    // 创建公共路由（无需认证）
    let public_routes = Router::new()
        .route("/health", get(routes::health_check)) // 健康检查端点
        .route("/v1/version", get(routes::version)); // 版本信息端点

    // 创建受保护的路由（需要认证）
    let protected_routes = Router::new()
        // 抓取任务相关端点
        .route("/v1/scrape", post(scrape_handler::create_scrape)) // 创建抓取任务
        .route("/v1/scrape/:id", get(scrape_handler::get_scrape_status)) // 获取抓取状态
        // Webhook 相关端点
        .route(
            "/v1/webhooks",
            post(webhook_handler::create_webhook::<WebhookRepoImpl>), // 创建 Webhook
        )
        // 爬取任务相关端点
        .route(
            "/v1/crawl",
            post(
                crawl_handler::create_crawl::<
                    CrawlRepositoryImpl,
                    TaskRepositoryImpl,
                    WebhookRepoImpl,
                    ScrapeResultRepositoryImpl,
                >, // 创建爬取任务
            ),
        )
        .route(
            "/v1/crawl/:id",
            get(crawl_handler::get_crawl::<
                CrawlRepositoryImpl,
                TaskRepositoryImpl,
                WebhookRepoImpl,
                ScrapeResultRepositoryImpl,
            >), // 获取爬取任务详情
        )
        .route(
            "/v1/crawl/:id/results",
            get(crawl_handler::get_crawl_results::<
                CrawlRepositoryImpl,
                TaskRepositoryImpl,
                WebhookRepoImpl,
                ScrapeResultRepositoryImpl,
            >), // 获取爬取结果
        )
        .route(
            "/v1/crawl/:id",
            delete(
                crawl_handler::cancel_crawl::<
                    CrawlRepositoryImpl,
                    TaskRepositoryImpl,
                    WebhookRepoImpl,
                    ScrapeResultRepositoryImpl,
                >, // 取消爬取任务
            ),
        )
        // 搜索端点
        .route(
            "/v1/search",
            post(search_handler::search::<CrawlRepositoryImpl, TaskRepositoryImpl>), // 搜索功能
        )
        // 添加认证中间件
        .layer(axum::middleware::from_fn_with_state(
            auth_state.clone(),
            auth_middleware,
        ))
        // 添加团队信号量中间件
        .layer(axum::middleware::from_fn_with_state(
            team_semaphore.clone(),
            team_semaphore_middleware,
        ))
        // 添加依赖注入扩展
        .layer(Extension(task_repo.clone()))
        .layer(Extension(result_repo.clone()))
        .layer(Extension(crawl_repo.clone()))
        .layer(Extension(webhook_repository.clone()))
        .layer(Extension(webhook_event_repository.clone()));

    // 创建最终的 Axum 应用
    // 合并公共路由和受保护路由，并添加全局中间件和扩展
    let app = Router::new()
        .merge(public_routes) // 合并公共路由
        .merge(protected_routes) // 合并受保护路由
        // 添加全局扩展，供所有路由使用
        .layer(Extension(team_semaphore.clone())) // 团队信号量
        .layer(Extension(queue)) // 任务队列
        .layer(Extension(task_repo.clone())) // 任务仓库
        .layer(Extension(webhook_event_repository)) // Webhook 事件仓库
        .layer(Extension(webhook_repository.clone())) // Webhook 仓库
        .layer(Extension(redis_client)) // Redis 客户端
        .layer(Extension(rate_limiter)) // 速率限制器
        .layer(Extension(crawl_repo.clone())) // 爬取仓库
        .layer(Extension(settings.clone())); // 应用配置

    // 构建服务器地址
    let addr = format!("{}:{}", settings.server.host, settings.server.port);

    // 绑定 TCP 监听器
    let listener = TcpListener::bind(&addr).await?;
    info!("Server listening on {}", addr);

    // 启动 HTTP 服务器
    // 这将阻塞当前任务，直到服务器停止
    axum::serve(listener, app).await?;

    Ok(())
}
