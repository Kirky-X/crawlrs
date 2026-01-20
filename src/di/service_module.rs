// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Service module for Shaku dependency injection.
//!
//! This module provides Shaku components for service layer dependencies
//! including TeamService, WebhookService, and other application services.

use std::sync::Arc;

use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::repositories::tasks_backlog_repository::TasksBacklogRepository;
use crate::domain::services::audit_service::AuditServiceTrait;
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::team_service::TeamService;
use crate::domain::services::webhook_service::WebhookService;
use crate::engines::router::EngineRouter;
use crate::engines::router::EngineRouterTrait;
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::infrastructure::geolocation::GeoLocationServiceTrait;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::utils::robots::RobotsCheckerTrait;
use crate::di::search_module::HttpClientTrait;
use crate::di::infrastructure_module::RedisClientTrait;

/// Trait for RateLimitingService component
pub trait RateLimitingServiceTrait: Send + Sync {
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

/// Trait for TeamService component
pub trait TeamServiceTrait: Send + Sync {
    fn get_service(&self) -> &TeamService;
}

/// TeamService component
#[allow(dead_code)]
pub struct TeamServiceComponent {
    /// Geolocation service
    geolocation_service: Arc<dyn GeoLocationServiceTrait>,
    /// Geo restriction repo
    geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
}

impl TeamServiceComponent {
    /// Create a new TeamServiceComponent with explicit dependencies
    pub fn new(
        geolocation_service: Arc<dyn GeoLocationServiceTrait>,
        geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
    ) -> Self {
        Self {
            geolocation_service,
            geo_restriction_repo,
        }
    }
}

impl TeamServiceTrait for TeamServiceComponent {
    fn get_service(&self) -> &TeamService {
        // Return a reference to self as TeamService
        // Note: This is a workaround for the struct-based service
        unimplemented!("TeamService is a struct, not a trait")
    }
}

/// Trait for WebhookService component
pub trait WebhookServiceTrait: Send + Sync {
    fn get_service(&self) -> &dyn WebhookService;
}

/// WebhookService component
#[allow(dead_code)]
pub struct WebhookServiceComponent {
    /// Webhook secret
    secret: String,
}

impl WebhookServiceComponent {
    /// Create a new WebhookServiceComponent with explicit secret
    pub fn new(secret: String) -> Self {
        Self { secret }
    }

    /// Create from environment variable (convenience method)
    pub fn from_env() -> Self {
        let secret = std::env::var("WEBHOOK_SECRET")
            .unwrap_or_else(|_| "default-webhook-secret".to_string());
        Self { secret }
    }
}

#[async_trait::async_trait]
impl WebhookService for WebhookServiceComponent {
    async fn send_webhook(
        &self,
        _event: &crate::domain::models::webhook::WebhookEvent,
    ) -> Result<(), anyhow::Error> {
        Ok(())
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
#[derive(Default)]
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
}

impl AuditServiceTraitDI for AuditServiceComponent {
    fn get_service(&self) -> &dyn AuditServiceTrait {
        self
    }
}

// Service module components - for Shaku DI
