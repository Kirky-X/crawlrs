// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Service module for Shaku dependency injection.
//!
//! This module provides Shaku components for service layer dependencies
//! including TeamService, WebhookService, and other application services.

use std::sync::Arc;

use shaku::{Component, HasComponent, Interface, Module, ModuleBuildContext};

use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::di::infrastructure_module::{HttpClientTrait, RedisClientTrait, SettingsTrait};
use crate::domain::repositories::auth_scope_repository::AuthScopeRepository;
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::repositories::tasks_backlog_repository::TasksBacklogRepository;
use crate::domain::services::audit_service::AuditServiceTrait;
use crate::domain::services::auth_scope_service::{AuthScopeService, AuthScopeServiceTrait};
use crate::domain::services::llm_service::{
    FileTemplateLoader, LLMService, LLMServiceTrait, TemplateLoaderTrait,
};
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::search_service::{SearchService, SearchServiceTrait};
use crate::domain::services::team_service::TeamService;
use crate::domain::services::team_service::{GeoRestrictionResult, TeamGeoRestrictions};
use crate::domain::services::webhook_service::WebhookService;
use crate::engines::router::EngineRouterTrait;
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::search::client::SearchClientTrait;
use crate::utils::robots::RobotsCheckerTrait;

/// Trait for RateLimitingService component
pub trait RateLimitingServiceTrait: Interface + Send + Sync {
    fn get_service(&self) -> &dyn RateLimitingService;
}

/// RateLimitingService component
#[allow(dead_code)]
pub struct RateLimitingServiceComponent {
    /// Redis client
    redis_client: Arc<RedisClient>,
    /// Credits repo
    credits_repo: Arc<dyn CreditsRepository>,
    /// Task repo
    task_repo: Arc<dyn TaskRepository>,
    /// Tasks backlog repo
    tasks_backlog_repo: Arc<dyn TasksBacklogRepository>,
    /// Rate limit enabled
    enabled: bool,
    /// Default rate limit per second
    default_rate_limit: u32,
    /// Burst size
    burst_size: u32,
}

impl<M: Module> Component<M> for RateLimitingServiceComponent
where
    M: HasComponent<dyn RedisClientTrait>
        + HasComponent<dyn CreditsRepository>
        + HasComponent<dyn TaskRepository>
        + HasComponent<dyn TasksBacklogRepository>
        + HasComponent<dyn SettingsTrait>,
{
    type Interface = dyn RateLimitingServiceTrait;
    type Parameters = ();

    fn build(
        context: &mut ModuleBuildContext<M>,
        _params: Self::Parameters,
    ) -> Box<dyn RateLimitingServiceTrait> {
        let redis_client_component: Arc<dyn RedisClientTrait> = M::build_component(context);
        let redis_client = redis_client_component.get_client();
        let credits_repo: Arc<dyn CreditsRepository> = M::build_component(context);
        let task_repo: Arc<dyn TaskRepository> = M::build_component(context);
        let tasks_backlog_repo: Arc<dyn TasksBacklogRepository> = M::build_component(context);
        let settings_component: Arc<dyn SettingsTrait> = M::build_component(context);
        let settings = settings_component.get();

        let enabled = settings.rate_limiting.enabled;
        let default_rate_limit = settings.rate_limiting.default_limit;
        let burst_size = settings.rate_limiting.burst_size;

        Box::new(Self::new(
            redis_client,
            Arc::from(credits_repo),
            Arc::from(task_repo),
            Arc::from(tasks_backlog_repo),
            enabled,
            default_rate_limit,
            burst_size,
        ))
    }
}

impl RateLimitingServiceComponent {
    /// Create a new RateLimitingServiceComponent with explicit dependencies
    pub fn new(
        redis_client: Arc<RedisClient>,
        credits_repo: Arc<dyn CreditsRepository>,
        task_repo: Arc<dyn TaskRepository>,
        tasks_backlog_repo: Arc<dyn TasksBacklogRepository>,
        enabled: bool,
        default_rate_limit: u32,
        burst_size: u32,
    ) -> Self {
        Self {
            redis_client,
            credits_repo,
            task_repo,
            tasks_backlog_repo,
            enabled,
            default_rate_limit,
            burst_size,
        }
    }
}

impl RateLimitingServiceTrait for RateLimitingServiceComponent {
    fn get_service(&self) -> &dyn RateLimitingService {
        self
    }
}

