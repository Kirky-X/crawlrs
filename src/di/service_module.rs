// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Service module for dependency injection.
//!
//! This module provides components for service layer dependencies
//! including TeamService, WebhookService, and other application services.

use std::sync::Arc;

use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::domain::repositories::audit_log_repository::AuditLogRepository;
use crate::domain::repositories::auth_scope_repository::AuthScopeRepository;
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::audit_service::AuditServiceTrait;
use crate::domain::services::auth_scope_service::{AuthScopeService, AuthScopeServiceTrait};
use crate::domain::services::llm_service::{FileTemplateLoader, TemplateLoaderTrait};
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::search_service::{SearchService, SearchServiceTrait};
use crate::domain::services::team_service::TeamService;
use crate::domain::services::team_service::{GeoRestrictionResult, TeamGeoRestrictions};
use crate::domain::services::webhook_service::WebhookService;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::search::client::SearchClientTrait;

#[cfg(test)]
use crate::di::database_module::HttpClientTrait;
#[cfg(test)]
use crate::engines::router::EngineRouterTrait;

/// Trait for RateLimitingService component
pub trait RateLimitingServiceTrait: Send + Sync {
    fn get_service(&self) -> &dyn RateLimitingService;
}

/// RateLimitingService component - delegates to LimiteronService
#[cfg(feature = "rate-limiting")]
pub struct RateLimitingServiceComponent {
    /// Inner implementation
    inner: crate::infrastructure::services::limiteron_service::LimiteronService,
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
pub struct TeamServiceComponent {
    /// Geolocation service
    geolocation_service: Arc<dyn crate::domain::services::geo_location::GeoLocationService>,
    /// Geo restriction repository
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
pub trait WebhookServiceTrait: Send + Sync {
    fn get_service(&self) -> &dyn WebhookService;
}

/// WebhookService component - delegates to WebhookServiceImpl
pub struct WebhookServiceComponent {
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
pub trait CreateScrapeUseCaseTraitDI: Send + Sync {
    fn get_use_case(&self) -> &dyn CreateScrapeUseCaseTrait;
}

/// CreateScrapeUseCase component - delegates to CreateScrapeUseCase
pub struct CreateScrapeUseCaseComponent {
    /// Engine client
    engine_client: Arc<crate::engines::engine_client::EngineClient>,
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
#[derive(Default)]
pub struct TeamSemaphoreComponent {
    /// Default permits per team
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
pub trait TeamSemaphoreTrait: Send + Sync {
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

// Service module components

/// TemplateLoader component
#[allow(dead_code)]
pub struct TemplateLoaderComponent {
    inner: FileTemplateLoader,
}

impl TemplateLoaderTrait for TemplateLoaderComponent {
    fn load_templates(&self) -> anyhow::Result<std::collections::HashMap<String, String>> {
        self.inner.load_templates()
    }
}

/// AuthScopeService component
pub struct AuthScopeServiceComponent {
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
pub struct GeoLocationServiceComponent {
    /// HTTP client
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::di::engines_module::EngineRouterComponent;

    // ========== TeamSemaphoreComponent ==========

    #[test]
    fn test_team_semaphore_component_new_stores_semaphore() {
        let semaphore = Arc::new(TeamSemaphore::new(50));
        let component = TeamSemaphoreComponent::new(semaphore.clone(), 50);
        let retrieved = component.get_semaphore();
        assert!(Arc::ptr_eq(&retrieved, &semaphore));
    }

    #[test]
    fn test_team_semaphore_component_new_with_different_permits() {
        let semaphore = Arc::new(TeamSemaphore::new(200));
        let component = TeamSemaphoreComponent::new(semaphore.clone(), 200);
        let retrieved = component.get_semaphore();
        assert!(Arc::ptr_eq(&retrieved, &semaphore));
    }

    #[test]
    fn test_team_semaphore_component_with_defaults_creates_on_demand() {
        let component = TeamSemaphoreComponent::with_defaults();
        // with_defaults() 内部调用 default()，semaphore 字段为 None
        // get_semaphore() 应按需创建一个新的 TeamSemaphore
        let sem1 = component.get_semaphore();
        let sem2 = component.get_semaphore();
        // 由于 semaphore 为 None，每次调用 get_semaphore() 都会创建新的 Arc
        // 因此两次调用不应指向同一对象
        assert!(!Arc::ptr_eq(&sem1, &sem2));
    }

