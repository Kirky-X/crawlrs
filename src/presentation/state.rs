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
    scrape_result_repository::ScrapeResultRepository, task_repository::TaskRepository,
    webhook_repository::WebhookRepository,
};
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::search_service::SearchService;
use crate::domain::services::team_service::TeamService;
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::infrastructure::database::connection::DatabaseConnection;

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
    pub redis_client: Arc<RedisClient>,
    pub rate_limiting_service: Arc<dyn RateLimitingService>,
    pub search_engine_service: Arc<dyn SearchService>,
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
        redis_client: Arc<RedisClient>,
        rate_limiting_service: Arc<dyn RateLimitingService>,
        search_engine_service: Arc<dyn SearchService>,
        team_service: Arc<TeamService>,
    ) -> Self {
        Self {
            db,
            task_repo,
            crawl_repo,
            result_repo,
            webhook_repo,
            credits_repo,
            redis_client,
            rate_limiting_service,
            search_engine_service,
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
    fn redis_client(&self) -> Arc<RedisClient>;
    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService>;
    fn search_engine_service(&self) -> Arc<dyn SearchService>;
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

    fn redis_client(&self) -> Arc<RedisClient> {
        self.redis_client.clone()
    }

    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService> {
        self.rate_limiting_service.clone()
    }

    fn search_engine_service(&self) -> Arc<dyn SearchService> {
        self.search_engine_service.clone()
    }

    fn team_service(&self) -> Arc<TeamService> {
        self.team_service.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_creation() {
        let state = AppState::new(
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
        );

        assert!(state.task_repo().is::<()>());
        assert!(state.crawl_repo().is::<()>());
        assert!(state.redis_client().is::<()>());
    }

    #[test]
    fn test_state_ext_trait() {
        let state: Arc<AppState> = Arc::new(AppState::new(
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
            Arc::new(()),
        ));

        let _task_repo = state.task_repo();
        let _crawl_repo = state.crawl_repo();
        let _result_repo = state.result_repo();
        let _webhook_repo = state.webhook_repo();
        let _credits_repo = state.credits_repo();
        let _redis_client = state.redis_client();
        let _rate_limiting_service = state.rate_limiting_service();
        let _search_engine_service = state.search_engine_service();
        let _team_service = state.team_service();
    }
}