// Implement individual sub-traits for RateLimitingServiceComponent
#[async_trait::async_trait]
impl crate::domain::services::rate_limiting_service::RateLimitService
    for RateLimitingServiceComponent
{
    async fn check_rate_limit(
        &self,
        _api_key: &str,
        _endpoint: &str,
    ) -> Result<
        crate::domain::services::rate_limiting_service::RateLimitResult,
        crate::domain::services::rate_limiting_service::RateLimitingError,
    > {
        Ok(crate::domain::services::rate_limiting_service::RateLimitResult::Allowed)
    }

    async fn get_team_rate_limit_config(
        &self,
        _team_id: uuid::Uuid,
    ) -> Result<
        crate::domain::services::rate_limiting_service::RateLimitConfig,
        crate::domain::services::rate_limiting_service::RateLimitingError,
    > {
        Ok(
            crate::domain::services::rate_limiting_service::RateLimitConfig {
                strategy:
                    crate::domain::services::rate_limiting_service::RateLimitStrategy::TokenBucket,
                requests_per_second: self.default_rate_limit,
                requests_per_minute: self.default_rate_limit * 60,
                requests_per_hour: self.default_rate_limit * 3600,
                bucket_capacity: Some(self.burst_size),
                enabled: self.enabled,
            },
        )
    }

    async fn update_team_rate_limit_config(
        &self,
        _team_id: uuid::Uuid,
        _config: crate::domain::services::rate_limiting_service::RateLimitConfig,
    ) -> Result<(), crate::domain::services::rate_limiting_service::RateLimitingError> {
        Ok(())
    }

    async fn cleanup_expired_rate_limits(
        &self,
    ) -> Result<u64, crate::domain::services::rate_limiting_service::RateLimitingError> {
        Ok(0)
    }
}

