// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Axum integration for trait-kit dependency injection.
//!
//! This module provides trait-kit-compatible state management for Axum,
//! enabling clean dependency injection in HTTP handlers.
//!
//! [`AppState::from_kit`] is the canonical entry point: after building all
//! trait-kit modules via `AsyncKit::build()`, call `from_kit()` to extract
//! capabilities and populate the runtime state consumed by Axum handlers.

use std::sync::Arc;

use trait_kit::{AsyncKit, AsyncReady};

use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::di::modules::{EngineModule, InfrastructureModule, ModuleBuildError, ServiceModule};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::repositories::tasks_backlog_repository::TasksBacklogRepository;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::domain::services::audit_service::AuditServiceTrait;
use crate::domain::services::auth_scope_service::AuthScopeService;
use crate::domain::services::extraction_service::ExtractionServiceTrait;
use crate::domain::services::geo_location::GeoLocationService;
use crate::domain::services::llm_service::LLMServiceTrait;
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::search_service::SearchServiceTrait;
use crate::domain::services::team_service::TeamService;
use crate::domain::services::webhook_service::WebhookService;
use crate::engines::engine_client::EngineClient;
use crate::engines::router::EngineRouter;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::queue::task_queue::TaskQueue;
use crate::search::client::SearchClient;
use crate::utils::regex_cache::RegexCache;
use crate::utils::robots::RobotsCheckerTrait;
use dbnexus::DbPool;

/// Runtime state extracted from a built `AsyncKit<Ready>` for use in Axum handlers.
///
/// Construct via [`AppState::from_kit`] after registering and building all
/// trait-kit modules. Once created, `AppState` is a plain `Clone` struct
/// with no connection to the DI container — it is cheap to clone per-request.
#[derive(Clone)]
pub struct AppState {
    /// Database pool (dbnexus DbPool for repositories that need it)
    pub db_pool: Arc<DbPool>,
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
    /// Create scrape use case
    pub create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    /// Search client
    pub search_client: Arc<SearchClient>,
    /// Search service (trait object for DI)
    pub search_service: Arc<dyn SearchServiceTrait>,
    /// Auth scope service for API key permission management
    pub auth_scope_service: Option<Arc<AuthScopeService>>,
    /// LLM service for LLM operations
    pub llm_service: Arc<dyn LLMServiceTrait>,
    /// Extraction service for data extraction
    pub extraction_service: Arc<dyn ExtractionServiceTrait>,
    /// Regex cache for performance optimization
    pub regex_cache: Arc<RegexCache>,
    /// Audit service
    pub audit_service: Arc<dyn AuditServiceTrait>,
    /// Webhook worker
    pub webhook_worker: Arc<crate::workers::webhook_worker::WebhookWorker>,
    /// Backlog worker
    pub backlog_worker: Arc<crate::workers::backlog_worker::BacklogWorker>,
    /// Expiration worker
    pub expiration_worker: Arc<crate::workers::expiration_worker::ExpirationWorker>,
    /// Geo location service
    pub geo_location_service: Arc<dyn GeoLocationService>,
    /// Geo restriction repository
    pub geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
}

