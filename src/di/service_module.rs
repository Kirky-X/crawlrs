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
use crate::di::cache_module::RedisClientTrait;
use crate::di::database_module::{HttpClientTrait, SettingsTrait};
use crate::domain::repositories::audit_log_repository::AuditLogRepository;
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
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::search::client::SearchClientTrait;
use crate::utils::robots::RobotsCheckerTrait;

/// Trait for RateLimitingService component
pub trait RateLimitingServiceTrait: Interface + Send + Sync {
    fn get_service(&self) -> &dyn RateLimitingService;
}

/// RateLimitingService component - delegates to RateLimitingServiceImpl
#[cfg(feature = "rate-limiting")]
pub struct RateLimitingServiceComponent {
    /// Inner implementation
    inner: crate::infrastructure::services::rate_limiting_service_impl::RateLimitingServiceImpl,
}

#[cfg(feature = "rate-limiting")]
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
        use crate::domain::services::rate_limiting_service::{
            ConcurrencyConfig, ConcurrencyStrategy, RateLimitConfig, RateLimitStrategy,
        };
        use crate::infrastructure::services::rate_limiting_service_impl::RateLimitingConfig;

        let redis_client_component: Arc<dyn RedisClientTrait> = M::build_component(context);
        let redis_client = redis_client_component.get_client();
        let credits_repo: Arc<dyn CreditsRepository> = M::build_component(context);
        let task_repo: Arc<dyn TaskRepository> = M::build_component(context);
        let tasks_backlog_repo: Arc<dyn TasksBacklogRepository> = M::build_component(context);
        let settings_component: Arc<dyn SettingsTrait> = M::build_component(context);
        let settings = settings_component.get();

        let rate_limit_config = RateLimitConfig {
            strategy: RateLimitStrategy::TokenBucket,
            requests_per_second: settings.rate_limiting.default_rpm / 60,
            requests_per_minute: settings.rate_limiting.default_rpm,
            requests_per_hour: settings.rate_limiting.default_rpm * 60,
            bucket_capacity: Some(settings.rate_limiting.burst_size),
            enabled: settings.rate_limiting.enabled,
        };

        let concurrency_config = ConcurrencyConfig {
            strategy: ConcurrencyStrategy::DistributedSemaphore,
            max_concurrent_tasks: settings.concurrency.default_team_limit as u32,
            max_concurrent_per_team: settings.concurrency.default_team_limit as u32,
            lock_timeout_seconds: settings.concurrency.task_lock_duration_seconds as u64,
            enabled: true,
        };

        let config = RateLimitingConfig {
            redis_key_prefix: "crawlrs".to_string(),
            rate_limit: rate_limit_config,
            concurrency: concurrency_config,
            backlog_process_interval_seconds: 30,
            rate_limit_ttl_seconds: 3600,
        };

        let inner = crate::infrastructure::services::rate_limiting_service_impl::RateLimitingServiceImpl::new(
            redis_client,
            task_repo,
            tasks_backlog_repo,
            credits_repo,
            config,
        );

        Box::new(Self { inner })
    }
}

#[cfg(feature = "rate-limiting")]
impl RateLimitingServiceTrait for RateLimitingServiceComponent {
    fn get_service(&self) -> &dyn RateLimitingService {
        &self.inner
    }
}

