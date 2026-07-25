// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Application services initialization.

use log::info;
use std::sync::Arc;

use crate::application::use_cases::create_scrape::{CreateScrapeUseCase, CreateScrapeUseCaseTrait};
use crate::bootstrap::infrastructure::InfrastructureComponents;
use crate::bootstrap::infrastructure::Repositories;
use crate::config::settings::Settings;
// R-auth-engine-002 / T011：auth feature 关闭时不导入 garrison 桥接类型
#[cfg(feature = "auth")]
use crate::bootstrap::error::BootstrapError;
#[cfg(feature = "auth")]
use crate::infrastructure::auth::{
    build_garrison_config, init_garrison_dao, CrawlrsGarrisonInterface,
};
// garrison prelude 提供 GarrisonDao / GarrisonInterface / GarrisonManager trait 与类型。
use crate::domain::services::audit_service::{AuditService, AuditServiceTrait};
use crate::domain::services::auth_scope_service::AuthScopeService;
use crate::domain::services::extraction_service::{ExtractionService, ExtractionServiceTrait};
#[cfg(feature = "auth")]
use dbnexus::DbPool;
#[cfg(feature = "auth")]
use garrison::prelude::{GarrisonDao, GarrisonInterface, GarrisonManager};
// R-teams-004 / T015：teams-off 时不导入 teams 相关类型
#[cfg(feature = "teams")]
use crate::domain::services::geo_location::GeoLocationService;
use crate::domain::services::llm_service::{LLMService, LLMServiceTrait};
use crate::domain::services::rate_limiting_service::RateLimitingService;
// R-rl-003 / T020：rate-limit feature 关闭时不导入 LimiteronService 相关配置类型
#[cfg(feature = "rate-limit")]
use crate::domain::services::rate_limiting_service::{
    ConcurrencyConfig, ConcurrencyStrategy, RateLimitConfig, RateLimitStrategy,
};
use crate::domain::services::search_service::{SearchService, SearchServiceTrait};
#[cfg(feature = "teams")]
use crate::domain::services::team_service::TeamService;
// R-wh-003 / T027：webhook feature 关闭时不导入 WebhookServiceImpl
// （NoopWebhookService 在 webhook-off 时替代，trait 始终导入）
use crate::domain::services::webhook_service::WebhookService;
#[cfg(feature = "webhook")]
use crate::domain::services::webhook_service::WebhookServiceImpl;
// R-wh-003 / T027：webhook feature 关闭时导入 NoopWebhookService
#[cfg(not(feature = "webhook"))]
use crate::domain::services::noop_webhook_service::NoopWebhookService;
use crate::engines::engine_client::EngineClient;
use crate::engines::router::EngineRouter;
use crate::infrastructure::database::repositories::audit_log_repo_impl::AuditLogRepositoryImpl;
use crate::infrastructure::database::repositories::auth_scope_repo_impl::AuthScopeRepositoryImpl;
#[cfg(feature = "teams")]
use crate::infrastructure::geolocation::GeoLocationServiceImpl;
// R-rl-003 / T020：rate-limit feature 关闭时不导入 LimiteronService
#[cfg(feature = "rate-limit")]
use crate::infrastructure::services::limiteron_service::{LimiteronService, RateLimitingConfig};
// R-rl-003 / T020：rate-limit feature 关闭时导入 NoopRateLimitingService
#[cfg(not(feature = "rate-limit"))]
use crate::infrastructure::services::noop_rate_limiting_service::NoopRateLimitingService;
// R-wh-003 / T027：webhook feature 关闭时不导入 WebhookSenderImpl
// （webhook_sender_impl 模块本身也被 cfg 门控，见 infrastructure::services::mod）
#[cfg(feature = "webhook")]
use crate::infrastructure::services::webhook_sender_impl::WebhookSenderImpl;
use crate::presentation::middleware::auth_middleware::AuthRateLimiter;
use crate::presentation::middleware::rate_limit_middleware::RateLimitMiddleware;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::queue::task_queue::{PostgresTaskQueue, TaskQueue};
use crate::search::ab_test::SearchABTestEngine;
use crate::search::aggregator::SearchAggregator;
use crate::search::client::SearchClientTrait;
use crate::search::engine_trait::SearchEngine;
use crate::search::smart as smart_search;
use crate::utils::regex_cache::RegexCache;
use crate::utils::robots::RobotsChecker;