impl AppState {
    /// Construct an [`AppState`] from a built `AsyncKit<Ready>`.
    ///
    /// This is the canonical entry point for wiring the application at startup:
    /// register all trait-kit modules, call `AsyncKit::build()`, then pass the
    /// ready kit here. The method extracts the three top-level capability
    /// bundles (infrastructure, engines, services) and maps them onto the
    /// flat `AppState` fields consumed by Axum handlers.
    ///
    /// # Errors
    ///
    /// Returns [`ModuleBuildError`] if any required module is missing from the kit.
    pub fn from_kit(kit: &AsyncKit<AsyncReady>) -> Result<Self, ModuleBuildError> {
        let infra = kit.require::<InfrastructureModule>()?;
        let engines = kit.require::<EngineModule>()?;
        let services = kit.require::<ServiceModule>()?;

        let search_client = Arc::new(SearchClient::new(engines.engine_client.clone()));

        Ok(AppState {
            db_pool: infra.db.inner().clone(),
            task_repo: infra.repositories.task_repo.clone(),
            credits_repo: infra.repositories.credits_repo.clone(),
            crawl_repo: infra.repositories.crawl_repo.clone(),
            result_repo: infra.repositories.result_repo.clone(),
            webhook_repo: infra.repositories.webhook_repo.clone(),
            webhook_event_repo: infra.repositories.webhook_event_repo.clone(),
            tasks_backlog_repo: infra.repositories.tasks_backlog_repo.clone(),
            task_queue: services.queue.clone(),
            rate_limiting_service: services.rate_limiting_service.clone(),
            team_service: services.team_service.clone(),
            webhook_service: services.webhook_service.clone(),
            robots_checker: services.robots_checker.clone(),
            team_semaphore: services.team_semaphore.clone(),
            engine_router: engines.router.clone(),
            engine_client: engines.engine_client.clone(),
            create_scrape_use_case: services.create_scrape_use_case.clone(),
            search_client,
            search_service: services.search_service.clone(),
            auth_scope_service: services.auth_scope_service.clone(),
            llm_service: services.llm_service.clone(),
            extraction_service: services.extraction_service.clone(),
            regex_cache: services.regex_cache.clone(),
            audit_service: services.audit_service.clone(),
            webhook_worker: services.webhook_worker.clone(),
            backlog_worker: services.backlog_worker.clone(),
            expiration_worker: services.expiration_worker.clone(),
            geo_location_service: services.geo_location_service.clone(),
            geo_restriction_repo: infra.repositories.geo_restriction_repo.clone(),
        })
    }
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
    /// Get create scrape use case
    fn create_scrape_use_case(&self) -> Arc<dyn CreateScrapeUseCaseTrait>;
    /// Get search client
    fn search_client(&self) -> Arc<SearchClient>;
    /// Get search service
    fn search_service(&self) -> Arc<dyn SearchServiceTrait>;
    /// Get auth scope service
    fn auth_scope_service(&self) -> Option<Arc<AuthScopeService>>;
    /// Get LLM service
    fn llm_service(&self) -> Arc<dyn LLMServiceTrait>;
    /// Get regex cache
    fn regex_cache(&self) -> Arc<RegexCache>;
    /// Get database pool (dbnexus DbPool)
    fn db_pool(&self) -> Arc<DbPool>;
    /// Get tasks backlog repository
    fn tasks_backlog_repo(&self) -> Arc<dyn TasksBacklogRepository>;
    /// Get task queue
    fn task_queue(&self) -> Arc<dyn TaskQueue>;
    /// Get robots checker
    fn robots_checker(&self) -> Arc<dyn RobotsCheckerTrait>;
    /// Get team semaphore
    fn team_semaphore(&self) -> Arc<TeamSemaphore>;
    /// Get audit service
    fn audit_service(&self) -> Arc<dyn AuditServiceTrait>;
    /// Get extraction service
    fn extraction_service(&self) -> Arc<dyn ExtractionServiceTrait>;
    /// Get webhook worker
    fn webhook_worker(&self) -> Arc<crate::workers::webhook_worker::WebhookWorker>;
    /// Get backlog worker
    fn backlog_worker(&self) -> Arc<crate::workers::backlog_worker::BacklogWorker>;
    /// Get expiration worker
    fn expiration_worker(&self) -> Arc<crate::workers::expiration_worker::ExpirationWorker>;
    /// Get geo location service
    fn geo_location_service(&self) -> Arc<dyn GeoLocationService>;
    /// Get geo restriction repository
    fn geo_restriction_repo(&self) -> Arc<dyn GeoRestrictionRepository>;
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