    #[test]
    fn test_team_semaphore_component_as_trait_object() {
        let semaphore = Arc::new(TeamSemaphore::new(10));
        let component = TeamSemaphoreComponent::new(semaphore.clone(), 10);
        let trait_obj: &dyn TeamSemaphoreTrait = &component;
        let retrieved = trait_obj.get_semaphore();
        assert!(Arc::ptr_eq(&retrieved, &semaphore));
    }

    // ========== CreateScrapeUseCaseComponent ==========

    #[test]
    fn test_create_scrape_use_case_component_new() {
        let engine_client = Arc::new(crate::engines::engine_client::EngineClient::new());
        let component = CreateScrapeUseCaseComponent::new(engine_client);
        // get_use_case() 应返回组件自身作为 trait 对象
        let _use_case: &dyn CreateScrapeUseCaseTrait = component.get_use_case();
    }

    #[test]
    fn test_create_scrape_use_case_component_as_trait_object() {
        let engine_client = Arc::new(crate::engines::engine_client::EngineClient::new());
        let component = CreateScrapeUseCaseComponent::new(engine_client);
        let trait_obj: &dyn CreateScrapeUseCaseTrait = &component;
        let di_obj: &dyn CreateScrapeUseCaseTraitDI = &component;
        // 两个 trait 对象应指向同一底层组件
        let _from_trait = trait_obj;
        let _from_di = di_obj.get_use_case();
    }

    // ========== GeoLocationServiceComponent ==========

    #[test]
    fn test_geo_location_service_component_new() {
        let http_client: Arc<dyn HttpClientTrait> = Arc::new(
            crate::di::database_module::HttpClientComponent::new(Arc::new(reqwest::Client::new())),
        );
        let component = GeoLocationServiceComponent::new(http_client);
        // 组件构造成功即可验证，get_location 需要网络访问故不在此测试
        let _trait_obj: &dyn crate::domain::services::geo_location::GeoLocationService = &component;
    }

    // ========== EngineClientComponent (via engines_module) ==========
    // 以下测试验证 service_module 中引用的 EngineRouterComponent 可正常构造

    #[test]
    fn test_engine_router_component_integration() {
        // 验证 EngineRouterComponent 可用于构造 CreateScrapeUseCaseComponent 的依赖链
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouterComponent::with_defaults());
        let engine_client = Arc::new(crate::engines::engine_client::EngineClient::with_router(
            router,
        ));
        let component = CreateScrapeUseCaseComponent::new(engine_client);
        let _use_case = component.get_use_case();
    }

    // ========== WebhookServiceComponent ==========
    // WebhookServiceComponent 接受 Arc<dyn WebhookService>，可用 mock 实现验证
    // send_webhook/trigger_completion/trigger_failure 的委托逻辑以及 get_service 访问器。

    use crate::domain::auth::{AuditDecision, AuditLogEntry};
    use crate::domain::models::{Task, TaskType, WebhookEvent, WebhookEventType};
    use crate::domain::repositories::audit_log_repository::{
        AuditLogRepository, AuditRepositoryError,
    };
    use crate::domain::repositories::auth_scope_repository::{
        AuthScopeRepository, RepositoryError as AuthScopeRepoError,
    };
    use crate::domain::services::audit_service::AuditServiceError;
    use crate::domain::services::auth_scope_service::AuthScopeServiceError;

    /// Mock WebhookService 用于测试 WebhookServiceComponent 的委托逻辑。
    struct MockWebhookService {
        send_count: std::sync::atomic::AtomicU32,
        completion_count: std::sync::atomic::AtomicU32,
        failure_count: std::sync::atomic::AtomicU32,
        should_fail: bool,
    }

    impl MockWebhookService {
        fn success() -> Self {
            Self {
                send_count: std::sync::atomic::AtomicU32::new(0),
                completion_count: std::sync::atomic::AtomicU32::new(0),
                failure_count: std::sync::atomic::AtomicU32::new(0),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                send_count: std::sync::atomic::AtomicU32::new(0),
                completion_count: std::sync::atomic::AtomicU32::new(0),
                failure_count: std::sync::atomic::AtomicU32::new(0),
                should_fail: true,
            }
        }
    }

    #[async_trait::async_trait]
    impl WebhookService for MockWebhookService {
        async fn send_webhook(&self, _event: &WebhookEvent) -> Result<(), anyhow::Error> {
            self.send_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.should_fail {
                return Err(anyhow::anyhow!("send_webhook failed"));
            }
            Ok(())
        }

