// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Application services initialization.

use std::sync::Arc;
use tracing::info;

use crate::application::use_cases::create_scrape::{CreateScrapeUseCase, CreateScrapeUseCaseTrait};
use crate::bootstrap::infrastructure::InfrastructureComponents;
use crate::bootstrap::infrastructure::Repositories;
use crate::config::settings::Settings;
use crate::domain::services::audit_service::{AuditService, AuditServiceTrait};
use crate::domain::services::auth_scope_service::AuthScopeService;
use crate::domain::services::extraction_service::{ExtractionService, ExtractionServiceTrait};
use crate::domain::services::llm_service::{LLMService, LLMServiceTrait};
use crate::domain::services::rate_limiting_service::{
    ConcurrencyConfig, ConcurrencyStrategy, RateLimitConfig, RateLimitStrategy, RateLimitingService,
};
use crate::infrastructure::services::rate_limiting_service_impl::{
    RateLimitingConfig, RateLimitingServiceImpl,
};
use crate::domain::services::search_service::{SearchService, SearchServiceTrait};
use crate::search::client::SearchClientTrait;
use crate::domain::services::team_service::TeamService;
use crate::domain::services::webhook_service::{WebhookService, WebhookServiceImpl};
use crate::engines::engine_client::EngineClient;
use crate::engines::router::EngineRouter;
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::infrastructure::database::repositories::audit_log_repo_impl::AuditLogRepositoryImpl;
use crate::infrastructure::database::repositories::auth_scope_repo_impl::AuthScopeRepositoryImpl;
use crate::infrastructure::geolocation::GeoLocationService;
use crate::presentation::middleware::auth_middleware::AuthRateLimiter;
use crate::presentation::middleware::rate_limit_middleware::RateLimitMiddleware;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::queue::task_queue::{PostgresTaskQueue, TaskQueue};
use crate::search::ab_test::SearchABTestEngine;
use crate::search::aggregator::SearchAggregator;
use crate::search::engine_trait::SearchEngine;
use crate::search::smart as smart_search;
use crate::infrastructure::services::webhook_sender_impl::WebhookSenderImpl;
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
    /// Rate limiter (Redis-based)
    pub rate_limiter: Option<Arc<AuthRateLimiter>>,
    /// Create scrape use case.
    pub create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    /// Webhook service.
    pub webhook_service: Arc<dyn WebhookService>,
    /// Team service.
    pub team_service: Arc<TeamService>,
    /// Geo Location Service
    pub geo_location_service: Arc<GeoLocationService>,
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
pub fn init_rate_limit_middleware(rate_limiting_service: Arc<dyn RateLimitingService>) -> RateLimitMiddleware {
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
/// # Arguments
///
/// * `redis_client` - Redis client for distributed rate limiting
/// * `repositories` - Application repositories
/// * `settings` - Application settings
///
/// # Returns
///
/// Returns an initialized rate limiting service.
pub fn init_rate_limiting_service(
    redis_client: Arc<RedisClient>,
    repositories: &Repositories,
    settings: &Settings,
) -> Arc<dyn RateLimitingService> {
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
        tracing::error!("Rate limit configuration error: {}", e);
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
        tracing::error!("Concurrency configuration error: {}", e);
    }

    let rate_limiting_config = RateLimitingConfig {
        redis_key_prefix: "crawlrs".to_string(),
        rate_limit: rate_limit_config,
        concurrency: concurrency_config,
        backlog_process_interval_seconds: 30,
        rate_limit_ttl_seconds: 3600,
    };

    Arc::new(RateLimitingServiceImpl::new(
        redis_client.clone(),
        repositories.task_repo.clone(),
        repositories.tasks_backlog_repo.clone(),
        repositories.credits_repo.clone(),
        rate_limiting_config,
    ))
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
/// This function creates the AuthScopeService for managing API key scopes
/// and permissions, following dependency injection principles.
///
/// # Arguments
///
/// * `db` - Database connection
///
/// # Returns
///
/// Returns an initialized auth scope service wrapped in Arc.
pub fn init_auth_scope_service(db: &sea_orm::DbConn) -> Arc<AuthScopeService> {
    let repo = Arc::new(AuthScopeRepositoryImpl::new((*db).clone()));
    Arc::new(AuthScopeService::new(repo))
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
    Arc::new(RegexCache::new())
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
pub fn init_services(
    infrastructure: &InfrastructureComponents,
    engine_router: Arc<EngineRouter>,
    engine_client: Arc<EngineClient>,
    http_client: Arc<reqwest::Client>,
    settings: &Settings,
) -> ServicesComponents {
    let redis_client = infrastructure.redis_client.clone();
    let repositories = &infrastructure.repositories;

    // Initialize rate limiter (for auth rate limiting)
    let rate_limiter = Some(Arc::new(AuthRateLimiter::new()));

    // Initialize team semaphore
    let team_semaphore = init_team_semaphore(settings.concurrency.default_team_limit as u64);

    // Initialize rate limiting service
    let rate_limiting_service =
        init_rate_limiting_service(redis_client.clone(), repositories, settings);

    // Initialize rate limit middleware
    let rate_limit_middleware = init_rate_limit_middleware(rate_limiting_service.clone());

    // Initialize create scrape use case
    let create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait> =
        Arc::new(CreateScrapeUseCase::new(engine_client.clone()));

    // Initialize webhook service (使用 WebhookSenderImpl)
    let webhook_sender: Arc<WebhookSenderImpl> = Arc::new(WebhookSenderImpl::new(
        http_client.clone(),
        std::time::Duration::from_secs(10),
    ));
    let webhook_service: Arc<WebhookServiceImpl> = Arc::new(WebhookServiceImpl::new(
        webhook_sender.clone(),
        settings.webhook.secret().to_string(),
        repositories.webhook_event_repo.clone(),
    ));

    // Initialize GeoLocationService
    let geo_location_service = Arc::new(
        crate::infrastructure::geolocation::GeoLocationService::new(http_client.clone()),
    );

    // Initialize team service
    let team_service = Arc::new(TeamService::new(
        geo_location_service.clone(),
        repositories.geo_restriction_repo.clone(),
    ));

    // Initialize robots checker (使用依赖注入的 HTTP_CLIENT)
    let robots_checker = Arc::new(RobotsChecker::new(
        http_client.clone(),
        Some(redis_client.clone()),
        None,
    ));

    // Initialize search engine (for backward compatibility)
    let search_engine_service: Arc<dyn SearchEngine> = init_search_engine(
        Arc::new(EngineClient::with_router(engine_router.clone())),
        settings,
    );

    // Initialize search client (wraps search engines)
    let search_client: Arc<dyn SearchClientTrait> = Arc::new(crate::search::client::SearchClient::new(
        Arc::new(EngineClient::with_router(engine_router.clone()))
    ));

    // Initialize search service
    let search_service = init_search_service(repositories, settings, search_client.clone());

    // Initialize auth scope service
    let auth_scope_service = Some(init_auth_scope_service(&infrastructure.db));

    // Initialize task queue
    let queue: Arc<dyn TaskQueue> =
        Arc::new(PostgresTaskQueue::new(repositories.task_repo.clone()));

    // Initialize audit service
    let audit_repo = Arc::new(AuditLogRepositoryImpl::new(infrastructure.db.clone()));
    let audit_service = Arc::new(AuditService::new(audit_repo));

    // Initialize LLM service (使用依赖注入的 http_client)
    let llm_service = init_llm_service(settings, http_client.clone());

    // Initialize extraction service
    let extraction_service = Arc::new(ExtractionService::new(llm_service.clone()));

    // Initialize regex cache
    let regex_cache = init_regex_cache();

    // Use Shaku module to resolve workers
    // This assumes we have an AppModule that we can use or build here.
    // However, since we are initializing services manually here for now,
    // we should update this function to use Shaku module if we want full DI.
    // For now, we will manually construct the workers using the components' new methods
    // or by resolving from a module if we had one built.

    // But wait, the goal is to use Shaku.
    // Let's create the components and use them.

    // Initialize WebhookWorker
    let webhook_worker = Arc::new(crate::workers::webhook_worker::WebhookWorker::new(
        repositories.webhook_event_repo.clone(),
        webhook_service.clone(),
        crate::utils::retry_policy::RetryPolicy::default(),
    ));

    // Initialize BacklogWorker
    // Note: BacklogWorker now expects SettingsTrait, but we have Settings struct.
    // We need to wrap Settings in SettingsComponent or similar if we use manual new.
    // But BacklogWorker::new takes Arc<dyn SettingsTrait>.
    let settings_component = Arc::new(crate::di::infrastructure_module::SettingsComponent::new(
        Arc::new(settings.clone()),
    ));

    let backlog_worker = Arc::new(crate::workers::backlog_worker::BacklogWorker::new(
        repositories.tasks_backlog_repo.clone(),
        repositories.task_repo.clone(),
        rate_limiting_service.clone(),
        settings_component,
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
        team_service,
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
        webhook_worker,
        backlog_worker,
        expiration_worker,
    }
}