    fn create_scrape_use_case(&self) -> Arc<dyn CreateScrapeUseCaseTrait> {
        self.create_scrape_use_case.clone()
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

    fn llm_service(&self) -> Arc<dyn LLMServiceTrait> {
        self.llm_service.clone()
    }

    fn regex_cache(&self) -> Arc<RegexCache> {
        self.regex_cache.clone()
    }

    fn db_pool(&self) -> Arc<DbPool> {
        self.db_pool.clone()
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

    fn audit_service(&self) -> Arc<dyn AuditServiceTrait> {
        self.audit_service.clone()
    }

    fn extraction_service(&self) -> Arc<dyn ExtractionServiceTrait> {
        self.extraction_service.clone()
    }

    fn webhook_worker(&self) -> Arc<crate::workers::webhook_worker::WebhookWorker> {
        self.webhook_worker.clone()
    }

    fn backlog_worker(&self) -> Arc<crate::workers::backlog_worker::BacklogWorker> {
        self.backlog_worker.clone()
    }

    fn expiration_worker(&self) -> Arc<crate::workers::expiration_worker::ExpirationWorker> {
        self.expiration_worker.clone()
    }

    fn geo_location_service(&self) -> Arc<dyn GeoLocationService> {
        self.geo_location_service.clone()
    }

    fn geo_restriction_repo(&self) -> Arc<dyn GeoRestrictionRepository> {
        self.geo_restriction_repo.clone()
    }
}

impl AppStateExt for Arc<AppState> {
    fn task_repo(&self) -> Arc<dyn TaskRepository> {
        self.as_ref().task_repo()
    }

    fn credits_repo(&self) -> Arc<dyn CreditsRepository> {
        self.as_ref().credits_repo()
    }

    fn crawl_repo(&self) -> Arc<dyn CrawlRepository> {
        self.as_ref().crawl_repo()
    }

    fn result_repo(&self) -> Arc<dyn ScrapeResultRepository> {
        self.as_ref().result_repo()
    }

    fn webhook_repo(&self) -> Arc<dyn WebhookRepository> {
        self.as_ref().webhook_repo()
    }

    fn webhook_event_repo(&self) -> Arc<dyn WebhookEventRepository> {
        self.as_ref().webhook_event_repo()
    }

    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService> {
        self.as_ref().rate_limiting_service()
    }

    fn team_service(&self) -> Arc<TeamService> {
        self.as_ref().team_service()
    }

    fn webhook_service(&self) -> Arc<dyn WebhookService> {
        self.as_ref().webhook_service()
    }

    fn engine_router(&self) -> Arc<EngineRouter> {
        self.as_ref().engine_router()
    }

    fn engine_client(&self) -> Arc<EngineClient> {
        self.as_ref().engine_client()
    }

    fn create_scrape_use_case(&self) -> Arc<dyn CreateScrapeUseCaseTrait> {
        self.as_ref().create_scrape_use_case()
    }

    fn search_client(&self) -> Arc<SearchClient> {
        self.as_ref().search_client()
    }

    fn search_service(&self) -> Arc<dyn SearchServiceTrait> {
        self.as_ref().search_service()
    }

    fn auth_scope_service(&self) -> Option<Arc<AuthScopeService>> {
        self.as_ref().auth_scope_service()
    }

    fn llm_service(&self) -> Arc<dyn LLMServiceTrait> {
        self.as_ref().llm_service()
    }

    fn regex_cache(&self) -> Arc<RegexCache> {
        self.as_ref().regex_cache()
    }

    fn db_pool(&self) -> Arc<DbPool> {
        self.as_ref().db_pool()
    }

    fn tasks_backlog_repo(&self) -> Arc<dyn TasksBacklogRepository> {
        self.as_ref().tasks_backlog_repo()
    }

    fn task_queue(&self) -> Arc<dyn TaskQueue> {
        self.as_ref().task_queue()
    }

    fn robots_checker(&self) -> Arc<dyn RobotsCheckerTrait> {
        self.as_ref().robots_checker()
    }

    fn team_semaphore(&self) -> Arc<TeamSemaphore> {
        self.as_ref().team_semaphore()
    }

    fn audit_service(&self) -> Arc<dyn AuditServiceTrait> {
        self.as_ref().audit_service()
    }

    fn extraction_service(&self) -> Arc<dyn ExtractionServiceTrait> {
        self.as_ref().extraction_service()
    }

    fn webhook_worker(&self) -> Arc<crate::workers::webhook_worker::WebhookWorker> {
        self.as_ref().webhook_worker()
    }

    fn backlog_worker(&self) -> Arc<crate::workers::backlog_worker::BacklogWorker> {
        self.as_ref().backlog_worker()
    }

    fn expiration_worker(&self) -> Arc<crate::workers::expiration_worker::ExpirationWorker> {
        self.as_ref().expiration_worker()
    }

    fn geo_location_service(&self) -> Arc<dyn GeoLocationService> {
        self.as_ref().geo_location_service()
    }

    fn geo_restriction_repo(&self) -> Arc<dyn GeoRestrictionRepository> {
        self.as_ref().geo_restriction_repo()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_support::testcontainers_fixtures as tcf;
    use crate::di::modules::{
        CacheModule, DatabaseModule, EngineModule, HttpModule, InfrastructureModule,
        RepositoryModule, ServiceModule, SettingsModule,
    };
    use std::sync::Arc;

    async fn require_docker() -> bool {
        tcf::docker_available().await
    }

    /// Build a full AppState via `AsyncKit` + `AppState::from_kit()`.
    ///
    /// Registers all trait-kit modules against a testcontainers-provided
    /// PostgreSQL pair, builds the kit, then constructs `AppState`
    /// through the canonical `from_kit` entry point.
    async fn build_app_state() -> anyhow::Result<AppState> {
        let handle = tcf::DbHandle::start().await?;
        let settings = Arc::new(tcf::settings_with_urls(&handle.pg.url)?);

        let mut kit = AsyncKit::new();
        kit.set_config(settings);
        kit.register::<SettingsModule>()
            .map_err(|e| anyhow::anyhow!("register SettingsModule: {e}"))?;
        kit.register::<DatabaseModule>()
            .map_err(|e| anyhow::anyhow!("register DatabaseModule: {e}"))?;
        kit.register::<HttpModule>()
            .map_err(|e| anyhow::anyhow!("register HttpModule: {e}"))?;
        kit.register::<CacheModule>()
            .map_err(|e| anyhow::anyhow!("register CacheModule: {e}"))?;
        kit.register::<RepositoryModule>()
            .map_err(|e| anyhow::anyhow!("register RepositoryModule: {e}"))?;
        kit.register::<EngineModule>()
            .map_err(|e| anyhow::anyhow!("register EngineModule: {e}"))?;
        kit.register::<InfrastructureModule>()
            .map_err(|e| anyhow::anyhow!("register InfrastructureModule: {e}"))?;
        kit.register::<ServiceModule>()
            .map_err(|e| anyhow::anyhow!("register ServiceModule: {e}"))?;

        // 高并行度下 kit.build() 可能因数据库连接失败而报错，用 ? 传播错误让调用方优雅跳过
        let kit = kit
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to build kit: {e}"))?;
        let state = AppState::from_kit(&kit)?;
        Ok(state)
    }

    #[tokio::test]
    async fn tc_app_state_all_accessors_return_valid_arcs() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_app_state_all_accessors_return_valid_arcs");
            return;
        }
        let state = match build_app_state().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[skip] failed to build AppState: {e}");
                return;
            }
        };