#[cfg(feature = "rate-limiting")]
#[async_trait::async_trait]
impl crate::domain::services::rate_limiting_service::RateLimitService
    for RateLimitingServiceComponent
{
    async fn check_rate_limit(
        &self,
        api_key: &str,
        endpoint: &str,
    ) -> Result<
        crate::domain::services::rate_limiting_service::RateLimitResult,
        crate::domain::services::rate_limiting_service::RateLimitingError,
    > {
        self.inner.check_rate_limit(api_key, endpoint).await
    }

    async fn get_team_rate_limit_config(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<
        crate::domain::services::rate_limiting_service::RateLimitConfig,
        crate::domain::services::rate_limiting_service::RateLimitingError,
    > {
        self.inner.get_team_rate_limit_config(team_id).await
    }

    async fn update_team_rate_limit_config(
        &self,
        team_id: uuid::Uuid,
        config: crate::domain::services::rate_limiting_service::RateLimitConfig,
    ) -> Result<(), crate::domain::services::rate_limiting_service::RateLimitingError> {
        self.inner
            .update_team_rate_limit_config(team_id, config)
            .await
    }

    async fn cleanup_expired_rate_limits(
        &self,
    ) -> Result<u64, crate::domain::services::rate_limiting_service::RateLimitingError> {
        self.inner.cleanup_expired_rate_limits().await
    }
}

#[cfg(feature = "rate-limiting")]
#[async_trait::async_trait]
impl crate::domain::services::rate_limiting_service::ConcurrencyControlService
    for RateLimitingServiceComponent
{
    async fn check_team_concurrency(
        &self,
        team_id: uuid::Uuid,
        task_id: uuid::Uuid,
    ) -> Result<
        crate::domain::services::rate_limiting_service::ConcurrencyResult,
        crate::domain::services::rate_limiting_service::RateLimitingError,
    > {
        self.inner.check_team_concurrency(team_id, task_id).await
    }

    async fn release_team_concurrency_slot(
        &self,
        team_id: uuid::Uuid,
        task_id: uuid::Uuid,
    ) -> Result<(), crate::domain::services::rate_limiting_service::RateLimitingError> {
        self.inner
            .release_team_concurrency_slot(team_id, task_id)
            .await
    }

    async fn get_team_current_concurrency(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<u32, crate::domain::services::rate_limiting_service::RateLimitingError> {
        self.inner.get_team_current_concurrency(team_id).await
    }

    async fn get_team_concurrency_config(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<
        crate::domain::services::rate_limiting_service::ConcurrencyConfig,
        crate::domain::services::rate_limiting_service::RateLimitingError,
    > {
        self.inner.get_team_concurrency_config(team_id).await
    }

    async fn update_team_concurrency_config(
        &self,
        team_id: uuid::Uuid,
        config: crate::domain::services::rate_limiting_service::ConcurrencyConfig,
    ) -> Result<(), crate::domain::services::rate_limiting_service::RateLimitingError> {
        self.inner
            .update_team_concurrency_config(team_id, config)
            .await
    }
}

#[cfg(feature = "rate-limiting")]
#[async_trait::async_trait]
impl crate::domain::services::rate_limiting_service::BacklogService
    for RateLimitingServiceComponent
{
    async fn process_backlog_tasks(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<u32, crate::domain::services::rate_limiting_service::RateLimitingError> {
        self.inner.process_backlog_tasks(team_id).await
    }
}

#[cfg(feature = "rate-limiting")]
#[async_trait::async_trait]
impl crate::domain::services::rate_limiting_service::QuotaService for RateLimitingServiceComponent {
    async fn check_and_deduct_quota(
        &self,
        team_id: uuid::Uuid,
        amount: i64,
        transaction_type: crate::domain::models::CreditsTransactionType,
        description: String,
        reference_id: Option<uuid::Uuid>,
    ) -> Result<(), crate::domain::services::rate_limiting_service::RateLimitingError> {
        self.inner
            .check_and_deduct_quota(team_id, amount, transaction_type, description, reference_id)
            .await
    }

    async fn get_quota_balance(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<i64, crate::domain::services::rate_limiting_service::RateLimitingError> {
        self.inner.get_quota_balance(team_id).await
    }
}

#[cfg(feature = "rate-limiting")]
impl RateLimitingService for RateLimitingServiceComponent {}

/// TeamService component
#[derive(Component)]
#[shaku(interface = crate::domain::services::team_service::TeamServiceTrait)]
pub struct TeamServiceComponent {
    /// Geolocation service
    #[shaku(inject)]
    geolocation_service: Arc<dyn crate::domain::services::geo_location::GeoLocationService>,
    /// Geo restriction repository
    #[shaku(inject)]
    geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
}

impl TeamServiceComponent {
    /// Create a new TeamServiceComponent with explicit dependencies
    pub fn new(
        geolocation_service: Arc<dyn crate::domain::services::geo_location::GeoLocationService>,
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
pub trait WebhookServiceTrait: Interface + Send + Sync {
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
        event: &crate::domain::models::WebhookEvent,
    ) -> Result<(), anyhow::Error> {
        self.inner.send_webhook(event).await
    }

    async fn trigger_completion(
        &self,
        task: &crate::domain::models::Task,
    ) -> Result<(), anyhow::Error> {
        self.inner.trigger_completion(task).await
    }

    async fn trigger_failure(
        &self,
        task: &crate::domain::models::Task,
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
pub trait CreateScrapeUseCaseTraitDI: Interface + Send + Sync {
    fn get_use_case(&self) -> &dyn CreateScrapeUseCaseTrait;
}

/// CreateScrapeUseCase component - delegates to CreateScrapeUseCase
pub struct CreateScrapeUseCaseComponent {
    /// Engine client
    engine_client: Arc<crate::engines::engine_client::EngineClient>,
}

impl<M: Module + HasComponent<dyn EngineRouterTrait>> Component<M>
    for CreateScrapeUseCaseComponent
{
    type Interface = dyn CreateScrapeUseCaseTrait;
    type Parameters = ();

    fn build(context: &mut ModuleBuildContext<M>, _: Self::Parameters) -> Box<Self::Interface> {
        let engine_router: Arc<dyn EngineRouterTrait> = M::build_component(context);
        let engine_client = Arc::new(crate::engines::engine_client::EngineClient::with_router(
            engine_router,
        ));
        Box::new(Self::new(engine_client))
    }
}

impl CreateScrapeUseCaseComponent {
    /// Create a new CreateScrapeUseCaseComponent with explicit dependencies
    pub fn new(engine_client: Arc<crate::engines::engine_client::EngineClient>) -> Self {
        Self { engine_client }
    }
}

#[async_trait::async_trait]
impl CreateScrapeUseCaseTrait for CreateScrapeUseCaseComponent {
    async fn execute(
        &self,
        request_dto: crate::application::dto::scrape_request::ScrapeRequestDto,
    ) -> Result<crate::engines::engine_client::ScrapeResponse, crate::domain::models::DomainError>
    {
        let use_case = crate::application::use_cases::create_scrape::CreateScrapeUseCase::new(
            self.engine_client.clone(),
        );
        use_case.execute(request_dto).await
    }
}

impl CreateScrapeUseCaseTraitDI for CreateScrapeUseCaseComponent {
    fn get_use_case(&self) -> &dyn CreateScrapeUseCaseTrait {
        self
    }
}

/// TeamSemaphore component
#[allow(dead_code)]
#[derive(Component, Default)]
#[shaku(interface = TeamSemaphoreTrait)]
pub struct TeamSemaphoreComponent {
    /// Default permits per team
    #[shaku(default = 100)]
    default_permits: usize,
    /// The actual team semaphore (optional, created on demand)
    semaphore: Option<Arc<TeamSemaphore>>,
}

impl TeamSemaphoreComponent {
    /// Create a new TeamSemaphoreComponent with explicit dependencies
    pub fn new(semaphore: Arc<TeamSemaphore>, default_permits: usize) -> Self {
        Self {
            semaphore: Some(semaphore),
            default_permits,
        }
    }

    /// Create with default permits (100)
    pub fn with_defaults() -> Self {
        Self::default()
    }
}

/// Trait for TeamSemaphore component
#[async_trait::async_trait]
pub trait TeamSemaphoreTrait: Interface + Send + Sync {
    /// Get the semaphore
    fn get_semaphore(&self) -> Arc<TeamSemaphore>;
}

#[async_trait::async_trait]
impl TeamSemaphoreTrait for TeamSemaphoreComponent {
    fn get_semaphore(&self) -> Arc<TeamSemaphore> {
        self.semaphore
            .clone()
            .unwrap_or_else(|| Arc::new(TeamSemaphore::new(self.default_permits)))
    }
}

/// Trait for AuditService component
pub trait AuditServiceTraitDI: Send + Sync {
    fn get_service(&self) -> &dyn AuditServiceTrait;
}

/// AuditService component with cached implementation instance
pub struct AuditServiceComponent {
    audit_repo: Arc<dyn AuditLogRepository>,
}

impl<M: Module + HasComponent<dyn AuditLogRepository>> Component<M> for AuditServiceComponent {
    type Interface = dyn AuditServiceTrait;
    type Parameters = ();

    fn build(context: &mut ModuleBuildContext<M>, _: Self::Parameters) -> Box<Self::Interface> {
        let audit_repo: Arc<dyn AuditLogRepository> = M::build_component(context);
        Box::new(Self::new(audit_repo))
    }
}

impl AuditServiceComponent {
    /// Create a new AuditServiceComponent with explicit dependencies
    pub fn new(audit_repo: Arc<dyn AuditLogRepository>) -> Self {
        Self { audit_repo }
    }
}

#[async_trait::async_trait]
impl AuditServiceTrait for AuditServiceComponent {
    async fn log(
        &self,
        entry: crate::domain::auth::AuditLogEntry,
    ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
        self.audit_repo.create(&entry).await?;
        Ok(())
    }

    async fn log_allow(
        &self,
        action: String,
        api_key_id: uuid::Uuid,
        team_id: uuid::Uuid,
        scope: crate::domain::auth::ApiKeyScope,
    ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
        let entry = crate::domain::services::audit_service::AuditLogBuilder::new(
            action,
            crate::domain::auth::AuditDecision::Allow,
        )
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .with_scope(scope)
        .build();
        self.audit_repo.create(&entry).await?;
        Ok(())
    }

    async fn log_deny(
        &self,
        action: String,
        api_key_id: Option<uuid::Uuid>,
        team_id: Option<uuid::Uuid>,
        reason: String,
        scope: Option<crate::domain::auth::ApiKeyScope>,
    ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
        let entry = crate::domain::services::audit_service::AuditLogBuilder::new(
            action,
            crate::domain::auth::AuditDecision::Deny,
        )
        .with_api_key_id(api_key_id.unwrap_or_default())
        .with_team_id(team_id.unwrap_or_default())
        .with_denial_reason(reason)
        .with_scope(scope.unwrap_or_default())
        .build();
        self.audit_repo.create(&entry).await?;
        Ok(())
    }

    async fn get_logs_for_key(
        &self,
        api_key_id: uuid::Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<
        Vec<crate::domain::auth::AuditLogEntry>,
        crate::domain::services::audit_service::AuditServiceError,
    > {
        self.audit_repo
            .find_by_api_key_id(api_key_id, limit, offset)
            .await
            .map_err(Into::into)
    }

    async fn get_logs_for_team(
        &self,
        team_id: uuid::Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<
        Vec<crate::domain::auth::AuditLogEntry>,
        crate::domain::services::audit_service::AuditServiceError,
    > {
        self.audit_repo
            .find_by_team_id(team_id, limit, offset)
            .await
            .map_err(Into::into)
    }

    async fn get_denied_requests(
        &self,
        api_key_id: uuid::Uuid,
        limit: u64,
    ) -> Result<
        Vec<crate::domain::auth::AuditLogEntry>,
        crate::domain::services::audit_service::AuditServiceError,
    > {
        self.audit_repo
            .find_denied_for_key(api_key_id, limit)
            .await
            .map_err(Into::into)
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
#[shaku(interface = crate::domain::services::geo_location::GeoLocationService)]
pub struct GeoLocationServiceComponent {
    /// HTTP client
    #[shaku(inject)]
    http_client: Arc<dyn crate::di::database_module::HttpClientTrait>,
}

impl GeoLocationServiceComponent {
    /// Create a new GeoLocationServiceComponent with explicit dependencies
    pub fn new(http_client: Arc<dyn crate::di::database_module::HttpClientTrait>) -> Self {
        Self { http_client }
    }
}

#[async_trait::async_trait]
impl crate::domain::services::geo_location::GeoLocationService for GeoLocationServiceComponent {
    async fn get_location(
        &self,
        ip: &std::net::IpAddr,
    ) -> anyhow::Result<crate::domain::services::geo_location::GeoLocation> {
        let service = crate::infrastructure::geolocation::GeoLocationServiceImpl::new(
            self.http_client.get().clone(),
        );
        service.get_location(ip).await
    }
}

/// RegexCache component - re-export from utils module
pub type RegexCacheComponent = crate::utils::regex_cache::RegexCacheComponent;