        async fn trigger_completion(&self, _task: &Task) -> Result<(), anyhow::Error> {
            self.completion_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.should_fail {
                return Err(anyhow::anyhow!("trigger_completion failed"));
            }
            Ok(())
        }

        async fn trigger_failure(
            &self,
            _task: &Task,
            _error_msg: String,
        ) -> Result<(), anyhow::Error> {
            self.failure_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.should_fail {
                return Err(anyhow::anyhow!("trigger_failure failed"));
            }
            Ok(())
        }
    }

    fn make_test_webhook_event() -> WebhookEvent {
        WebhookEvent::new(
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            WebhookEventType::ScrapeCompleted,
            serde_json::Value::Null,
            "https://example.com/webhook".to_string(),
        )
    }

    fn make_test_task_for_service() -> Task {
        Task::new(
            uuid::Uuid::new_v4(),
            TaskType::Scrape,
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::Value::Null,
        )
    }

    #[test]
    fn test_webhook_service_component_new_stores_inner() {
        let inner: Arc<dyn WebhookService> = Arc::new(MockWebhookService::success());
        let component = WebhookServiceComponent::new(inner);
        let _trait_obj: &dyn WebhookService = &component;
    }

    #[test]
    fn test_webhook_service_component_get_service_returns_self() {
        let inner: Arc<dyn WebhookService> = Arc::new(MockWebhookService::success());
        let component = WebhookServiceComponent::new(inner);
        let di_obj: &dyn WebhookServiceTrait = &component;
        let _service: &dyn WebhookService = di_obj.get_service();
    }

