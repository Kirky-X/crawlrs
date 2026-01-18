// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Service module for Shaku dependency injection.
//!
//! This module provides Shaku components for service layer dependencies
//! including TeamService, WebhookService, and other application services.

use shaku::Component;
use std::sync::Arc;

use crate::application::use_cases::create_scrape::CreateScrapeUseCase;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::team_service::{TeamGeoRestrictions, TeamService};
use crate::domain::services::webhook_service::WebhookService;
use crate::infrastructure::geolocation::GeoLocationServiceTrait;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::utils::robots::RobotsChecker;
use crate::utils::robots::RobotsCheckerTrait;

/// Component parameters for ServiceModule
#[derive(shaku::ComponentParameters)]
pub struct ServiceModuleParameters {
    /// Application settings
    pub settings: Arc<crate::config::settings::Settings>,
}

/// RateLimitingService component
#[derive(Component)]
#[shaku(interface = RateLimitingService)]
pub struct RateLimitingServiceComponent {
    /// Redis client for distributed rate limiting
    #[shaku(inject)]
    redis_client: Arc<dyn crate::infrastructure::cache::redis_client::RedisClient>,

    /// Credits repository for credit-based rate limiting
    #[shaku(inject)]
    credits_repo: Arc<dyn crate::domain::repositories::credits_repository::CreditsRepository>,

    /// Task repository for backlog processing
    #[shaku(inject)]
    task_repo: Arc<dyn crate::domain::repositories::task_repository::TaskRepository>,

    /// Settings for rate limiting
    settings: Arc<crate::config::settings::Settings>,
}

impl RateLimitingService for RateLimitingServiceComponent {
    // Implementation delegated to infrastructure service impl
}

/// TeamService component
#[derive(Component)]
#[shaku(interface = TeamService)]
pub struct TeamServiceComponent {
    /// Geolocation service for IP-based restrictions
    #[shaku(inject)]
    geolocation_service: Arc<dyn GeoLocationServiceTrait>,

    /// Geo restriction repository
    #[shaku(inject)]
    geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
}

impl TeamService for TeamServiceComponent {
    // Implementation delegated to team service impl
}

/// WebhookService component
#[derive(Component)]
#[shaku(interface = WebhookService)]
pub struct WebhookServiceComponent {
    /// Webhook secret for signature verification
    webhook_secret: String,
}

impl WebhookService for WebhookServiceComponent {
    // Implementation delegated to webhook service impl
}

/// CreateScrapeUseCase component
#[derive(Component)]
#[shaku(interface = CreateScrapeUseCase)]
pub struct CreateScrapeUseCaseComponent {
    /// Engine router for selecting appropriate engines
    #[shaku(inject)]
    engine_router: Arc<dyn crate::engines::router::EngineRouter>,
}

impl CreateScrapeUseCase for CreateScrapeUseCaseComponent {
    // Implementation delegated to use case impl
}

/// RobotsChecker component
#[derive(Component)]
#[shaku(interface = RobotsCheckerTrait)]
pub struct RobotsCheckerComponent {
    /// Redis client for caching
    #[shaku(inject)]
    redis_client: Option<Arc<dyn crate::infrastructure::cache::redis_client::RedisClient>>,

    /// Settings for retry policy
    settings: Arc<crate::config::settings::Settings>,
}

impl RobotsCheckerTrait for RobotsCheckerComponent {
    // Implementation delegated to RobotsChecker
}

impl RobotsCheckerComponent {
    pub fn create(
        redis_client: Option<Arc<dyn crate::infrastructure::cache::redis_client::RedisClient>>,
        settings: Arc<crate::config::settings::Settings>,
    ) -> Self {
        Self {
            redis_client,
            settings,
        }
    }
}

/// TeamSemaphore component
#[derive(Component)]
#[shaku(interface = ())]
pub struct TeamSemaphoreComponent {
    /// The actual team semaphore
    semaphore: Arc<TeamSemaphore>,

    /// Default permits per team
    default_permits: usize,
}

impl TeamSemaphoreComponent {
    pub fn create(settings: &crate::config::settings::Settings) -> Self {
        Self {
            semaphore: Arc::new(TeamSemaphore::new(settings.concurrency.default_team_limit)),
            default_permits: settings.concurrency.default_team_limit,
        }
    }
}

/// Service module for Shaku DI
///
/// This module provides all service components including:
/// - RateLimitingService (distributed rate limiting)
/// - TeamService (team management and geo restrictions)
/// - WebhookService (webhook notifications)
/// - CreateScrapeUseCase (scrape operation use case)
shaku::module! {
    pub ServiceModule {
        components = [
            RateLimitingServiceComponent,
            TeamServiceComponent,
            WebhookServiceComponent,
            CreateScrapeUseCaseComponent,
            RobotsCheckerComponent,
            TeamSemaphoreComponent,
        ],
        providers = []
    }
}