#[async_trait::async_trait]
impl crate::domain::services::rate_limiting_service::ConcurrencyControlService
    for RateLimitingServiceComponent
{
    async fn check_team_concurrency(
        &self,
        _team_id: uuid::Uuid,
        _task_id: uuid::Uuid,
    ) -> Result<
        crate::domain::services::rate_limiting_service::ConcurrencyResult,
        crate::domain::services::rate_limiting_service::RateLimitingError,
    > {
        Ok(crate::domain::services::rate_limiting_service::ConcurrencyResult::Allowed)
    }

    async fn release_team_concurrency_slot(
        &self,
        _team_id: uuid::Uuid,
        _task_id: uuid::Uuid,
    ) -> Result<(), crate::domain::services::rate_limiting_service::RateLimitingError> {
        Ok(())
    }

    async fn get_team_current_concurrency(
        &self,
        _team_id: uuid::Uuid,
    ) -> Result<u32, crate::domain::services::rate_limiting_service::RateLimitingError> {
        Ok(0)
    }

    async fn get_team_concurrency_config(
        &self,
        _team_id: uuid::Uuid,
    ) -> Result<
        crate::domain::services::rate_limiting_service::ConcurrencyConfig,
        crate::domain::services::rate_limiting_service::RateLimitingError,
    > {
        Ok(
            crate::domain::services::rate_limiting_service::ConcurrencyConfig {
                strategy:
                    crate::domain::services::rate_limiting_service::ConcurrencyStrategy::Semaphore,
                max_concurrent_tasks: 100,
                max_concurrent_per_team: 10,
                lock_timeout_seconds: 300,
                enabled: true,
            },
        )
    }

    async fn update_team_concurrency_config(
        &self,
        _team_id: uuid::Uuid,
        _config: crate::domain::services::rate_limiting_service::ConcurrencyConfig,
    ) -> Result<(), crate::domain::services::rate_limiting_service::RateLimitingError> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::domain::services::rate_limiting_service::BacklogService
    for RateLimitingServiceComponent
{
    async fn process_backlog_tasks(
        &self,
        _team_id: uuid::Uuid,
    ) -> Result<u32, crate::domain::services::rate_limiting_service::RateLimitingError> {
        Ok(0)
    }
}

#[async_trait::async_trait]
impl crate::domain::services::rate_limiting_service::QuotaService for RateLimitingServiceComponent {
    async fn check_and_deduct_quota(
        &self,
        _team_id: uuid::Uuid,
        _amount: i64,
        _transaction_type: crate::domain::models::credits::CreditsTransactionType,
        _description: String,
        _reference_id: Option<uuid::Uuid>,
    ) -> Result<(), crate::domain::services::rate_limiting_service::RateLimitingError> {
        Ok(())
    }

    async fn get_quota_balance(
        &self,
        _team_id: uuid::Uuid,
    ) -> Result<i64, crate::domain::services::rate_limiting_service::RateLimitingError> {
        Ok(1000)
    }
}

// RateLimitingService is automatically implemented when all sub-traits are implemented
impl RateLimitingService for RateLimitingServiceComponent {}

/// TeamService component
#[derive(Component)]
#[shaku(interface = crate::domain::services::team_service::TeamServiceTrait)]
pub struct TeamServiceComponent {
    /// Geolocation service
    #[shaku(inject)]
    geolocation_service: Arc<dyn crate::infrastructure::geolocation::GeoLocationServiceTrait>,
    /// Geo restriction repository
    #[shaku(inject)]
    geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
}

impl TeamServiceComponent {
    /// Create a new TeamServiceComponent with explicit dependencies
    pub fn new(
        geolocation_service: Arc<dyn crate::infrastructure::geolocation::GeoLocationServiceTrait>,
        geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
    ) -> Self {
        Self {
            geolocation_service,
            geo_restriction_repo,
        }
    }
}

#[async_trait::async_trait]
impl crate::domain::services::team_service::TeamServiceTrait for TeamServiceComponent {
    async fn validate_geographic_restriction(
        &self,
        team_id: uuid::Uuid,
        ip_address: &str,
        restrictions: &TeamGeoRestrictions,
    ) -> anyhow::Result<GeoRestrictionResult> {
        let service = TeamService::new(
            self.geolocation_service.clone(),
            self.geo_restriction_repo.clone(),
        );
        service
            .validate_geographic_restriction(team_id, ip_address, restrictions)
            .await
    }

    fn validate_domain_blacklist(
        &self,
        domain: &str,
        restrictions: &TeamGeoRestrictions,
    ) -> anyhow::Result<GeoRestrictionResult> {
        let service = TeamService::new(
            self.geolocation_service.clone(),
            self.geo_restriction_repo.clone(),
        );
        service.validate_domain_blacklist(domain, restrictions)
    }

    async fn get_team_geo_restrictions(&self, team_id: uuid::Uuid) -> TeamGeoRestrictions {
        let service = TeamService::new(
            self.geolocation_service.clone(),
            self.geo_restriction_repo.clone(),
        );
        service.get_team_geo_restrictions(team_id).await
    }
}

/// Trait for WebhookService component (delegate to WebhookServiceImpl)
pub trait WebhookServiceTrait: Send + Sync {
    fn get_service(&self) -> &dyn WebhookService;
}

/// WebhookService component - delegates to WebhookServiceImpl via Shaku
#[derive(Component)]
#[shaku(interface = WebhookService)]
pub struct WebhookServiceComponent {
    #[shaku(inject)]
    inner: Arc<dyn WebhookService>,
}

impl WebhookServiceComponent {
    /// Create with explicit inner service
    pub fn new(inner: Arc<dyn WebhookService>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl WebhookService for WebhookServiceComponent {
    async fn send_webhook(
        &self,
        event: &crate::domain::models::webhook::WebhookEvent,
    ) -> Result<(), anyhow::Error> {
        self.inner.send_webhook(event).await
    }

    async fn trigger_completion(
        &self,
        task: &crate::domain::models::task::Task,
    ) -> Result<(), anyhow::Error> {
        self.inner.trigger_completion(task).await
    }

    async fn trigger_failure(
        &self,
        task: &crate::domain::models::task::Task,
        error_msg: String,
    ) -> Result<(), anyhow::Error> {
        self.inner.trigger_failure(task, error_msg).await
    }
}

impl WebhookServiceTrait for WebhookServiceComponent {
    fn get_service(&self) -> &dyn WebhookService {
        self
    }
}

/// Trait for CreateScrapeUseCase component
pub trait CreateScrapeUseCaseTraitDI: Send + Sync {
    fn get_use_case(&self) -> &dyn CreateScrapeUseCaseTrait;
}

/// CreateScrapeUseCase component
#[allow(dead_code)]
pub struct CreateScrapeUseCaseComponent {
    /// Engine router
    engine_router: Arc<dyn EngineRouterTrait>,
}

impl CreateScrapeUseCaseComponent {
    /// Create a new CreateScrapeUseCaseComponent with explicit dependencies
    pub fn new(engine_router: Arc<dyn EngineRouterTrait>) -> Self {
        Self { engine_router }
    }
}

#[async_trait::async_trait]
impl CreateScrapeUseCaseTrait for CreateScrapeUseCaseComponent {
    async fn execute(
        &self,
        _request_dto: crate::application::dto::scrape_request::ScrapeRequestDto,
    ) -> Result<
        crate::engines::engine_client::ScrapeResponse,
        crate::domain::models::task::DomainError,
    > {
        // Placeholder implementation
        Err(crate::domain::models::task::DomainError::EngineError(
            "CreateScrapeUseCase not fully implemented".to_string(),
        ))
    }
}

impl CreateScrapeUseCaseTraitDI for CreateScrapeUseCaseComponent {
    fn get_use_case(&self) -> &dyn CreateScrapeUseCaseTrait {
        self
    }
}

/// Trait for RobotsChecker component
pub trait RobotsCheckerTraitDI: Send + Sync {
    fn get_checker(&self) -> &dyn RobotsCheckerTrait;
}

/// RobotsChecker component
#[allow(dead_code)]
pub struct RobotsCheckerComponent {
    /// HTTP client
    http_client: Arc<reqwest::Client>,
    /// Redis client
    redis_client: Arc<RedisClient>,
}

impl RobotsCheckerComponent {
    /// Create a new RobotsCheckerComponent with explicit dependencies
    pub fn new(http_client: Arc<reqwest::Client>, redis_client: Arc<RedisClient>) -> Self {
        Self {
            http_client,
            redis_client,
        }
    }
}

#[async_trait::async_trait]
impl RobotsCheckerTrait for RobotsCheckerComponent {
    async fn is_allowed(&self, _url_str: &str, _user_agent: &str) -> anyhow::Result<bool> {
        Ok(true)
    }
    async fn get_crawl_delay(
        &self,
        _url_str: &str,
        _user_agent: &str,
    ) -> anyhow::Result<Option<std::time::Duration>> {
        Ok(None)
    }
}

impl RobotsCheckerTraitDI for RobotsCheckerComponent {
    fn get_checker(&self) -> &dyn RobotsCheckerTrait {
        self
    }
}

/// TeamSemaphore component
#[allow(dead_code)]
pub struct TeamSemaphoreComponent {
    /// The actual team semaphore
    semaphore: Arc<TeamSemaphore>,
    /// Default permits per team
    default_permits: usize,
}

impl TeamSemaphoreComponent {
    /// Create a new TeamSemaphoreComponent with explicit dependencies
    pub fn new(semaphore: Arc<TeamSemaphore>, default_permits: usize) -> Self {
        Self {
            semaphore,
            default_permits,
        }
    }

    /// Create with default permits (100)
    pub fn with_defaults() -> Self {
        Self {
            semaphore: Arc::new(TeamSemaphore::new(100)),
            default_permits: 100,
        }
    }
}

/// Trait for TeamSemaphore component
#[async_trait::async_trait]
pub trait TeamSemaphoreTrait: Send + Sync {
    /// Get the semaphore
    fn get_semaphore(&self) -> Arc<TeamSemaphore>;
}

#[async_trait::async_trait]
impl TeamSemaphoreTrait for TeamSemaphoreComponent {
    fn get_semaphore(&self) -> Arc<TeamSemaphore> {
        self.semaphore.clone()
    }
}

/// Trait for AuditService component
pub trait AuditServiceTraitDI: Send + Sync {
    fn get_service(&self) -> &dyn AuditServiceTrait;
}

/// AuditService component
#[derive(Component, Default)]
#[shaku(interface = AuditServiceTrait)]
pub struct AuditServiceComponent {}

#[async_trait::async_trait]
impl AuditServiceTrait for AuditServiceComponent {
    async fn log(
        &self,
        _entry: crate::domain::auth::AuditLogEntry,
    ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
        Ok(())
    }

    async fn log_allow(
        &self,
        _action: String,
        _api_key_id: uuid::Uuid,
        _team_id: uuid::Uuid,
        _scope: crate::domain::auth::ApiKeyScope,
    ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
        Ok(())
    }

    async fn log_deny(
        &self,
        _action: String,
        _api_key_id: Option<uuid::Uuid>,
        _team_id: Option<uuid::Uuid>,
        _reason: String,
        _scope: Option<crate::domain::auth::ApiKeyScope>,
    ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
        Ok(())
    }

    async fn get_logs_for_key(
        &self,
        _api_key_id: uuid::Uuid,
        _limit: u64,
        _offset: u64,
    ) -> Result<
        Vec<crate::domain::auth::AuditLogEntry>,
        crate::domain::services::audit_service::AuditServiceError,
    > {
        Ok(Vec::new())
    }

    async fn get_logs_for_team(
        &self,
        _team_id: uuid::Uuid,
        _limit: u64,
        _offset: u64,
    ) -> Result<
        Vec<crate::domain::auth::AuditLogEntry>,
        crate::domain::services::audit_service::AuditServiceError,
    > {
        Ok(Vec::new())
    }

    async fn get_denied_requests(
        &self,
        _api_key_id: uuid::Uuid,
        _limit: u64,
    ) -> Result<
        Vec<crate::domain::auth::AuditLogEntry>,
        crate::domain::services::audit_service::AuditServiceError,
    > {
        Ok(Vec::new())
    }
}

impl AuditServiceTraitDI for AuditServiceComponent {
    fn get_service(&self) -> &dyn AuditServiceTrait {
        self
    }
}

use crate::utils::robots::RobotsChecker;

// Service module components - for Shaku DI

/// TemplateLoader component
#[allow(dead_code)]
pub struct TemplateLoaderComponent {
    inner: FileTemplateLoader,
}

impl<M: Module> Component<M> for TemplateLoaderComponent {
    type Interface = dyn TemplateLoaderTrait;
    type Parameters = ();

    fn build(_: &mut ModuleBuildContext<M>, _: Self::Parameters) -> Box<Self::Interface> {
        Box::new(Self {
            inner: FileTemplateLoader::default(),
        })
    }
}

impl TemplateLoaderTrait for TemplateLoaderComponent {
    fn load_templates(&self) -> anyhow::Result<std::collections::HashMap<String, String>> {
        self.inner.load_templates()
    }
}

/// RobotsChecker component
impl<M: Module + HasComponent<dyn HttpClientTrait> + HasComponent<dyn RedisClientTrait>>
    Component<M> for RobotsChecker
{
    type Interface = dyn RobotsCheckerTrait;
    type Parameters = ();

    fn build(context: &mut ModuleBuildContext<M>, _: Self::Parameters) -> Box<Self::Interface> {
        let http_client_comp: Arc<dyn HttpClientTrait> = M::build_component(context);
        let redis_client_comp: Arc<dyn RedisClientTrait> = M::build_component(context);

        let http_client = http_client_comp.get();
        let redis_client = redis_client_comp.get_client();

        let checker = RobotsChecker::new(http_client, Some(redis_client), None);
        Box::new(checker)
    }
}

/// LLMService component
impl<
        M: Module
            + HasComponent<dyn SettingsTrait>
            + HasComponent<dyn HttpClientTrait>
            + HasComponent<dyn TemplateLoaderTrait>,
    > Component<M> for LLMService
{
    type Interface = dyn LLMServiceTrait;
    type Parameters = ();

    fn build(context: &mut ModuleBuildContext<M>, _: Self::Parameters) -> Box<Self::Interface> {
        let settings_component: Arc<dyn SettingsTrait> = M::build_component(context);
        let http_client_comp: Arc<dyn HttpClientTrait> = M::build_component(context);
        let template_loader: Arc<dyn TemplateLoaderTrait> = M::build_component(context);

        let settings = settings_component.get();
        let http_client = http_client_comp.get();

        Box::new(LLMService::new_with_template_loader(
            &settings,
            http_client,
            template_loader,
        ))
    }
}

/// AuthScopeService component
#[derive(Component)]
#[shaku(interface = AuthScopeServiceTrait)]
pub struct AuthScopeServiceComponent {
    #[shaku(inject)]
    scope_repo: Arc<dyn AuthScopeRepository>,
}

impl AuthScopeServiceComponent {
    pub fn new(scope_repo: Arc<dyn AuthScopeRepository>) -> Self {
        Self { scope_repo }
    }
}

#[async_trait::async_trait]
impl AuthScopeServiceTrait for AuthScopeServiceComponent {
    async fn get_scope_for_key(
        &self,
        api_key_id: uuid::Uuid,
        team_default_scope: Option<crate::domain::auth::ApiKeyScope>,
    ) -> Result<
        crate::domain::auth::ApiKeyScope,
        crate::domain::services::auth_scope_service::AuthScopeServiceError,
    > {
        let service = AuthScopeService::new(self.scope_repo.clone());
        service
            .get_scope_for_key(api_key_id, team_default_scope)
            .await
    }

    async fn set_scope(
        &self,
        api_key_id: uuid::Uuid,
        scope: crate::domain::auth::ApiKeyScope,
    ) -> Result<
        crate::domain::auth::ApiKeyScope,
        crate::domain::services::auth_scope_service::AuthScopeServiceError,
    > {
        let service = AuthScopeService::new(self.scope_repo.clone());
        service.set_scope(api_key_id, scope).await
    }

    async fn delete_scope(
        &self,
        api_key_id: uuid::Uuid,
    ) -> Result<bool, crate::domain::services::auth_scope_service::AuthScopeServiceError> {
        let service = AuthScopeService::new(self.scope_repo.clone());
        service.delete_scope(api_key_id).await
    }
}

/// SearchService component
pub struct SearchServiceComponent {
    crawl_repo: Arc<dyn CrawlRepository>,
    task_repo: Arc<dyn TaskRepository>,
    credits_repo: Arc<dyn CreditsRepository>,
    search_client: Arc<dyn SearchClientTrait>,
    settings: Arc<crate::config::Settings>,
}

impl<M: Module> Component<M> for SearchServiceComponent
where
    M: HasComponent<dyn CrawlRepository>
        + HasComponent<dyn TaskRepository>
        + HasComponent<dyn CreditsRepository>
        + HasComponent<dyn SearchClientTrait>
        + HasComponent<dyn SettingsTrait>,
{
    type Interface = dyn SearchServiceTrait;
    type Parameters = ();

    fn build(context: &mut ModuleBuildContext<M>, _: Self::Parameters) -> Box<Self::Interface> {
        let crawl_repo: Arc<dyn CrawlRepository> = M::build_component(context);
        let task_repo: Arc<dyn TaskRepository> = M::build_component(context);
        let credits_repo: Arc<dyn CreditsRepository> = M::build_component(context);
        let search_client: Arc<dyn SearchClientTrait> = M::build_component(context);
        let settings_component: Arc<dyn SettingsTrait> = M::build_component(context);
        let settings = settings_component.get();

        Box::new(Self {
            crawl_repo,
            task_repo,
            credits_repo,
            search_client,
            settings,
        })
    }
}

#[async_trait::async_trait]
impl SearchServiceTrait for SearchServiceComponent {
    async fn search(
        &self,
        team_id: uuid::Uuid,
        api_key_id: uuid::Uuid,
        query: crate::domain::services::search_service::SearchQuery,
    ) -> Result<
        crate::domain::services::search_service::SearchResponse,
        crate::domain::services::search_service::SearchServiceError,
    > {
        let service = SearchService::new(
            self.crawl_repo.clone(),
            self.task_repo.clone(),
            self.credits_repo.clone(),
            self.settings.clone(),
            self.search_client.clone(),
        );
        service.search(team_id, api_key_id, query).await
    }
}

/// GeoLocationService component
#[derive(Component)]
#[shaku(interface = crate::infrastructure::geolocation::GeoLocationServiceTrait)]
pub struct GeoLocationServiceComponent {
    /// HTTP client
    #[shaku(inject)]
    http_client: Arc<dyn crate::di::infrastructure_module::HttpClientTrait>,
}

impl GeoLocationServiceComponent {
    /// Create a new GeoLocationServiceComponent with explicit dependencies
    pub fn new(http_client: Arc<dyn crate::di::infrastructure_module::HttpClientTrait>) -> Self {
        Self { http_client }
    }
}

#[async_trait::async_trait]
impl crate::infrastructure::geolocation::GeoLocationServiceTrait for GeoLocationServiceComponent {
    async fn get_location(
        &self,
        ip: &std::net::IpAddr,
    ) -> anyhow::Result<crate::infrastructure::geolocation::GeoLocation> {
        let service =
            crate::infrastructure::geolocation::GeoLocationService::new(self.http_client.get().clone());
        service.get_location(ip).await
    }
}

/// RegexCache component - re-export from utils module
pub type RegexCacheComponent = crate::utils::regex_cache::RegexCacheComponent;