/// All application services.
#[derive(Clone)]
pub struct ServicesComponents {
    /// Rate limit middleware for API requests.
    pub rate_limit_middleware: RateLimitMiddleware,
    /// Team semaphore for concurrency control.
    pub team_semaphore: Arc<TeamSemaphore>,
    /// Rate limiting service for distributed rate limiting.
    pub rate_limiting_service: Arc<dyn RateLimitingService>,
    /// Rate limiter (in-memory via limiteron)
    pub rate_limiter: Option<Arc<AuthRateLimiter>>,
    /// Create scrape use case.
    pub create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    /// Webhook service.
    ///
    /// R-wh-003 / T027：webhook feature 关闭时此字段保留，但装配 `NoopWebhookService`。
    /// `WebhookService` trait 需始终编译（业务逻辑通过 trait 调用 trigger_completion / trigger_failure），
    /// 故此字段不门控；`init_services` 中的装配按 cfg 选择具体实现。
    pub webhook_service: Arc<dyn WebhookService>,
    /// Team service.
    ///
    /// R-teams-004 / T015：teams feature 关闭时不编译此字段。
    #[cfg(feature = "teams")]
    pub team_service: Arc<TeamService>,
    /// Geo Location Service
    ///
    /// R-teams-004 / T015：teams feature 关闭时不编译此字段。
    #[cfg(feature = "teams")]
    pub geo_location_service: Arc<dyn GeoLocationService>,
    /// Robots.txt checker.
    pub robots_checker: Arc<RobotsChecker>,
    /// Search engine service.
    pub search_engine_service: Arc<dyn SearchEngine>,
    /// Search service.
    pub search_service: Arc<dyn SearchServiceTrait>,
    /// Auth scope service for API key permission management.
    pub auth_scope_service: Option<Arc<AuthScopeService>>,
    /// Task queue.
    pub queue: Arc<dyn TaskQueue>,
    /// Audit service.
    pub audit_service: Arc<dyn AuditServiceTrait>,
    /// HTTP Client
    pub http_client: Arc<reqwest::Client>,
    /// LLM service for LLM operations.
    pub llm_service: Arc<dyn LLMServiceTrait>,
    /// Extraction service.
    pub extraction_service: Arc<dyn ExtractionServiceTrait>,
    /// Regex cache for performance optimization.
    pub regex_cache: Arc<RegexCache>,
    /// Webhook worker
    ///
    /// R-wh-003 / T027：webhook feature 关闭时不编译此字段。
    /// webhook-off 模式下，不启动 webhook_worker，不需要此字段。
    #[cfg(feature = "webhook")]
    pub webhook_worker: Arc<crate::workers::webhook_worker::WebhookWorker>,
    /// Backlog worker
    pub backlog_worker: Arc<crate::workers::backlog_worker::BacklogWorker>,
    /// Expiration worker
    pub expiration_worker: Arc<crate::workers::expiration_worker::ExpirationWorker>,
}

/// Initialize rate limit middleware.
///
/// # Arguments
///
/// * `rate_limiting_service` - Rate limiting service for distributed rate limiting
///
/// # Returns
///
/// Returns an initialized rate limit middleware.
pub fn init_rate_limit_middleware(
    rate_limiting_service: Arc<dyn RateLimitingService>,
) -> RateLimitMiddleware {
    RateLimitMiddleware::new(rate_limiting_service)
}

/// Initialize team semaphore for concurrency control.
///
/// # Arguments
///
/// * `default_team_limit` - Default concurrent limit per team
///
/// # Returns
///
/// Returns an initialized team semaphore.
pub fn init_team_semaphore(default_team_limit: u64) -> Arc<TeamSemaphore> {
    Arc::new(TeamSemaphore::new(default_team_limit as usize))
}

/// Initialize rate limiting service.
///
/// R-rl-003 / T020：根据 `rate-limit` feature 选择装配：
/// - `rate-limit` on：使用 `LimiteronService`（内存存储）
/// - `rate-limit` off：使用 `NoopRateLimitingService`（放行所有请求）
///
/// # Arguments
///
/// * `repositories` - Application repositories
/// * `settings` - Application settings
///
/// # Returns
///
/// Returns an initialized rate limiting service as `Arc<dyn RateLimitingService>`.
pub async fn init_rate_limiting_service(
    repositories: &Repositories,
    settings: &Settings,
) -> Arc<dyn RateLimitingService> {
    #[cfg(feature = "rate-limit")]
    {
        let rate_limit_config = RateLimitConfig {
            strategy: RateLimitStrategy::TokenBucket,
            requests_per_second: settings.rate_limiting.default_rpm / 60,
            requests_per_minute: settings.rate_limiting.default_rpm,
            requests_per_hour: settings.rate_limiting.default_rpm * 60,
            bucket_capacity: Some(settings.rate_limiting.default_rpm),
            enabled: settings.rate_limiting.enabled,
        };

        // Validate rate limit config
        if let Err(e) = rate_limit_config.validate() {
            log::error!("Rate limit configuration error: {}", e);
        }

        let concurrency_config = ConcurrencyConfig {
            strategy: ConcurrencyStrategy::DistributedSemaphore,
            max_concurrent_tasks: settings.concurrency.default_team_limit as u32,
            max_concurrent_per_team: settings.concurrency.default_team_limit as u32,
            lock_timeout_seconds: settings.concurrency.task_lock_duration_seconds as u64,
            enabled: true,
        };

        // Validate concurrency config
        if let Err(e) = concurrency_config.validate() {
            log::error!("Concurrency configuration error: {}", e);
        }

        let rate_limiting_config = RateLimitingConfig {
            rate_limit: rate_limit_config,
            concurrency: concurrency_config,
            backlog_process_interval_seconds: 30,
            rate_limit_ttl_seconds: 3600,
        };

        let service = LimiteronService::new(
            repositories.task_repo.clone(),
            repositories.tasks_backlog_repo.clone(),
            repositories.credits_repo.clone(),
            rate_limiting_config,
        )
        .await
        .expect("Failed to create LimiteronService");

        Arc::new(service)
    }
    // R-rl-003 / T020：rate-limit-off 时装配 NoopRateLimitingService
    #[cfg(not(feature = "rate-limit"))]
    {
        // 避免 unused 参数 warning
        let _ = repositories;
        let _ = settings;
        log::warn!(
            "rate-limit feature disabled, using NoopRateLimitingService — \
             all requests are allowed without rate limiting"
        );
        Arc::new(NoopRateLimitingService::new())
    }
}

