// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Application services initialization.

use std::sync::Arc;
use tracing::info;

use crate::application::use_cases::create_scrape::CreateScrapeUseCase;
use crate::bootstrap::infrastructure::InfrastructureComponents;
use crate::bootstrap::infrastructure::Repositories;
use crate::config::settings::Settings;
use crate::domain::services::auth_scope_service::AuthScopeService;
use crate::domain::services::audit_service::AuditService;
use crate::domain::services::rate_limiting_service::{
    ConcurrencyConfig, ConcurrencyStrategy, RateLimitConfig, RateLimitStrategy, RateLimitingService,
};
use crate::domain::services::search_service::{SearchService, SearchServiceTrait};
use crate::domain::services::llm_service::LLMService;
use crate::domain::services::team_service::TeamService;
use crate::engines::engine_client::EngineClient;
use crate::engines::router::EngineRouter;
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::infrastructure::geolocation::GeoLocationService;
use crate::infrastructure::services::rate_limiting_service_impl::{
    RateLimitingConfig, RateLimitingServiceImpl,
};
use crate::infrastructure::services::webhook_service_impl::WebhookServiceImpl;
use crate::presentation::middleware::rate_limit_middleware::RateLimiter;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::queue::task_queue::{PostgresTaskQueue, TaskQueue};
use crate::search::ab_test::SearchABTestEngine;
use crate::search::aggregator::SearchAggregator;
use crate::search::engine_trait::SearchEngine;
use crate::search::smart as smart_search;
use crate::utils::robots::RobotsChecker;
use crate::utils::regex_cache::RegexCache;

/// All application services.
#[derive(Clone)]
pub struct ServicesComponents {
    /// Rate limiter for API requests.
    pub rate_limiter: Arc<RateLimiter>,
    /// Team semaphore for concurrency control.
    pub team_semaphore: Arc<TeamSemaphore>,
    /// Rate limiting service for distributed rate limiting.
    pub rate_limiting_service: Arc<dyn RateLimitingService>,
    /// Create scrape use case.
    pub create_scrape_use_case: Arc<CreateScrapeUseCase>,
    /// Webhook service.
    pub webhook_service: Arc<WebhookServiceImpl>,
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
    pub audit_service: Arc<AuditService>,
    /// HTTP Client
    pub http_client: Arc<reqwest::Client>,
    /// LLM service for LLM operations.
    pub llm_service: Arc<LLMService>,
    /// Regex cache for performance optimization.
    pub regex_cache: Arc<RegexCache>,
}

/// Initialize rate limiter.
///
/// # Arguments
///
/// * `redis_client` - Redis client for distributed rate limiting
/// * `default_rpm` - Default requests per minute limit
///
/// # Returns
///
/// Returns an initialized rate limiter.
pub fn init_rate_limiter(redis_client: Arc<RedisClient>, default_rpm: u32) -> Arc<RateLimiter> {
    Arc::new(RateLimiter::new((*redis_client).clone(), default_rpm))
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
/// * `search_engine` - Search engine instance
///
/// # Returns
///
/// Returns an initialized search service as trait object.
pub fn init_search_service(
    repositories: &Repositories,
    settings: &Settings,
    search_engine: Arc<dyn SearchEngine>,
) -> Arc<dyn SearchServiceTrait> {
    // Create SearchService with concrete repository types
    let service = SearchServiceConcrete::new(
        repositories.crawl_repo.clone(),
        repositories.task_repo.clone(),
        repositories.credits_repo.clone(),
        Arc::new(settings.clone()),
        search_engine,
    );
    Arc::new(service)
}

/// Concrete SearchService type for DI
type SearchServiceConcrete = SearchService<
    crate::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl,
    crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl,
    crate::infrastructure::repositories::credits_repo_impl::CreditsRepositoryImpl,
>;

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
    Arc::new(AuthScopeService::new((*db).clone()))
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
) -> Arc<LLMService> {
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

    // Initialize rate limiter
    let rate_limiter = init_rate_limiter(redis_client.clone(), settings.rate_limiting.default_rpm);

    // Initialize team semaphore
    let team_semaphore = init_team_semaphore(settings.concurrency.default_team_limit as u64);

    // Initialize rate limiting service
    let rate_limiting_service =
        init_rate_limiting_service(redis_client.clone(), repositories, settings);

    // Initialize create scrape use case
    let create_scrape_use_case = Arc::new(CreateScrapeUseCase::new(engine_client.clone()));

    // Initialize webhook service (使用依赖注入的 HTTP_CLIENT)
    let webhook_service = Arc::new(WebhookServiceImpl::new(
        settings.webhook.secret().to_string(),
        http_client.clone(),
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

    // Initialize search engine
    let search_engine_service = init_search_engine(
        Arc::new(EngineClient::with_router(engine_router.clone())),
        settings,
    );

    // Initialize search service
    let search_service = init_search_service(
        repositories,
        settings,
        search_engine_service.clone(),
    );

    // Initialize auth scope service
    let auth_scope_service = Some(init_auth_scope_service(&infrastructure.db));

    // Initialize task queue
    let queue: Arc<dyn TaskQueue> =
        Arc::new(PostgresTaskQueue::new(repositories.task_repo.clone()));

    // Initialize audit service
    let audit_service = Arc::new(AuditService::new(infrastructure.db.clone()));

    // Initialize LLM service (使用依赖注入的 http_client)
    let llm_service = init_llm_service(settings, http_client.clone());

    // Initialize regex cache
    let regex_cache = init_regex_cache();

    info!("Services initialized");

    ServicesComponents {
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
        regex_cache,
    }
}