    #[tokio::test]
    async fn test_webhook_service_component_send_webhook_delegates_to_inner() {
        let mock = Arc::new(MockWebhookService::success());
        let component = WebhookServiceComponent::new(mock.clone() as Arc<dyn WebhookService>);
        let event = make_test_webhook_event();
        let result = component.send_webhook(&event).await;
        assert!(result.is_ok());
        assert_eq!(mock.send_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_webhook_service_component_trigger_completion_delegates_to_inner() {
        let mock = Arc::new(MockWebhookService::success());
        let component = WebhookServiceComponent::new(mock.clone() as Arc<dyn WebhookService>);
        let task = make_test_task_for_service();
        let result = component.trigger_completion(&task).await;
        assert!(result.is_ok());
        assert_eq!(
            mock.completion_count
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn test_webhook_service_component_trigger_failure_delegates_to_inner() {
        let mock = Arc::new(MockWebhookService::success());
        let component = WebhookServiceComponent::new(mock.clone() as Arc<dyn WebhookService>);
        let task = make_test_task_for_service();
        let result = component
            .trigger_failure(&task, "timeout".to_string())
            .await;
        assert!(result.is_ok());
        assert_eq!(
            mock.failure_count.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn test_webhook_service_component_send_webhook_propagates_error() {
        let mock = Arc::new(MockWebhookService::failing());
        let component = WebhookServiceComponent::new(mock.clone() as Arc<dyn WebhookService>);
        let event = make_test_webhook_event();
        let result = component.send_webhook(&event).await;
        assert!(result.is_err());
    }

    // ========== AuditServiceComponent ==========
    // AuditServiceComponent 接受 Arc<dyn AuditLogRepository>，可用 mock 实现验证
    // log/log_allow/log_deny/get_logs_for_key/get_logs_for_team/get_denied_requests
    // 的委托逻辑，以及 AuditLogBuilder 在组件内部的构建行为。

    use std::sync::Mutex;

    /// Mock AuditLogRepository 用于测试 AuditServiceComponent。
    /// 记录最近一次 create 收到的 AuditLogEntry 以验证 builder 构建结果。
    struct MockAuditLogRepository {
        create_count: std::sync::atomic::AtomicU32,
        last_entry: Mutex<Option<AuditLogEntry>>,
        should_fail: bool,
    }

    impl MockAuditLogRepository {
        fn success() -> Self {
            Self {
                create_count: std::sync::atomic::AtomicU32::new(0),
                last_entry: Mutex::new(None),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                create_count: std::sync::atomic::AtomicU32::new(0),
                last_entry: Mutex::new(None),
                should_fail: true,
            }
        }

        fn last_entry(&self) -> Option<AuditLogEntry> {
            self.last_entry
                .lock()
                .expect("last_entry mutex poisoned")
                .clone()
        }
    }

    #[async_trait::async_trait]
    impl AuditLogRepository for MockAuditLogRepository {
        async fn create(
            &self,
            entry: &AuditLogEntry,
        ) -> Result<AuditLogEntry, AuditRepositoryError> {
            self.create_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.last_entry.lock().expect("last_entry mutex poisoned") = Some(entry.clone());
            if self.should_fail {
                return Err(AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(
                    "create failed".to_string(),
                )));
            }
            Ok(entry.clone())
        }

        async fn find_by_api_key_id(
            &self,
            _api_key_id: uuid::Uuid,
            _limit: u64,
            _offset: u64,
        ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError> {
            if self.should_fail {
                return Err(AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(
                    "find_by_api_key_id failed".to_string(),
                )));
            }
            Ok(vec![])
        }

        async fn find_by_team_id(
            &self,
            _team_id: uuid::Uuid,
            _limit: u64,
            _offset: u64,
        ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError> {
            if self.should_fail {
                return Err(AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(
                    "find_by_team_id failed".to_string(),
                )));
            }
            Ok(vec![])
        }

        async fn find_denied_for_key(
            &self,
            _api_key_id: uuid::Uuid,
            _limit: u64,
        ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError> {
            if self.should_fail {
                return Err(AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(
                    "find_denied_for_key failed".to_string(),
                )));
            }
            Ok(vec![])
        }

        async fn cleanup_old_logs(
            &self,
            _retention_days: i64,
        ) -> Result<u64, AuditRepositoryError> {
            Ok(0)
        }
    }

    #[test]
    fn test_audit_service_component_new_stores_repo() {
        let repo: Arc<dyn AuditLogRepository> = Arc::new(MockAuditLogRepository::success());
        let component = AuditServiceComponent::new(repo);
        let _trait_obj: &dyn AuditServiceTrait = &component;
    }

    #[test]
    fn test_audit_service_component_get_service_returns_self() {
        let repo: Arc<dyn AuditLogRepository> = Arc::new(MockAuditLogRepository::success());
        let component = AuditServiceComponent::new(repo);
        let di_obj: &dyn AuditServiceTraitDI = &component;
        let _service: &dyn AuditServiceTrait = di_obj.get_service();
    }

    #[tokio::test]
    async fn test_audit_service_component_log_calls_create() {
        let mock = Arc::new(MockAuditLogRepository::success());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let entry = AuditLogEntry {
            id: uuid::Uuid::new_v4(),
            api_key_id: Some(uuid::Uuid::new_v4()),
            team_id: Some(uuid::Uuid::new_v4()),
            requested_action: "scrape".to_string(),
            decision: AuditDecision::Allow,
            denial_reason: None,
            scope_used: None,
            ip_address: None,
            trace_id: None,
            user_agent: None,
            request_path: None,
            request_method: None,
            metadata: serde_json::Value::Null,
            created_at: chrono::Utc::now(),
        };
        let result = component.log(entry).await;
        assert!(result.is_ok());
        assert_eq!(
            mock.create_count.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn test_audit_service_component_log_propagates_error() {
        let mock = Arc::new(MockAuditLogRepository::failing());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let entry = AuditLogEntry {
            id: uuid::Uuid::new_v4(),
            api_key_id: None,
            team_id: None,
            requested_action: "search".to_string(),
            decision: AuditDecision::Deny,
            denial_reason: Some("forbidden".to_string()),
            scope_used: None,
            ip_address: None,
            trace_id: None,
            user_agent: None,
            request_path: None,
            request_method: None,
            metadata: serde_json::Value::Null,
            created_at: chrono::Utc::now(),
        };
        let result = component.log(entry).await;
        assert!(result.is_err());
        match result {
            Err(AuditServiceError::RepositoryError(_)) => {}
            other => panic!(
                "Expected AuditServiceError::RepositoryError, got {:?}",
                other
            ),
        }
    }

    #[tokio::test]
    async fn test_audit_service_component_log_allow_builds_entry_with_allow_decision() {
        let mock = Arc::new(MockAuditLogRepository::success());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let api_key_id = uuid::Uuid::new_v4();
        let team_id = uuid::Uuid::new_v4();
        let scope = crate::domain::auth::ApiKeyScope::default();
        let result = component
            .log_allow("scrape".to_string(), api_key_id, team_id, scope)
            .await;
        assert!(result.is_ok());
        let last = mock.last_entry().expect("entry should have been created");
        assert_eq!(last.requested_action, "scrape");
        assert_eq!(last.decision, AuditDecision::Allow);
        assert_eq!(last.api_key_id, Some(api_key_id));
        assert_eq!(last.team_id, Some(team_id));
    }

    #[tokio::test]
    async fn test_audit_service_component_log_deny_builds_entry_with_deny_decision() {
        let mock = Arc::new(MockAuditLogRepository::success());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let api_key_id = uuid::Uuid::new_v4();
        let team_id = uuid::Uuid::new_v4();
        let result = component
            .log_deny(
                "delete".to_string(),
                Some(api_key_id),
                Some(team_id),
                "insufficient scope".to_string(),
                None,
            )
            .await;
        assert!(result.is_ok());
        let last = mock.last_entry().expect("entry should have been created");
        assert_eq!(last.requested_action, "delete");
        assert_eq!(last.decision, AuditDecision::Deny);
        assert_eq!(last.api_key_id, Some(api_key_id));
        assert_eq!(last.team_id, Some(team_id));
        assert_eq!(last.denial_reason.as_deref(), Some("insufficient scope"));
    }

    #[tokio::test]
    async fn test_audit_service_component_log_deny_uses_defaults_for_none_ids() {
        let mock = Arc::new(MockAuditLogRepository::success());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let result = component
            .log_deny(
                "search".to_string(),
                None,
                None,
                "no api key".to_string(),
                None,
            )
            .await;
        assert!(result.is_ok());
        let last = mock.last_entry().expect("entry should have been created");
        // unwrap_or_default() 应产生 nil UUID
        assert_eq!(last.api_key_id, Some(uuid::Uuid::nil()));
        assert_eq!(last.team_id, Some(uuid::Uuid::nil()));
    }

    #[tokio::test]
    async fn test_audit_service_component_get_logs_for_key_delegates_to_repo() {
        let mock = Arc::new(MockAuditLogRepository::success());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let result = component
            .get_logs_for_key(uuid::Uuid::new_v4(), 10, 0)
            .await;
        assert!(result.is_ok());
        let logs = result.expect("logs result");
        assert!(logs.is_empty());
    }

    #[tokio::test]
    async fn test_audit_service_component_get_logs_for_team_delegates_to_repo() {
        let mock = Arc::new(MockAuditLogRepository::success());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let result = component
            .get_logs_for_team(uuid::Uuid::new_v4(), 10, 0)
            .await;
        assert!(result.is_ok());
        let logs = result.expect("logs result");
        assert!(logs.is_empty());
    }

    #[tokio::test]
    async fn test_audit_service_component_get_denied_requests_delegates_to_repo() {
        let mock = Arc::new(MockAuditLogRepository::success());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let result = component.get_denied_requests(uuid::Uuid::new_v4(), 5).await;
        assert!(result.is_ok());
        let logs = result.expect("denied logs result");
        assert!(logs.is_empty());
    }

    // ========== AuthScopeServiceComponent ==========
    // AuthScopeServiceComponent 接受 Arc<dyn AuthScopeRepository>，可用 mock 实现验证
    // get_scope_for_key/set_scope/delete_scope 的委托逻辑。

    use crate::domain::auth::ApiKeyScope;

    /// Mock AuthScopeRepository 用于测试 AuthScopeServiceComponent。
    struct MockAuthScopeRepository {
        stored_scope: Mutex<Option<ApiKeyScope>>,
        should_fail: bool,
    }

    impl MockAuthScopeRepository {
        fn with_scope(scope: ApiKeyScope) -> Self {
            Self {
                stored_scope: Mutex::new(Some(scope)),
                should_fail: false,
            }
        }

        fn empty() -> Self {
            Self {
                stored_scope: Mutex::new(None),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                stored_scope: Mutex::new(None),
                should_fail: true,
            }
        }
    }

    #[async_trait::async_trait]
    impl AuthScopeRepository for MockAuthScopeRepository {
        async fn find_by_api_key_id(
            &self,
            _api_key_id: uuid::Uuid,
        ) -> Result<Option<ApiKeyScope>, AuthScopeRepoError> {
            if self.should_fail {
                return Err(AuthScopeRepoError::Database(sea_orm::DbErr::Custom(
                    "find failed".to_string(),
                )));
            }
            Ok(self
                .stored_scope
                .lock()
                .expect("stored_scope mutex poisoned")
                .clone())
        }

        async fn find_by_api_key(
            &self,
            _key: &str,
        ) -> Result<Option<ApiKeyScope>, AuthScopeRepoError> {
            Ok(self
                .stored_scope
                .lock()
                .expect("stored_scope mutex poisoned")
                .clone())
        }

        async fn upsert(
            &self,
            _api_key_id: uuid::Uuid,
            scope: ApiKeyScope,
        ) -> Result<ApiKeyScope, AuthScopeRepoError> {
            if self.should_fail {
                return Err(AuthScopeRepoError::Database(sea_orm::DbErr::Custom(
                    "upsert failed".to_string(),
                )));
            }
            *self
                .stored_scope
                .lock()
                .expect("stored_scope mutex poisoned") = Some(scope.clone());
            Ok(scope)
        }

        async fn delete_by_api_key_id(
            &self,
            _api_key_id: uuid::Uuid,
        ) -> Result<bool, AuthScopeRepoError> {
            if self.should_fail {
                return Err(AuthScopeRepoError::Database(sea_orm::DbErr::Custom(
                    "delete failed".to_string(),
                )));
            }
            let mut guard = self
                .stored_scope
                .lock()
                .expect("stored_scope mutex poisoned");
            let existed = guard.is_some();
            *guard = None;
            Ok(existed)
        }
    }

    #[test]
    fn test_auth_scope_service_component_new_stores_repo() {
        let repo: Arc<dyn AuthScopeRepository> = Arc::new(MockAuthScopeRepository::empty());
        let component = AuthScopeServiceComponent::new(repo);
        let _trait_obj: &dyn AuthScopeServiceTrait = &component;
    }

    #[tokio::test]
    async fn test_auth_scope_service_component_get_scope_for_key_returns_stored_scope() {
        let scope = ApiKeyScope::default();
        let repo: Arc<dyn AuthScopeRepository> =
            Arc::new(MockAuthScopeRepository::with_scope(scope));
        let component = AuthScopeServiceComponent::new(repo);
        let result = component
            .get_scope_for_key(uuid::Uuid::new_v4(), None)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_auth_scope_service_component_get_scope_for_key_uses_default_when_not_found() {
        let repo: Arc<dyn AuthScopeRepository> = Arc::new(MockAuthScopeRepository::empty());
        let component = AuthScopeServiceComponent::new(repo);
        let default_scope = ApiKeyScope::default();
        let result = component
            .get_scope_for_key(uuid::Uuid::new_v4(), Some(default_scope))
            .await;
        // 当仓库返回 None 时，AuthScopeService 使用 team_default_scope 作为后备
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_auth_scope_service_component_set_scope_delegates_to_repo() {
        let repo: Arc<dyn AuthScopeRepository> = Arc::new(MockAuthScopeRepository::empty());
        let component = AuthScopeServiceComponent::new(repo);
        let scope = ApiKeyScope::default();
        let result = component.set_scope(uuid::Uuid::new_v4(), scope).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_auth_scope_service_component_delete_scope_delegates_to_repo() {
        let repo: Arc<dyn AuthScopeRepository> =
            Arc::new(MockAuthScopeRepository::with_scope(ApiKeyScope::default()));
        let component = AuthScopeServiceComponent::new(repo);
        let result = component.delete_scope(uuid::Uuid::new_v4()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_auth_scope_service_component_set_scope_propagates_error() {
        let repo: Arc<dyn AuthScopeRepository> = Arc::new(MockAuthScopeRepository::failing());
        let component = AuthScopeServiceComponent::new(repo);
        let result = component
            .set_scope(uuid::Uuid::new_v4(), ApiKeyScope::default())
            .await;
        assert!(result.is_err());
        match result {
            Err(AuthScopeServiceError::DatabaseError(_)) => {}
            other => panic!(
                "Expected AuthScopeServiceError::DatabaseError, got {:?}",
                other
            ),
        }
    }

    // ========== TeamServiceComponent ==========
    // TeamServiceComponent 接受 Arc<dyn GeoLocationService> 和 Arc<dyn GeoRestrictionRepository>，
    // 可用 mock 实现验证 validate_domain_blacklist / validate_geographic_restriction /
    // get_team_geo_restrictions 的委托逻辑。

    use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError;
    use crate::domain::services::geo_location::{GeoLocation, GeoLocationService};
    use crate::domain::services::team_service::TeamServiceTrait;
    use std::net::IpAddr;

    /// Mock GeoLocationService 用于测试 TeamServiceComponent。
    /// 返回可配置的国家代码。
    struct MockGeoLocationService {
        country_code: String,
    }

    impl MockGeoLocationService {
        fn new(country_code: &str) -> Self {
            Self {
                country_code: country_code.to_string(),
            }
        }
    }

    #[async_trait::async_trait]
    impl GeoLocationService for MockGeoLocationService {
        async fn get_location(&self, _ip: &IpAddr) -> anyhow::Result<GeoLocation> {
            Ok(GeoLocation {
                country_code: self.country_code.clone(),
                ..Default::default()
            })
        }
    }

    /// Mock GeoRestrictionRepository 用于测试 TeamServiceComponent。
    /// 返回可配置的地理限制配置。
    struct MockGeoRestrictionRepository {
        restrictions: Mutex<TeamGeoRestrictions>,
    }

    impl MockGeoRestrictionRepository {
        fn with_restrictions(restrictions: TeamGeoRestrictions) -> Self {
            Self {
                restrictions: Mutex::new(restrictions),
            }
        }

        fn default_restrictions() -> Self {
            Self {
                restrictions: Mutex::new(TeamGeoRestrictions::default()),
            }
        }
    }

    #[async_trait::async_trait]
    impl GeoRestrictionRepository for MockGeoRestrictionRepository {
        async fn get_team_restrictions(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<TeamGeoRestrictions, GeoRestrictionRepositoryError> {
            Ok(self
                .restrictions
                .lock()
                .expect("restrictions mutex poisoned")
                .clone())
        }

        async fn update_team_restrictions(
            &self,
            _team_id: uuid::Uuid,
            _restrictions: &TeamGeoRestrictions,
        ) -> Result<(), GeoRestrictionRepositoryError> {
            Ok(())
        }

        async fn log_geo_restriction_action(
            &self,
            _team_id: uuid::Uuid,
            _ip_address: &str,
            _country_code: &str,
            _action: &str,
            _reason: &str,
        ) -> Result<(), GeoRestrictionRepositoryError> {
            Ok(())
        }
    }

    #[test]
    fn test_team_service_component_new_stores_dependencies() {
        let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoLocationService::new("US"));
        let geo_repo: Arc<dyn GeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::default_restrictions());
        let component = TeamServiceComponent::new(geo_service, geo_repo);
        let _trait_obj: &dyn TeamServiceTrait = &component;
    }

    #[test]
    fn test_team_service_component_validate_domain_blacklist_denies_blacklisted_domain() {
        let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoLocationService::new("US"));
        let geo_repo: Arc<dyn GeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::default_restrictions());
        let component = TeamServiceComponent::new(geo_service, geo_repo);

        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            domain_blacklist: Some(vec!["malicious.com".to_string()]),
            ..Default::default()
        };

        let result = component.validate_domain_blacklist("www.malicious.com", &restrictions);
        assert!(matches!(result, Ok(GeoRestrictionResult::Denied(_))));
    }

    #[test]
    fn test_team_service_component_validate_domain_blacklist_allows_safe_domain() {
        let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoLocationService::new("US"));
        let geo_repo: Arc<dyn GeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::default_restrictions());
        let component = TeamServiceComponent::new(geo_service, geo_repo);

        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            domain_blacklist: Some(vec!["malicious.com".to_string()]),
            ..Default::default()
        };

        let result = component.validate_domain_blacklist("www.google.com", &restrictions);
        assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
    }

    #[test]
    fn test_team_service_component_validate_domain_blacklist_disabled_allows_all() {
        let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoLocationService::new("US"));
        let geo_repo: Arc<dyn GeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::default_restrictions());
        let component = TeamServiceComponent::new(geo_service, geo_repo);

        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: false,
            domain_blacklist: Some(vec!["malicious.com".to_string()]),
            ..Default::default()
        };

        let result = component.validate_domain_blacklist("www.malicious.com", &restrictions);
        assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
    }

    #[tokio::test]
    async fn test_team_service_component_validate_geographic_restriction_disabled_allows() {
        let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoLocationService::new("US"));
        let geo_repo: Arc<dyn GeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::default_restrictions());
        let component = TeamServiceComponent::new(geo_service, geo_repo);

        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: false,
            ..Default::default()
        };