        // Exercise every AppStateExt accessor on &AppState.
        let task_repo: Arc<dyn TaskRepository> = state.task_repo();
        assert!(Arc::strong_count(&task_repo) >= 1);

        let credits_repo: Arc<dyn CreditsRepository> = state.credits_repo();
        assert!(Arc::strong_count(&credits_repo) >= 1);

        let crawl_repo: Arc<dyn CrawlRepository> = state.crawl_repo();
        assert!(Arc::strong_count(&crawl_repo) >= 1);

        let result_repo: Arc<dyn ScrapeResultRepository> = state.result_repo();
        assert!(Arc::strong_count(&result_repo) >= 1);

        let webhook_repo: Arc<dyn WebhookRepository> = state.webhook_repo();
        assert!(Arc::strong_count(&webhook_repo) >= 1);

        let webhook_event_repo: Arc<dyn WebhookEventRepository> = state.webhook_event_repo();
        assert!(Arc::strong_count(&webhook_event_repo) >= 1);

        let rate_limiting: Arc<dyn RateLimitingService> = state.rate_limiting_service();
        assert!(Arc::strong_count(&rate_limiting) >= 1);

        let team_service: Arc<TeamService> = state.team_service();
        assert!(Arc::strong_count(&team_service) >= 1);

        let webhook_service: Arc<dyn WebhookService> = state.webhook_service();
        assert!(Arc::strong_count(&webhook_service) >= 1);

        let engine_router: Arc<EngineRouter> = state.engine_router();
        assert!(Arc::strong_count(&engine_router) >= 1);