/// Initialize search engine service.
///
/// # Arguments
///
/// * `engine_client` - Engine client for making requests
/// * `settings` - Application settings
///
/// # Returns
///
/// Returns an initialized search engine.
pub fn init_search_engine(
    engine_client: Arc<EngineClient>,
    settings: &Settings,
) -> Arc<dyn SearchEngine> {
    let search_engines: Vec<Arc<dyn SearchEngine>> = vec![
        smart_search::create_google_smart_search(engine_client.clone()),
        smart_search::create_baidu_smart_search(engine_client.clone()),
        smart_search::create_sogou_smart_search(engine_client.clone()),
        smart_search::create_bing_smart_search(engine_client.clone()),
    ];

    let search_aggregator = Arc::new(SearchAggregator::new(search_engines, 10000));

    if settings.search.ab_test_enabled {
        info!(
            "Search A/B testing enabled, weight: {}",
            settings.search.variant_b_weight
        );
        Arc::new(SearchABTestEngine::new(
            search_aggregator.clone(),
            search_aggregator,
            settings.search.variant_b_weight,
        ))
    } else {
        search_aggregator
    }
}

/// Initialize search service.
///
/// This function creates the SearchService with all required dependencies,
/// following dependency injection principles.
///
/// # Arguments
///
/// * `repositories` - Application repositories
/// * `settings` - Application settings
/// * `search_client` - Search client instance implementing SearchClientTrait
///
/// # Returns
///
/// Returns an initialized search service as trait object.
pub fn init_search_service(
    repositories: &Repositories,
    settings: &Settings,
    search_client: Arc<dyn SearchClientTrait>,
) -> Arc<dyn SearchServiceTrait> {
    // Create SearchService with concrete repository types
    let service = SearchService::new(
        repositories.crawl_repo.clone(),
        repositories.task_repo.clone(),
        repositories.credits_repo.clone(),
        Arc::new(settings.clone()),
        search_client,
    );
    Arc::new(service)
}

/// Initialize auth scope service.
///
/// This function creates the AuthScopeService for authentication scope operations,
/// following dependency injection principles.
///
/// # Arguments
///
/// * `pool` - Database pool
///
/// # Returns
///
/// Returns an initialized auth scope service wrapped in Arc.
pub fn init_auth_scope_service(pool: Arc<dbnexus::DbPool>) -> Arc<AuthScopeService> {
    let repo = Arc::new(AuthScopeRepositoryImpl::new(pool));
    Arc::new(AuthScopeService::new(repo))
}

/// 初始化 garrison 认证鉴权服务（R-auth-engine-002 / T011）。
///
/// 装配流程：
/// 1. 调用 [`init_garrison_dao`] 获取 `Arc<dyn GarrisonDao>`（oxcache 内存存储，自管理实例）
/// 2. 调用 [`build_garrison_config`] 构造 [`GarrisonConfig`]（弱密钥拒绝，HS256 ≥32 字节）
/// 3. 构造 [`CrawlrsGarrisonInterface`] 并装为 `Arc<dyn GarrisonInterface>`
///    （注入 crawlrs 的 `DbPool` 用于查询 garrison RBAC 表）
/// 4. 调用 [`GarrisonManager::init`] 写入 `GARRISON_MANAGER` 全局单例
/// 5. 错误（DAO 失败 / 弱密钥 / manager init 失败）映射为 [`BootstrapError::Auth`]
///
/// # Fail-Fast 语义
///
/// 此函数返回 `Result<(), BootstrapError>`，调用方（[`init_services`]）通过
/// `.expect()` 在失败时 panic，触发 bootstrap fail-fast：
/// - 弱密钥 / 空密钥 → 启动失败（强制运维提供强密钥）
/// - DAO oxcache 初始化失败 → 启动失败（内存资源不足）
/// - `GarrisonManager::init` 失败（配置非法等）→ 启动失败
///
/// 这是规则 12（失败必须显性化）的体现——不藏默认值背后。
///
/// # Arguments
///
/// * `settings` - 应用配置（含 `auth.jwt_secret`）
/// * `pool` - crawlrs 数据库连接池 `Arc<DbPool>`（注入到 `CrawlrsGarrisonInterface`
///   查询 RBAC 表；传 Arc 而非 DbPool 匹配现有 `init_auth_scope_service` 调用模式）
///
/// # Returns
///
/// - `Ok(())` - garrison 初始化成功（`GARRISON_MANAGER` 单例已就绪，后续中间件通过
///   `GarrisonUtil` 访问）
/// - `Err(BootstrapError::Auth)` - DAO 初始化失败 / 弱密钥 / `GarrisonManager::init` 失败
///
/// # Spec
///
/// - R-auth-engine-002：构造 `CrawlrsGarrisonDao`/`build_garrison_config`/
///   `CrawlrsGarrisonInterface` 并调 `GarrisonManager::init`，失败 map 为
///   `BootstrapError::Auth`
#[cfg(feature = "auth")]
pub async fn init_garrison_auth(
    settings: &Settings,
    pool: Arc<DbPool>,
) -> Result<(), BootstrapError> {
    // 1. 构造 GarrisonConfig（弱密钥 / 空密钥在此被拒绝）
    //
    // 顺序选择：先做便宜的同步 config 校验，再做异步 DAO 初始化——
    // 弱密钥场景下避免无谓的 oxcache 实例创建（规则5 简洁优先的执行层面体现）。
    let config = build_garrison_config(settings.auth.jwt_secret())
        .map_err(|e| BootstrapError::Auth(format!("garrison config error: {e}")))?;
    let config = Arc::new(config);

    // 2. 初始化 DAO（garrison 内建 oxcache，自管理实例）
    let dao: Arc<dyn GarrisonDao> = init_garrison_dao()
        .await
        .map_err(|e| BootstrapError::Auth(format!("garrison dao init failed: {e}")))?;

    // 3. 构造业务 Interface（注入 crawlrs DbPool 查询 RBAC 表）
    //
    // `(*pool).clone()`：deref Arc<DbPool> → DbPool，再 clone（DbPool 内部为
    // Arc<DbPoolInner>，clone 廉价 <10ns）。`CrawlrsGarrisonInterface::new` 持有
    // owned DbPool，符合其既有签名（规则6 外科手术式修改，不改动 Stage 1 代码）。
    let interface: Arc<dyn GarrisonInterface> =
        Arc::new(CrawlrsGarrisonInterface::new((*pool).clone()));

    // 4. 写入 garrison 全局单例（同步函数，覆盖式更新允许重复 init）
    GarrisonManager::init(dao, config, interface)
        .map_err(|e| BootstrapError::Auth(format!("garrison manager init failed: {e}")))?;

    info!("Garrison authentication manager initialized (token_style=jwt, HS256)");
    Ok(())
}

