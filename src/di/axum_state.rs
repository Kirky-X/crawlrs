// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Axum integration for Shaku dependency injection.
//!
//! This module provides Shaku-compatible state management for Axum,
//! enabling clean dependency injection in HTTP handlers.

use std::sync::Arc;

use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::storage_repository::StorageRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::repositories::tasks_backlog_repository::TasksBacklogRepository;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::domain::services::auth_scope_service::AuthScopeService;
use crate::domain::services::audit_service::AuditService;
use crate::domain::services::llm_service::LLMService;
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::search_service::SearchServiceTrait;
use crate::domain::services::team_service::TeamService;
use crate::domain::services::webhook_service::WebhookService;
use crate::engines::engine_client::EngineClient;
use crate::engines::router::EngineRouter;
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::queue::task_queue::TaskQueue;
use crate::search::client::SearchClient;
use crate::utils::regex_cache::RegexCache;
use crate::utils::robots::RobotsCheckerTrait;

/// State extracted from Shaku module for use in Axum handlers
#[derive(Clone)]
pub struct AppState {
    /// Database connection
    pub db: Arc<sea_orm::DbConn>,
    /// Redis client
    pub redis_client: Arc<RedisClient>,
    /// Task repository
    pub task_repo: Arc<dyn TaskRepository>,
    /// Credits repository
    pub credits_repo: Arc<dyn CreditsRepository>,
    /// Crawl repository
    pub crawl_repo: Arc<dyn CrawlRepository>,
    /// Scrape result repository
    pub result_repo: Arc<dyn ScrapeResultRepository>,
    /// Webhook repository
    pub webhook_repo: Arc<dyn WebhookRepository>,
    /// Webhook event repository
    pub webhook_event_repo: Arc<dyn WebhookEventRepository>,
    /// Storage repository
    pub storage_repo: Arc<dyn StorageRepository>,
    /// Tasks backlog repository
    pub tasks_backlog_repo: Arc<dyn TasksBacklogRepository>,
    /// Task queue
    pub task_queue: Arc<dyn TaskQueue>,
    /// Rate limiting service
    pub rate_limiting_service: Arc<dyn RateLimitingService>,
    /// Team service
    pub team_service: Arc<TeamService>,
    /// Webhook service
    pub webhook_service: Arc<dyn WebhookService>,
    /// Robots checker
    pub robots_checker: Arc<dyn RobotsCheckerTrait>,
    /// Team semaphore
    pub team_semaphore: Arc<TeamSemaphore>,
    /// Engine router
    pub engine_router: Arc<EngineRouter>,
    /// Engine client
    pub engine_client: Arc<EngineClient>,
    /// Search client
    pub search_client: Arc<SearchClient>,
    /// Search service (trait object for DI)
    pub search_service: Arc<dyn SearchServiceTrait>,
    /// Auth scope service for API key permission management
    pub auth_scope_service: Option<Arc<AuthScopeService>>,
    /// LLM service for LLM operations
    pub llm_service: Arc<LLMService>,
    /// Regex cache for performance optimization
    pub regex_cache: Arc<RegexCache>,
    /// Audit service
    pub audit_service: Arc<AuditService>,
}