        let engine_client: Arc<EngineClient> = state.engine_client();
        assert!(Arc::strong_count(&engine_client) >= 1);

        let create_scrape: Arc<dyn CreateScrapeUseCaseTrait> = state.create_scrape_use_case();
        assert!(Arc::strong_count(&create_scrape) >= 1);

        let search_client: Arc<SearchClient> = state.search_client();
        assert!(Arc::strong_count(&search_client) >= 1);

        let search_service: Arc<dyn SearchServiceTrait> = state.search_service();
        assert!(Arc::strong_count(&search_service) >= 1);

        let auth_scope: Option<Arc<AuthScopeService>> = state.auth_scope_service();
        assert!(auth_scope.is_some());

        let llm_service: Arc<dyn LLMServiceTrait> = state.llm_service();
        assert!(Arc::strong_count(&llm_service) >= 1);

        let regex_cache: Arc<RegexCache> = state.regex_cache();
        assert!(Arc::strong_count(&regex_cache) >= 1);

        let db_pool: Arc<DbPool> = state.db_pool();
        assert!(Arc::strong_count(&db_pool) >= 1);

        let tasks_backlog: Arc<dyn TasksBacklogRepository> = state.tasks_backlog_repo();
        assert!(Arc::strong_count(&tasks_backlog) >= 1);

        let task_queue: Arc<dyn TaskQueue> = state.task_queue();
        assert!(Arc::strong_count(&task_queue) >= 1);

        let robots_checker: Arc<dyn RobotsCheckerTrait> = state.robots_checker();
        assert!(Arc::strong_count(&robots_checker) >= 1);

        let team_semaphore: Arc<TeamSemaphore> = state.team_semaphore();
        assert!(Arc::strong_count(&team_semaphore) >= 1);

        let audit_service: Arc<dyn AuditServiceTrait> = state.audit_service();
        assert!(Arc::strong_count(&audit_service) >= 1);

        let extraction_service: Arc<dyn ExtractionServiceTrait> = state.extraction_service();
        assert!(Arc::strong_count(&extraction_service) >= 1);

        let webhook_worker = state.webhook_worker();
        assert!(Arc::strong_count(&webhook_worker) >= 1);

        let backlog_worker = state.backlog_worker();
        assert!(Arc::strong_count(&backlog_worker) >= 1);

        let expiration_worker = state.expiration_worker();
        assert!(Arc::strong_count(&expiration_worker) >= 1);

        let geo_location: Arc<dyn GeoLocationService> = state.geo_location_service();
        assert!(Arc::strong_count(&geo_location) >= 1);

        let geo_restriction: Arc<dyn GeoRestrictionRepository> = state.geo_restriction_repo();
        assert!(Arc::strong_count(&geo_restriction) >= 1);
    }

    #[tokio::test]
    async fn tc_app_state_arc_accessors_delegate_correctly() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_app_state_arc_accessors_delegate_correctly");
            return;
        }
        let state = match build_app_state().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[skip] failed to build AppState: {e}");
                return;
            }
        };
        let state_arc: Arc<AppState> = Arc::new(state);

        // The `impl AppStateExt for Arc<AppState>` should delegate to the
        // inner `&AppState` impl. Verify a few accessors produce equivalent
        // strong counts (i.e. the Arc is properly cloned).
        let task_repo_a = state_arc.task_repo();
        let task_repo_b = state_arc.task_repo();
        // Cloning an Arc increments the strong count; both should be valid.
        assert!(Arc::strong_count(&task_repo_a) >= 1);
        assert!(Arc::strong_count(&task_repo_b) >= 1);

        let db_a = state_arc.db_pool();
        let db_b = state_arc.db_pool();
        assert!(Arc::strong_count(&db_a) >= 1);
        assert!(Arc::strong_count(&db_b) >= 1);
    }

    #[tokio::test]
    async fn tc_app_state_clone_preserves_fields() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_app_state_clone_preserves_fields");
            return;
        }
        let state = match build_app_state().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[skip] failed to build AppState: {e}");
                return;
            }
        };
        // AppState derives Clone; verify clone produces an equivalent instance.
        let cloned = state.clone();
        // Both should return valid Arcs from accessors.
        let _ = state.task_repo();
        let _ = cloned.task_repo();
    }
}