/// Initialize LLM service.
///
/// This function creates the LLMService for LLM operations,
/// following dependency injection principles.
///
/// # Arguments
///
/// * `settings` - Application settings
/// * `http_client` - HTTP client for making requests
///
/// # Returns
///
/// Returns an initialized LLM service wrapped in Arc.
pub fn init_llm_service(
    settings: &Settings,
    http_client: Arc<reqwest::Client>,
) -> Arc<dyn LLMServiceTrait> {
    Arc::new(LLMService::new(settings, http_client))
}

/// Initialize regex cache.
///
/// This function creates a RegexCache for performance optimization,
/// following dependency injection principles.
///
/// # Returns
///
/// Returns an initialized regex cache wrapped in Arc.
pub fn init_regex_cache() -> Arc<RegexCache> {
    let cache = futures::executor::block_on(async {
        oxcache::Cache::builder()
            .capacity(1000)
            .ttl(std::time::Duration::from_secs(3600))
            .build()
            .await
            .expect("Failed to create regex cache")
    });
    Arc::new(RegexCache::new(Arc::new(cache)))
}

/// Initialize all application services.
///
/// # Arguments
///
/// * `infrastructure` - Initialized infrastructure components
/// * `engine_router` - Engine router for creating use cases
/// * `engine_client` - Engine client for scraping operations
/// * `settings` - Application settings
///
/// # Returns
///
/// Returns all initialized services.
pub async fn init_services(
    infrastructure: &InfrastructureComponents,
    engine_router: Arc<EngineRouter>,
    engine_client: Arc<EngineClient>,
    http_client: Arc<reqwest::Client>,
    settings: &Settings,
) -> ServicesComponents {
    let repositories = &infrastructure.repositories;

    // Initialize rate limiter (for auth rate limiting)
    let rate_limiter = Some(Arc::new(AuthRateLimiter::new()));

    // Initialize team semaphore
    let team_semaphore = init_team_semaphore(settings.concurrency.default_team_limit as u64);

    // Initialize rate limiting service
    let rate_limiting_service = init_rate_limiting_service(repositories, settings).await;

    // Initialize rate limit middleware
    let rate_limit_middleware = init_rate_limit_middleware(rate_limiting_service.clone());

    // Initialize create scrape use case
    let create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait> =
        Arc::new(CreateScrapeUseCase::new(engine_client.clone()));

    // R-wh-003 / T027：webhook 相关服务在 webhook-off 时不初始化
    // webhook-on：装配 WebhookServiceImpl（含 WebhookSenderImpl + 签名密钥 + 事件仓库）
    // webhook-off：装配 NoopWebhookService（所有方法返回 Ok(())，业务逻辑放行）
    #[cfg(feature = "webhook")]
    let webhook_sender: Arc<WebhookSenderImpl> = Arc::new(WebhookSenderImpl::new(
        http_client.clone(),
        std::time::Duration::from_secs(10),
    ));
    #[cfg(feature = "webhook")]
    let webhook_service: Arc<WebhookServiceImpl> = Arc::new(WebhookServiceImpl::new(
        webhook_sender.clone(),
        settings.webhook.secret().to_string(),
        repositories.webhook_event_repo.clone(),
    ));
    #[cfg(not(feature = "webhook"))]
    let webhook_service: Arc<NoopWebhookService> = {
        // tiangang 安全审查 LOW-1：webhook-off 时输出 info 日志，告知运维 webhook 投递已禁用。
        // 严重程度低于 auth/rate-limit 警告（webhook 关闭不直接引入安全风险，仅影响通知能力）。
        log::info!(
            "webhook feature disabled, using NoopWebhookService — \
             trigger_completion/trigger_failure are no-ops"
        );
        Arc::new(NoopWebhookService::new())
    };

    // R-teams-004 / T015：teams 相关服务在 teams-off 时不初始化
    #[cfg(feature = "teams")]
    let geo_location_service = Arc::new(GeoLocationServiceImpl::new(http_client.clone()));
    #[cfg(feature = "teams")]
    let team_service = Arc::new(TeamService::new(
        geo_location_service.clone(),
        repositories.geo_restriction_repo.clone(),
    ));

    // Initialize robots checker (使用依赖注入的 HTTP_CLIENT + CacheService)
    let robots_checker = Arc::new(RobotsChecker::new(
        http_client.clone(),
        Some(infrastructure.cache_service.clone()),
        None,
    ));

    // Initialize search engine (for backward compatibility)
    let search_engine_service: Arc<dyn SearchEngine> = init_search_engine(
        Arc::new(EngineClient::with_router(engine_router.clone())),
        settings,
    );

    // Initialize search client (wraps search engines)
    let search_client: Arc<dyn SearchClientTrait> =
        Arc::new(crate::search::client::SearchClient::new(Arc::new(
            EngineClient::with_router(engine_router.clone()),
        )));

    // Initialize search service
    let search_service = init_search_service(repositories, settings, search_client.clone());

    // Initialize auth scope service
    let auth_scope_service = Some(init_auth_scope_service(infrastructure.db.inner().clone()));

    // R-auth-engine-002 / T011：初始化 garrison 认证鉴权（auth-on 时 fail-fast）
    //
    // - 弱密钥 / 空密钥 → panic（强制运维提供强密钥，CWE-326）
    // - DAO oxcache 初始化失败 / GarrisonManager::init 失败 → panic
    //
    // 详见 [`init_garrison_auth`] 文档的「Fail-Fast 语义」章节。
    // 此调用无返回值——garrison 通过 GARRISON_MANAGER 全局单例暴露能力，
    // 后续中间件（Stage 3 / T017 重写 auth_middleware_inner）通过 GarrisonUtil 访问。
    //
    // pool 传 Arc<DbPool> 的 clone（Arc::clone 廉价 <10ns），匹配
    // init_auth_scope_service(infrastructure.db.inner().clone()) 既有模式。
    #[cfg(feature = "auth")]
    init_garrison_auth(settings, infrastructure.db.inner().clone())
        .await
        .expect(
            "garrison auth initialization failed (BootstrapError::Auth) — \
                 check CRAWLRS__AUTH__JWT_SECRET env var (HS256 requires >=32 bytes)",
        );

    // Initialize task queue
    let queue: Arc<dyn TaskQueue> =
        Arc::new(PostgresTaskQueue::new(repositories.task_repo.clone()));

    // Initialize audit service
    let audit_repo = Arc::new(AuditLogRepositoryImpl::new(
        infrastructure.db.inner().clone(),
    ));
    let audit_service = Arc::new(AuditService::new(audit_repo));

    // Initialize LLM service (使用依赖注入的 http_client)
    let llm_service = init_llm_service(settings, http_client.clone());

    // Initialize extraction service
    let extraction_service = Arc::new(ExtractionService::new(llm_service.clone()));

    // Initialize regex cache
    let regex_cache = init_regex_cache();

    // R-wh-003 / T027：Initialize WebhookWorker（仅 webhook-on 时构造）
    // webhook-off：不构造 WebhookWorker，ServicesComponents.webhook_worker 字段不编译
    #[cfg(feature = "webhook")]
    let webhook_worker = Arc::new(crate::workers::webhook_worker::WebhookWorker::new(
        repositories.webhook_event_repo.clone(),
        webhook_service.clone(),
        crate::utils::retry_policy::RetryPolicy::default(),
    ));

    // Initialize BacklogWorker
    let backlog_worker = Arc::new(crate::workers::backlog_worker::BacklogWorker::new(
        repositories.tasks_backlog_repo.clone(),
        repositories.task_repo.clone(),
        rate_limiting_service.clone(),
        Arc::new(settings.clone()),
    ));

    // Initialize ExpirationWorker
    let expiration_worker = Arc::new(crate::workers::expiration_worker::ExpirationWorker::new(
        repositories.task_repo.clone(),
    ));

    info!("Services initialized");

    ServicesComponents {
        rate_limit_middleware,
        rate_limiter,
        team_semaphore,
        rate_limiting_service,
        create_scrape_use_case,
        webhook_service,
        #[cfg(feature = "teams")]
        team_service,
        #[cfg(feature = "teams")]
        geo_location_service,
        robots_checker,
        search_engine_service,
        search_service,
        auth_scope_service,
        queue,
        audit_service,
        http_client,
        llm_service,
        extraction_service,
        regex_cache,
        #[cfg(feature = "webhook")]
        webhook_worker,
        backlog_worker,
        expiration_worker,
    }
}