/// Trait for extracting dependencies from AppState
///
/// This trait provides convenient accessors for commonly used dependencies
/// in Axum handlers.
pub trait AppStateExt {
    /// Get task repository
    fn task_repo(&self) -> Arc<dyn TaskRepository>;
    /// Get credits repository
    fn credits_repo(&self) -> Arc<dyn CreditsRepository>;
    /// Get crawl repository
    fn crawl_repo(&self) -> Arc<dyn CrawlRepository>;
    /// Get result repository
    fn result_repo(&self) -> Arc<dyn ScrapeResultRepository>;
    /// Get webhook repository
    fn webhook_repo(&self) -> Arc<dyn WebhookRepository>;
    /// Get webhook event repository
    fn webhook_event_repo(&self) -> Arc<dyn WebhookEventRepository>;
    /// Get rate limiting service
    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService>;
    /// Get team service
    fn team_service(&self) -> Arc<TeamService>;
    /// Get webhook service
    fn webhook_service(&self) -> Arc<dyn WebhookService>;
    /// Get engine router
    fn engine_router(&self) -> Arc<EngineRouter>;
    /// Get engine client
    fn engine_client(&self) -> Arc<EngineClient>;
    /// Get search client
    fn search_client(&self) -> Arc<SearchClient>;
    /// Get search service
    fn search_service(&self) -> Arc<dyn SearchServiceTrait>;
    /// Get auth scope service
    fn auth_scope_service(&self) -> Option<Arc<AuthScopeService>>;
    /// Get LLM service
    fn llm_service(&self) -> Arc<LLMService>;
    /// Get regex cache
    fn regex_cache(&self) -> Arc<RegexCache>;
    /// Get Redis client
    fn redis_client(&self) -> Arc<RedisClient>;
    /// Get database connection
    fn db(&self) -> Arc<sea_orm::DbConn>;
    /// Get storage repository
    fn storage_repo(&self) -> Arc<dyn StorageRepository>;
    /// Get tasks backlog repository
    fn tasks_backlog_repo(&self) -> Arc<dyn TasksBacklogRepository>;
    /// Get task queue
    fn task_queue(&self) -> Arc<dyn TaskQueue>;
    /// Get robots checker
    fn robots_checker(&self) -> Arc<dyn RobotsCheckerTrait>;
    /// Get team semaphore
    fn team_semaphore(&self) -> Arc<TeamSemaphore>;
    /// Get audit service
    fn audit_service(&self) -> Arc<AuditService>;
}

impl AppStateExt for AppState {
    fn task_repo(&self) -> Arc<dyn TaskRepository> {
        self.task_repo.clone()
    }

    fn credits_repo(&self) -> Arc<dyn CreditsRepository> {
        self.credits_repo.clone()
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

    fn webhook_event_repo(&self) -> Arc<dyn WebhookEventRepository> {
        self.webhook_event_repo.clone()
    }

    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService> {
        self.rate_limiting_service.clone()
    }

    fn team_service(&self) -> Arc<TeamService> {
        self.team_service.clone()
    }

    fn webhook_service(&self) -> Arc<dyn WebhookService> {
        self.webhook_service.clone()
    }

    fn engine_router(&self) -> Arc<EngineRouter> {
        self.engine_router.clone()
    }

    fn engine_client(&self) -> Arc<EngineClient> {
        self.engine_client.clone()
    }

    fn search_client(&self) -> Arc<SearchClient> {
        self.search_client.clone()
    }

    fn search_service(&self) -> Arc<dyn SearchServiceTrait> {
        self.search_service.clone()
    }

    fn auth_scope_service(&self) -> Option<Arc<AuthScopeService>> {
        self.auth_scope_service.clone()
    }

    fn llm_service(&self) -> Arc<LLMService> {
        self.llm_service.clone()
    }

    fn regex_cache(&self) -> Arc<RegexCache> {
        self.regex_cache.clone()
    }

    fn redis_client(&self) -> Arc<RedisClient> {
        self.redis_client.clone()
    }

    fn db(&self) -> Arc<sea_orm::DbConn> {
        self.db.clone()
    }

    fn storage_repo(&self) -> Arc<dyn StorageRepository> {
        self.storage_repo.clone()
    }

    fn tasks_backlog_repo(&self) -> Arc<dyn TasksBacklogRepository> {
        self.tasks_backlog_repo.clone()
    }

    fn task_queue(&self) -> Arc<dyn TaskQueue> {
        self.task_queue.clone()
    }

    fn robots_checker(&self) -> Arc<dyn RobotsCheckerTrait> {
        self.robots_checker.clone()
    }

    fn team_semaphore(&self) -> Arc<TeamSemaphore> {
        self.team_semaphore.clone()
    }

    fn audit_service(&self) -> Arc<AuditService> {
        self.audit_service.clone()
    }
}
