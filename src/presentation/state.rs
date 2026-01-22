// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Application state container for centralized dependency management.
//!
//! This module provides `AppState` as a reference pattern for dependency injection.
//! Currently, the application uses `Extension<T>` layers directly, but this structure
//! can be used as a migration path for cleaner handler signatures.
//!
//! # Example
//!
//! ```rust
//! use crate::presentation::state::AppState;
//!
//! async fn handler<T: StateExt>(state: T) {
//!     let task_repo = state.task_repo();
//!     // ... use task_repo
//! }
//! ```

use std::sync::Arc;

use crate::domain::repositories::{
    crawl_repository::CrawlRepository, credits_repository::CreditsRepository,
    geo_restriction_repository::GeoRestrictionRepository,
    scrape_result_repository::ScrapeResultRepository, task_repository::TaskRepository,
    webhook_repository::WebhookRepository,
};
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::team_service::TeamService;
use crate::infrastructure::cache::redis_client::RedisClient;
use sea_orm::DatabaseConnection;

/// Application state container holding all shared dependencies.
///
/// This structure centralizes commonly used dependencies to simplify
/// handler function signatures. Handlers can access dependencies through
/// the `StateExt` trait instead of multiple `Extension<T>` parameters.
///
/// # Current Status
///
/// This is a reference structure. The application currently uses direct
/// `Extension<T>` layers. Migrating handlers to use this pattern is optional
/// and should be done incrementally to minimize risk.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<DatabaseConnection>,
    pub task_repo: Arc<dyn TaskRepository>,
    pub crawl_repo: Arc<dyn CrawlRepository>,
    pub result_repo: Arc<dyn ScrapeResultRepository>,
    pub webhook_repo: Arc<dyn WebhookRepository>,
    pub credits_repo: Arc<dyn CreditsRepository>,
    pub geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
    pub redis_client: Arc<RedisClient>,
    pub rate_limiting_service: Arc<dyn RateLimitingService>,
    pub team_service: Arc<TeamService>,
}

impl AppState {
    /// Creates a new AppState from individual dependencies.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Arc<DatabaseConnection>,
        task_repo: Arc<dyn TaskRepository>,
        crawl_repo: Arc<dyn CrawlRepository>,
        result_repo: Arc<dyn ScrapeResultRepository>,
        webhook_repo: Arc<dyn WebhookRepository>,
        credits_repo: Arc<dyn CreditsRepository>,
        geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
        redis_client: Arc<RedisClient>,
        rate_limiting_service: Arc<dyn RateLimitingService>,
        team_service: Arc<TeamService>,
    ) -> Self {
        Self {
            db,
            task_repo,
            crawl_repo,
            result_repo,
            webhook_repo,
            credits_repo,
            geo_restriction_repo,
            redis_client,
            rate_limiting_service,
            team_service,
        }
    }
}

/// Trait for accessing application state dependencies.
///
/// This trait provides ergonomic accessors for common dependencies.
/// Implementors can be types that contain or provide access to `AppState`.
///
/// # Example
///
/// ```rust
/// use crate::presentation::state::{AppState, StateExt};
///
/// async fn my_handler<T: StateExt>(state: T) {
///     let task_repo = state.task_repo();
///     let team_service = state.team_service();
/// }
/// ```
pub trait StateExt {
    fn task_repo(&self) -> Arc<dyn TaskRepository>;
    fn crawl_repo(&self) -> Arc<dyn CrawlRepository>;
    fn result_repo(&self) -> Arc<dyn ScrapeResultRepository>;
    fn webhook_repo(&self) -> Arc<dyn WebhookRepository>;
    fn credits_repo(&self) -> Arc<dyn CreditsRepository>;
    fn geo_restriction_repo(&self) -> Arc<dyn GeoRestrictionRepository>;
    fn redis_client(&self) -> Arc<RedisClient>;
    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService>;
    fn team_service(&self) -> Arc<TeamService>;
}

impl StateExt for Arc<AppState> {
    fn task_repo(&self) -> Arc<dyn TaskRepository> {
        self.task_repo.clone()
    }

    fn crawl_repo(&self) -> Arc<dyn CrawlRepository> {
        self.crawl_repo.clone()
    }

    fn result_repo(&self) -> Arc<dyn ScrapeResultRepository> {
        self.result_repo.clone()
    }

    fn webhook_repo(&self) -> Arc<dyn WebhookRepository> {
        self.webhook_repo.clone()
    }

    fn credits_repo(&self) -> Arc<dyn CreditsRepository> {
        self.credits_repo.clone()
    }

    fn geo_restriction_repo(&self) -> Arc<dyn GeoRestrictionRepository> {
        self.geo_restriction_repo.clone()
    }

    fn redis_client(&self) -> Arc<RedisClient> {
        self.redis_client.clone()
    }

    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService> {
        self.rate_limiting_service.clone()
    }

    fn team_service(&self) -> Arc<TeamService> {
        self.team_service.clone()
    }
}

/// Crawl handler specific state for dependency injection.
///
/// This structure encapsulates all dependencies needed by crawl handlers,
/// reducing the number of parameters from 8+ to just 2-3.
#[derive(Clone)]
pub struct CrawlHandlerState {
    pub crawl_repo: Arc<dyn CrawlRepository>,
    pub task_repo: Arc<dyn TaskRepository>,
    pub webhook_repo: Arc<dyn WebhookRepository>,
    pub scrape_result_repo: Arc<dyn ScrapeResultRepository>,
    pub geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
    pub team_service: Arc<TeamService>,
    pub rate_limiting_service: Arc<dyn RateLimitingService>,
}

impl CrawlHandlerState {
    /// Create a new CrawlHandlerState from individual dependencies.
    pub fn new(
        crawl_repo: Arc<dyn CrawlRepository>,
        task_repo: Arc<dyn TaskRepository>,
        webhook_repo: Arc<dyn WebhookRepository>,
        scrape_result_repo: Arc<dyn ScrapeResultRepository>,
        geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
        team_service: Arc<TeamService>,
        rate_limiting_service: Arc<dyn RateLimitingService>,
    ) -> Self {
        Self {
            crawl_repo,
            task_repo,
            webhook_repo,
            scrape_result_repo,
            geo_restriction_repo,
            team_service,
            rate_limiting_service,
        }
    }

    /// Create CrawlHandlerState from AppState.
    pub fn from_app_state(
        app_state: &Arc<AppState>,
        rate_limiting_service: Arc<dyn RateLimitingService>,
    ) -> Self {
        Self {
            crawl_repo: app_state.crawl_repo.clone(),
            task_repo: app_state.task_repo.clone(),
            webhook_repo: app_state.webhook_repo.clone(),
            scrape_result_repo: app_state.result_repo.clone(),
            geo_restriction_repo: app_state.geo_restriction_repo.clone(),
            team_service: app_state.team_service.clone(),
            rate_limiting_service,
        }
    }

    /// Create a CrawlUseCase instance.
    pub fn create_use_case(&self) -> crate::application::use_cases::crawl_use_case::CrawlUseCase {
        crate::application::use_cases::crawl_use_case::CrawlUseCase::new(
            self.crawl_repo.clone(),
            self.task_repo.clone(),
            self.webhook_repo.clone(),
            self.scrape_result_repo.clone(),
            self.geo_restriction_repo.clone(),
            self.team_service.clone(),
        )
    }
}