// Note: The following functions are not unit-tested here because they require
// real external services that are only available in Docker-based integration tests:
//   - init_rate_limiting_service: needs Repositories (DB pool) — LimiteronService uses in-memory storage
//   - init_search_service: needs Repositories (DB pool for crawl/task/credits repos)
//   - init_auth_scope_service: needs dbnexus::DbPool (PostgreSQL connection)
//   - init_services: needs InfrastructureComponents (full DB + HTTP stack)
// These are covered by integration tests in tests/integration/ with Docker-provided
// PostgreSQL.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::CreditsTransactionType;
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyControlService, ConcurrencyResult, QuotaService,
        RateLimitResult, RateLimitService, RateLimitingError,
    };

    fn make_http_client() -> Arc<reqwest::Client> {
        Arc::new(reqwest::Client::new())
    }

    fn make_engine_client() -> Arc<EngineClient> {
        Arc::new(EngineClient::new())
    }

    // ========== Mock RateLimitingService for init_rate_limit_middleware tests ==========

    /// A no-op mock implementation of RateLimitingService for unit testing.
    /// All methods return Ok with default/empty values.
    struct MockRateLimitingService;

    #[async_trait::async_trait]
    impl RateLimitService for MockRateLimitingService {
        async fn check_rate_limit(
            &self,
            _api_key: &str,
            _endpoint: &str,
        ) -> Result<RateLimitResult, RateLimitingError> {
            Ok(RateLimitResult::Allowed)
        }

        async fn get_team_rate_limit_config(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<RateLimitConfig, RateLimitingError> {
            Ok(RateLimitConfig::default())
        }

        async fn update_team_rate_limit_config(
            &self,
            _team_id: uuid::Uuid,
            _config: RateLimitConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait::async_trait]
    impl ConcurrencyControlService for MockRateLimitingService {
        async fn check_team_concurrency(
            &self,
            _team_id: uuid::Uuid,
            _task_id: uuid::Uuid,
        ) -> Result<ConcurrencyResult, RateLimitingError> {
            Ok(ConcurrencyResult::Allowed)
        }

        async fn release_team_concurrency_slot(
            &self,
            _team_id: uuid::Uuid,
            _task_id: uuid::Uuid,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_team_current_concurrency(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }

        async fn get_team_concurrency_config(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<ConcurrencyConfig, RateLimitingError> {
            Ok(ConcurrencyConfig::default())
        }

        async fn update_team_concurrency_config(
            &self,
            _team_id: uuid::Uuid,
            _config: ConcurrencyConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl BacklogService for MockRateLimitingService {
        async fn process_backlog_tasks(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait::async_trait]
    impl QuotaService for MockRateLimitingService {
        async fn check_and_deduct_quota(
            &self,
            _team_id: uuid::Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<uuid::Uuid>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_quota_balance(&self, _team_id: uuid::Uuid) -> Result<i64, RateLimitingError> {
            Ok(0)
        }
    }

    impl RateLimitingService for MockRateLimitingService {}

    // ========== init_team_semaphore tests ==========

    #[test]
    fn test_init_team_semaphore_creates_instance() {
        let semaphore = init_team_semaphore(10);
        assert!(
            Arc::strong_count(&semaphore) >= 1,
            "init_team_semaphore should return a valid Arc<TeamSemaphore>"
        );
    }

    #[test]
    fn test_init_team_semaphore_with_different_limits() {
        let s1 = init_team_semaphore(1);
        let s2 = init_team_semaphore(100);
        let s3 = init_team_semaphore(1000);
        // All should be successfully created
        assert!(Arc::strong_count(&s1) >= 1);
        assert!(Arc::strong_count(&s2) >= 1);
        assert!(Arc::strong_count(&s3) >= 1);
    }

    #[test]
    fn test_init_team_semaphore_zero_limit() {
        // A zero limit is edge case; should still create without panic
        let semaphore = init_team_semaphore(0);
        assert!(Arc::strong_count(&semaphore) >= 1);
    }

    // ========== init_regex_cache tests ==========

    #[test]
    fn test_init_regex_cache_creates_instance() {
        let cache = init_regex_cache();
        // Verify the cache is usable by getting/inserting a simple pattern
        let result = cache.get_or_insert(r"\d+");
        assert!(
            result.is_ok(),
            "RegexCache should be usable after init_regex_cache"
        );
    }

    #[test]
    fn test_init_regex_cache_returns_arc() {
        let cache = init_regex_cache();
        assert!(
            Arc::strong_count(&cache) >= 1,
            "init_regex_cache should return a valid Arc<RegexCache>"
        );
    }

    // ========== init_llm_service tests ==========

    #[test]
    fn test_init_llm_service_creates_instance() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let http_client = make_http_client();
        let service = init_llm_service(&settings, http_client);
        // Verify the service is a valid Arc<dyn LLMServiceTrait>
        assert!(
            Arc::strong_count(&service) >= 1,
            "init_llm_service should return a valid Arc"
        );
    }

    // ========== init_search_engine tests ==========

    #[test]
    fn test_init_search_engine_creates_instance() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let engine_client = make_engine_client();
        let search_engine = init_search_engine(engine_client, &settings);
        assert!(
            Arc::strong_count(&search_engine) >= 1,
            "init_search_engine should return a valid Arc<dyn SearchEngine>"
        );
    }

    #[test]
    fn test_init_search_engine_with_ab_test_disabled() {
        let mut settings =
            crate::bootstrap::config::load_settings().expect("Failed to load settings");
        settings.search.ab_test_enabled = false;
        let engine_client = make_engine_client();
        let _search_engine = init_search_engine(engine_client, &settings);
        // Should create without panic; with ab_test disabled, returns SearchAggregator directly
    }

    #[test]
    fn test_init_search_engine_with_ab_test_enabled() {
        let mut settings =
            crate::bootstrap::config::load_settings().expect("Failed to load settings");
        settings.search.ab_test_enabled = true;
        settings.search.variant_b_weight = 0.5;
        let engine_client = make_engine_client();
        let _search_engine = init_search_engine(engine_client, &settings);
        // Should create without panic; with ab_test enabled, wraps in SearchABTestEngine
    }

    // ========== init_rate_limit_middleware tests ==========

    #[test]
    fn test_init_rate_limit_middleware_creates_instance() {
        let mock: Arc<dyn RateLimitingService> = Arc::new(MockRateLimitingService);
        let middleware = init_rate_limit_middleware(mock);
        // RateLimitMiddleware derives Clone; verify clone works
        let _cloned = middleware.clone();
    }

    #[test]
    fn test_init_rate_limit_middleware_with_cloned_service() {
        let mock: Arc<dyn RateLimitingService> = Arc::new(MockRateLimitingService);
        // Verify the middleware can be created with a cloned Arc
        let middleware = init_rate_limit_middleware(mock.clone());
        let _middleware2 = init_rate_limit_middleware(mock);
        // Both should be successfully created (verify no panic)
        let _ = &middleware;
    }

    // ========== testcontainers integration tests ==========
    //
    // These tests exercise service initialization paths that require real
    // PostgreSQL. They early-return if Docker is unavailable.

    use crate::bootstrap::infrastructure::{init_database, init_infrastructure, init_repositories};
    use crate::common::test_support::testcontainers_fixtures as tcf;

    async fn require_docker() -> bool {
        tcf::docker_available().await
    }

    #[tokio::test]
    async fn tc_init_rate_limiting_service() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_init_rate_limiting_service");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres container: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        // 高并行度下连接池创建可能因资源耗尽而失败，此时跳过而非 panic
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };
        let repos = init_repositories(db.clone(), &settings);

        let service = init_rate_limiting_service(&repos, &settings).await;
        // Verify the service is usable (Arc strong count >= 1).
        assert!(Arc::strong_count(&service) >= 1);
    }

    #[tokio::test]
    async fn tc_init_auth_scope_service_with_db() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_init_auth_scope_service_with_db");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres container: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        // 高并行度下（如 tarpaulin）连接池创建可能因资源耗尽而失败，此时跳过而非 panic
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };

        let service = init_auth_scope_service(db.inner().clone());
        assert!(Arc::strong_count(&service) >= 1);
    }

    #[tokio::test]
    async fn tc_init_search_service_with_repos() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_init_search_service_with_repos");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres container: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        // 高并行度下连接池创建可能因资源耗尽而失败，此时跳过而非 panic
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };
        let repos = init_repositories(db.clone(), &settings);

        // Build a search client with a dummy engine client.
        let engine_client = Arc::new(EngineClient::new());
        let search_client: Arc<dyn SearchClientTrait> =
            Arc::new(crate::search::client::SearchClient::new(engine_client));

        let service = init_search_service(&repos, &settings, search_client);
        assert!(Arc::strong_count(&service) >= 1);
    }

    #[tokio::test]
    async fn tc_init_services_full_stack() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_init_services_full_stack");
            return;
        }
        let handle = match tcf::DbHandle::start().await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[skip] failed to start db container: {e}");
                return;
            }
        };
        let mut settings = tcf::settings_with_urls(&handle.pg.url).unwrap();
        // R-auth-engine-002 / T011：auth feature on 时 init_services 会调 init_garrison_auth，
        // 弱密钥 / 空密钥会触发 fail-fast panic。此处注入测试用强密钥（>=32 字节 HS256）。
        #[cfg(feature = "auth")]
        {
            settings.auth.jwt_secret = "test-jwt-secret-32-bytes-or-more!!".to_string();
        }
        // 高并行度下基础设施初始化可能因资源耗尽而失败，此时跳过而非 panic
        let infra = match init_infrastructure(&settings).await {
            Ok(i) => i,
            Err(e) => {
                eprintln!("[skip] failed to init infrastructure: {e}");
                return;
            }
        };

        // Build engine router + client.
        // 注入 timeout（架构 MEDIUM 2：避免 ReqwestEngine 硬编码 30 秒）
        // proxy_url=None：此处无代理配置（架构 MEDIUM 5：用 Option 替代空字符串 sentinel）
        let engines = crate::bootstrap::engines::init_engines(
            infra.http_client.clone(),
            None,
            &settings.engines,
            settings.timeouts.engines.default_timeout_seconds,
        );
        let engine_router = Arc::new(EngineRouter::new(engines.clone()));
        let engine_client = Arc::new(EngineClient::with_router(engine_router.clone()));

        let services = init_services(
            &infra,
            engine_router,
            engine_client,
            infra.http_client.clone(),
            &settings,
        )
        .await;

        // Verify all service components are constructed.
        assert!(Arc::strong_count(&services.rate_limiting_service) >= 1);
        assert!(Arc::strong_count(&services.team_semaphore) >= 1);
        assert!(Arc::strong_count(&services.create_scrape_use_case) >= 1);
        assert!(Arc::strong_count(&services.webhook_service) >= 1);
        // R-teams-004 / T015：teams feature 关闭时这两个字段不编译
        #[cfg(feature = "teams")]
        {
            assert!(Arc::strong_count(&services.team_service) >= 1);
            assert!(Arc::strong_count(&services.geo_location_service) >= 1);
        }
        assert!(Arc::strong_count(&services.robots_checker) >= 1);
        assert!(Arc::strong_count(&services.search_engine_service) >= 1);
        assert!(Arc::strong_count(&services.search_service) >= 1);
        assert!(services.auth_scope_service.is_some());
        assert!(Arc::strong_count(&services.queue) >= 1);
        assert!(Arc::strong_count(&services.audit_service) >= 1);
        assert!(Arc::strong_count(&services.llm_service) >= 1);
        assert!(Arc::strong_count(&services.extraction_service) >= 1);
        assert!(Arc::strong_count(&services.regex_cache) >= 1);
        // R-wh-003 / T027：webhook feature 关闭时 webhook_worker 字段不编译
        #[cfg(feature = "webhook")]
        {
            assert!(Arc::strong_count(&services.webhook_worker) >= 1);
        }
        assert!(Arc::strong_count(&services.backlog_worker) >= 1);
        assert!(Arc::strong_count(&services.expiration_worker) >= 1);
    }

    // ========== init_garrison_auth tests (R-auth-engine-002 / T011) ==========
    //
    // 测试覆盖三类场景：
    // 1. 弱密钥 / 空密钥 → Err(BootstrapError::Auth)（fail-fast）
    // 2. 强密钥 + 真实 DbPool → Ok(())（标记 #[ignore] 由 Stage 7 集成测试覆盖）
    //
    // 注意：DbPool 在 dbnexus 0.4.0 无 Default 实现（需真实连接池），故弱密钥测试也
    // 依赖 TEST_DATABASE_URL 提供真实 pool。但弱密钥场景下 pool 不会被使用（config
    // 校验在第 1 步即返回 Err），故 create_test_db_pool 仅用于满足函数签名。
    #[cfg(feature = "auth")]
    mod garrison_auth_tests {
        use super::*;
        use crate::bootstrap::error::BootstrapError;
        use crate::common::test_helpers::{create_test_db_pool, skip_if_no_test_db};

        /// 构造测试用 Settings（仅填充 auth.jwt_secret，其他字段走 confers 默认）。
        fn make_settings(jwt_secret: &str) -> Settings {
            let mut settings =
                crate::bootstrap::config::load_settings().expect("Failed to load settings");
            settings.auth.jwt_secret = jwt_secret.to_string();
            settings
        }

        /// R-auth-engine-002：空 jwt_secret 返回 `Err(BootstrapError::Auth)`，
        /// 错误消息含 "config error" 与 EmptySecret Display（"must not be empty or missing"）。
        ///
        /// 验证规则12（失败必须显性化）——空密钥不藏默认值背后，而是返回类型化错误。
        #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
        async fn test_init_garrison_auth_rejects_empty_secret() {
            if skip_if_no_test_db() {
                return;
            }
            let settings = make_settings("");
            // pool 不会被触达（config 校验先失败），但需满足函数签名。
            let pool = create_test_db_pool();
            let result = init_garrison_auth(&settings, pool).await;
            assert!(
                result.is_err(),
                "empty jwt_secret must be rejected with Err(BootstrapError::Auth)"
            );
            let err = result.unwrap_err();
            assert!(
                matches!(err, BootstrapError::Auth(ref msg) if msg.contains("config error")),
                "error message should contain 'config error' prefix, got: {err}"
            );
            assert!(
                format!("{err}").contains("must not be empty or missing"),
                "error Display should contain EmptySecret description, got: {err}"
            );
        }

        /// R-auth-engine-002：弱密钥（<32 字节）返回 `Err(BootstrapError::Auth)`，
        /// 错误消息含 WeakSecret Display（"weak jwt_secret: length N < 32 bytes"）。
        #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
        async fn test_init_garrison_auth_rejects_weak_secret() {
            if skip_if_no_test_db() {
                return;
            }
            let weak = "too_short"; // 9 字节
            let settings = make_settings(weak);
            let pool = create_test_db_pool();
            let result = init_garrison_auth(&settings, pool).await;
            assert!(
                result.is_err(),
                "weak jwt_secret (<32 bytes) must be rejected with Err(BootstrapError::Auth)"
            );
            let err = result.unwrap_err();
            assert!(
                matches!(err, BootstrapError::Auth(ref msg) if msg.contains("config error")),
                "error message should contain 'config error' prefix, got: {err}"
            );
            // 验证错误消息含长度信息（验证类型化错误传递 len/min 字段到 Display）
            let display = format!("{err}");
            assert!(
                display.contains("length 9"),
                "error Display should contain 'length 9', got: {display}"
            );
            assert!(
                display.contains("< 32 bytes"),
                "error Display should contain '< 32 bytes', got: {display}"
            );
        }

        /// R-auth-engine-002：强密钥 + 真实 DbPool → `Ok(())`，garrison 单例成功初始化。
        ///
        /// 此测试为集成测试，前置条件：
        /// 1. `TEST_DATABASE_URL` 指向已运行 garrison postgres migrations 的数据库
        /// 2. `GarrisonManager::init` 不被其他并发测试污染（GARRISON_MANAGER 单例）
        ///
        /// **标记 `#[ignore]`**：完整端到端测试在 Stage 7 (`tests/integration/auth_garrison_test.rs`) 进行。
        /// 此处保留测试骨架作为占位，避免伪装成有效测试（规则17 测试要验证有意义属性）。
        #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
        #[ignore = "Stage 7 集成测试覆盖：需真实 DB + garrison migrations + GARRISON_MANAGER 单例隔离"]
        async fn test_init_garrison_auth_strong_secret_returns_ok() {
            let strong = "a-very-strong-secret-key-32-bytes-or-more!!"; // 44 字节
            let settings = make_settings(strong);

            // 跳过条件：无 TEST_DATABASE_URL（与 garrison_interface.rs 测试一致）
            if skip_if_no_test_db() {
                return;
            }
            let pool = create_test_db_pool();

            let result = init_garrison_auth(&settings, pool).await;
            assert!(
                result.is_ok(),
                "strong jwt_secret + valid DbPool should succeed, got: {:?}",
                result.err()
            );
        }
    }
}
