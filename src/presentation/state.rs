// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Handler state management module.
//!
//! This module provides the `HandlerState` trait for type-safe state access in handlers.
//! The main `AppState` is defined in `crate::di::axum_state` and provides centralized
//! dependency management through the Shaku DI container.
//!
//! # Architecture
//!
//! The application uses a single unified `AppState` (defined in `di::axum_state`) that
//! contains all dependencies. This module provides:
//!
//! - `HandlerState` trait: A marker trait for handlers that need state access
//! - `CrawlHandlerState`: A convenience wrapper for crawl-specific operations
//!
//! # Example
//!
//! ```rust
//! use crate::di::{AppState, AppStateExt};
//! use crate::presentation::state::HandlerState;
//!
//! async fn my_handler(state: HandlerState) {
//!     let task_repo = state.task_repo();
//!     // ... use task_repo
//! }
//! ```

use std::sync::Arc;

use crate::application::use_cases::crawl_use_case::CrawlUseCase;
use crate::di::{AppState, AppStateExt};
use crate::domain::repositories::{
    crawl_repository::CrawlRepository, geo_restriction_repository::GeoRestrictionRepository,
    scrape_result_repository::ScrapeResultRepository, task_repository::TaskRepository,
    webhook_repository::WebhookRepository,
};
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::team_service::TeamService;

/// Trait for handler state access.
///
/// This trait provides type-safe access to application state for HTTP handlers.
/// It is implemented by `Arc<AppState>` and provides ergonomic accessors for
/// commonly used dependencies.
///
/// # Type Safety
///
/// Using this trait ensures that handlers receive properly typed state and
/// can only access dependencies they are authorized to use.
pub trait HandlerState: Clone + Send + Sync + 'static {
    /// Get task repository
    fn task_repo(&self) -> Arc<dyn TaskRepository>;

    /// Get crawl repository
    fn crawl_repo(&self) -> Arc<dyn CrawlRepository>;

    /// Get scrape result repository
    fn result_repo(&self) -> Arc<dyn ScrapeResultRepository>;

    /// Get webhook repository
    fn webhook_repo(&self) -> Arc<dyn WebhookRepository>;

    /// Get geo restriction repository
    fn geo_restriction_repo(&self) -> Arc<dyn GeoRestrictionRepository>;

    /// Get team service
    fn team_service(&self) -> Arc<TeamService>;

    /// Get rate limiting service
    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService>;
}

impl HandlerState for Arc<AppState> {
    fn task_repo(&self) -> Arc<dyn TaskRepository> {
        AppStateExt::task_repo(self)
    }

    fn crawl_repo(&self) -> Arc<dyn CrawlRepository> {
        AppStateExt::crawl_repo(self)
    }

    fn result_repo(&self) -> Arc<dyn ScrapeResultRepository> {
        AppStateExt::result_repo(self)
    }

    fn webhook_repo(&self) -> Arc<dyn WebhookRepository> {
        AppStateExt::webhook_repo(self)
    }

    fn geo_restriction_repo(&self) -> Arc<dyn GeoRestrictionRepository> {
        AppStateExt::geo_restriction_repo(self)
    }

    fn team_service(&self) -> Arc<TeamService> {
        AppStateExt::team_service(self)
    }

    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService> {
        AppStateExt::rate_limiting_service(self)
    }
}

/// Crawl handler specific state wrapper.
///
/// This structure provides a convenient interface for crawl handlers,
/// encapsulating all dependencies needed for crawl operations and
/// providing factory methods for creating use cases.
///
/// # Usage
///
/// ```rust
/// use crate::di::AppState;
/// use crate::presentation::state::CrawlHandlerState;
///
/// async fn crawl_handler(state: Arc<AppState>) {
///     let crawl_state = CrawlHandlerState::from_app_state(&state);
///     let use_case = crawl_state.create_use_case();
///     // ... use the use case
/// }
/// ```
#[derive(Clone)]
pub struct CrawlHandlerState {
    /// Crawl repository
    pub crawl_repo: Arc<dyn CrawlRepository>,
    /// Task repository
    pub task_repo: Arc<dyn TaskRepository>,
    /// Webhook repository
    pub webhook_repo: Arc<dyn WebhookRepository>,
    /// Scrape result repository
    pub scrape_result_repo: Arc<dyn ScrapeResultRepository>,
    /// Geo restriction repository
    pub geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
    /// Team service
    pub team_service: Arc<TeamService>,
    /// Rate limiting service
    pub rate_limiting_service: Arc<dyn RateLimitingService>,
}

impl CrawlHandlerState {
    /// Create a new CrawlHandlerState from individual dependencies.
    #[allow(clippy::too_many_arguments)]
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
    ///
    /// This is the preferred way to create CrawlHandlerState as it
    /// ensures consistency with the main application state.
    pub fn from_app_state(app_state: &Arc<AppState>) -> Self {
        Self {
            crawl_repo: app_state.crawl_repo(),
            task_repo: app_state.task_repo(),
            webhook_repo: app_state.webhook_repo(),
            scrape_result_repo: app_state.result_repo(),
            geo_restriction_repo: app_state.geo_restriction_repo(),
            team_service: app_state.team_service(),
            rate_limiting_service: app_state.rate_limiting_service(),
        }
    }

    /// Create a CrawlUseCase instance.
    ///
    /// This factory method creates a new use case with all required
    /// dependencies injected from this state.
    pub fn create_use_case(&self) -> CrawlUseCase {
        CrawlUseCase::new(
            self.crawl_repo.clone(),
            self.task_repo.clone(),
            self.webhook_repo.clone(),
            self.scrape_result_repo.clone(),
            self.geo_restriction_repo.clone(),
            self.team_service.clone(),
        )
    }
}

impl HandlerState for CrawlHandlerState {
    fn task_repo(&self) -> Arc<dyn TaskRepository> {
        self.task_repo.clone()
    }

    fn crawl_repo(&self) -> Arc<dyn CrawlRepository> {
        self.crawl_repo.clone()
    }

    fn result_repo(&self) -> Arc<dyn ScrapeResultRepository> {
        self.scrape_result_repo.clone()
    }

    fn webhook_repo(&self) -> Arc<dyn WebhookRepository> {
        self.webhook_repo.clone()
    }

    fn geo_restriction_repo(&self) -> Arc<dyn GeoRestrictionRepository> {
        self.geo_restriction_repo.clone()
    }

    fn team_service(&self) -> Arc<TeamService> {
        self.team_service.clone()
    }

    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService> {
        self.rate_limiting_service.clone()
    }
}