        let result = component
            .validate_geographic_restriction(uuid::Uuid::new_v4(), "8.8.8.8", &restrictions)
            .await;
        assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
    }

    #[tokio::test]
    async fn test_team_service_component_validate_geographic_restriction_invalid_ip_denies() {
        let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoLocationService::new("US"));
        let geo_repo: Arc<dyn GeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::default_restrictions());
        let component = TeamServiceComponent::new(geo_service, geo_repo);

        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            ..Default::default()
        };

        let result = component
            .validate_geographic_restriction(uuid::Uuid::new_v4(), "invalid-ip", &restrictions)
            .await;
        assert!(matches!(result, Ok(GeoRestrictionResult::Denied(_))));
    }

    #[tokio::test]
    async fn test_team_service_component_validate_geographic_restriction_whitelist_allows() {
        let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoLocationService::new("RU"));
        let geo_repo: Arc<dyn GeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::default_restrictions());
        let component = TeamServiceComponent::new(geo_service, geo_repo);

        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            ip_whitelist: Some(vec!["192.168.1.0/24".to_string()]),
            ..Default::default()
        };

        let result = component
            .validate_geographic_restriction(uuid::Uuid::new_v4(), "192.168.1.50", &restrictions)
            .await;
        assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
    }

    #[tokio::test]
    async fn test_team_service_component_get_team_geo_restrictions_delegates_to_repo() {
        let expected_restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string()]),
            ..Default::default()
        };
        let geo_repo: Arc<dyn GeoRestrictionRepository> = Arc::new(
            MockGeoRestrictionRepository::with_restrictions(expected_restrictions),
        );
        let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoLocationService::new("US"));
        let component = TeamServiceComponent::new(geo_service, geo_repo);

        let result = component
            .get_team_geo_restrictions(uuid::Uuid::new_v4())
            .await;
        assert!(result.enable_geo_restrictions);
        assert_eq!(result.allowed_countries, Some(vec!["US".to_string()]));
    }

    // ========== 额外错误路径测试 ==========

    #[tokio::test]
    async fn test_webhook_service_component_trigger_completion_propagates_error() {
        let mock = Arc::new(MockWebhookService::failing());
        let component = WebhookServiceComponent::new(mock.clone() as Arc<dyn WebhookService>);
        let task = make_test_task_for_service();
        let result = component.trigger_completion(&task).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_webhook_service_component_trigger_failure_propagates_error() {
        let mock = Arc::new(MockWebhookService::failing());
        let component = WebhookServiceComponent::new(mock.clone() as Arc<dyn WebhookService>);
        let task = make_test_task_for_service();
        let result = component
            .trigger_failure(&task, "timeout".to_string())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_audit_service_component_log_allow_propagates_error() {
        let mock = Arc::new(MockAuditLogRepository::failing());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let result = component
            .log_allow(
                "scrape".to_string(),
                uuid::Uuid::new_v4(),
                uuid::Uuid::new_v4(),
                crate::domain::auth::ApiKeyScope::default(),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_audit_service_component_log_deny_propagates_error() {
        let mock = Arc::new(MockAuditLogRepository::failing());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let result = component
            .log_deny(
                "delete".to_string(),
                Some(uuid::Uuid::new_v4()),
                Some(uuid::Uuid::new_v4()),
                "insufficient scope".to_string(),
                None,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_audit_service_component_get_logs_for_key_propagates_error() {
        let mock = Arc::new(MockAuditLogRepository::failing());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let result = component
            .get_logs_for_key(uuid::Uuid::new_v4(), 10, 0)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_audit_service_component_get_logs_for_team_propagates_error() {
        let mock = Arc::new(MockAuditLogRepository::failing());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let result = component
            .get_logs_for_team(uuid::Uuid::new_v4(), 10, 0)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_audit_service_component_get_denied_requests_propagates_error() {
        let mock = Arc::new(MockAuditLogRepository::failing());
        let component = AuditServiceComponent::new(mock.clone() as Arc<dyn AuditLogRepository>);
        let result = component.get_denied_requests(uuid::Uuid::new_v4(), 5).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_auth_scope_service_component_get_scope_for_key_propagates_error() {
        let repo: Arc<dyn AuthScopeRepository> = Arc::new(MockAuthScopeRepository::failing());
        let component = AuthScopeServiceComponent::new(repo);
        let result = component
            .get_scope_for_key(uuid::Uuid::new_v4(), None)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_auth_scope_service_component_delete_scope_propagates_error() {
        let repo: Arc<dyn AuthScopeRepository> = Arc::new(MockAuthScopeRepository::failing());
        let component = AuthScopeServiceComponent::new(repo);
        let result = component.delete_scope(uuid::Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    // ========== 跳过的组件 ==========
    // 以下组件的构造器需要 mock 多个 trait 或需真实外部服务，无法在无外部依赖环境下测试：
    // - SearchServiceComponent — 需 mock 多个 repository 和 SearchClient trait
    // - RateLimitingServiceComponent — 需多个 repository 依赖
    // - TemplateLoaderComponent — 无公开构造方法
}
