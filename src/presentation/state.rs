// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Handler state management module.
//!
//! This module provides the `HandlerState` trait for type-safe state access in handlers.
//! The main `CrawlRsState` is defined in `crate::di::axum_state` and provides centralized
//! dependency management through the trait-kit AsyncKit container.
//!
//! # Architecture
//!
//! The application uses a single unified `CrawlRsState` (defined in `di::axum_state`) that
//! contains all dependencies. This module provides:
//!
//! - `HandlerState` trait: A marker trait for handlers that need state access
//! - `CrawlHandlerState`: A convenience wrapper for crawl-specific operations
//!
//! # Example
//!
//! ```ignore
//! use crate::di::{CrawlRsState, CrawlRsStateExt};
//! use crate::presentation::state::HandlerState;
//!
//! async fn my_handler(state: HandlerState) {
//!     let task_repo = state.task_repo();
//!     // ... use task_repo
//! }
//! ```

use std::sync::Arc;

use crate::application::use_cases::crawl_use_case::CrawlUseCase;
use crate::di::{CrawlRsState, CrawlRsStateExt};
use crate::domain::repositories::{
    crawl_repository::CrawlRepository, scrape_result_repository::ScrapeResultRepository,
    task_repository::TaskRepository,
};
// R-teams-004 / T014：teams feature 关闭时不导入 teams 相关类型
#[cfg(feature = "teams")]
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
#[cfg(feature = "teams")]
use crate::domain::services::team_service::TeamService;
// R-wh-003 / T027：webhook feature 关闭时不导入 WebhookRepository
#[cfg(feature = "webhook")]
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::domain::services::rate_limiting_service::RateLimitingService;

/// Trait for handler state access.
///
/// This trait provides type-safe access to application state for HTTP handlers.
/// It is implemented by `Arc<CrawlRsState>` and provides ergonomic accessors for
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
    ///
    /// R-wh-003 / T027：webhook feature 关闭时不编译此 accessor。
    #[cfg(feature = "webhook")]
    fn webhook_repo(&self) -> Arc<dyn WebhookRepository>;

    /// Get geo restriction repository
    ///
    /// R-teams-004 / T014：teams feature 关闭时不编译此 accessor。
    #[cfg(feature = "teams")]
    fn geo_restriction_repo(&self) -> Arc<dyn GeoRestrictionRepository>;

    /// Get team service
    ///
    /// R-teams-004 / T014：teams feature 关闭时不编译此 accessor。
    #[cfg(feature = "teams")]
    fn team_service(&self) -> Arc<TeamService>;

    /// Get rate limiting service
    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService>;
}

impl HandlerState for Arc<CrawlRsState> {
    fn task_repo(&self) -> Arc<dyn TaskRepository> {
        CrawlRsStateExt::task_repo(self)
    }

    fn crawl_repo(&self) -> Arc<dyn CrawlRepository> {
        CrawlRsStateExt::crawl_repo(self)
    }

    fn result_repo(&self) -> Arc<dyn ScrapeResultRepository> {
        CrawlRsStateExt::result_repo(self)
    }

    /// R-wh-003 / T027：webhook feature 关闭时不编译此 accessor 实现。
    #[cfg(feature = "webhook")]
    fn webhook_repo(&self) -> Arc<dyn WebhookRepository> {
        CrawlRsStateExt::webhook_repo(self)
    }

    /// R-teams-004 / T014：teams feature 关闭时不编译此 accessor 实现。
    #[cfg(feature = "teams")]
    fn geo_restriction_repo(&self) -> Arc<dyn GeoRestrictionRepository> {
        CrawlRsStateExt::geo_restriction_repo(self)
    }

    /// R-teams-004 / T014：teams feature 关闭时不编译此 accessor 实现。
    #[cfg(feature = "teams")]
    fn team_service(&self) -> Arc<TeamService> {
        CrawlRsStateExt::team_service(self)
    }

    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService> {
        CrawlRsStateExt::rate_limiting_service(self)
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
/// ```ignore
/// use crate::di::CrawlRsState;
/// use crate::presentation::state::CrawlHandlerState;
///
/// async fn crawl_handler(state: Arc<CrawlRsState>) {
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
    ///
    /// R-wh-003 / T027：webhook feature 关闭时不编译此字段。
    /// webhook-off 模式下，CrawlHandlerState 不持有 WebhookRepository。
    #[cfg(feature = "webhook")]
    pub webhook_repo: Arc<dyn WebhookRepository>,
    /// Scrape result repository
    pub scrape_result_repo: Arc<dyn ScrapeResultRepository>,
    /// Geo restriction repository
    ///
    /// R-teams-004 / T014：teams feature 关闭时不编译此字段。
    /// teams-off 模式下，CrawlHandlerState 不持有 GeoRestrictionRepository。
    #[cfg(feature = "teams")]
    pub geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
    /// Team service
    ///
    /// R-teams-004 / T014：teams feature 关闭时不编译此字段。
    /// teams-off 模式下，CrawlHandlerState 不持有 TeamService。
    #[cfg(feature = "teams")]
    pub team_service: Arc<TeamService>,
    /// Rate limiting service
    pub rate_limiting_service: Arc<dyn RateLimitingService>,
}

impl CrawlHandlerState {
    /// Create a new CrawlHandlerState from individual dependencies.
    ///
    /// # Parameters
    ///
    /// * `webhook_repo` - Webhook repository（仅 webhook-on 时传入）
    /// * `geo_restriction_repo` - Geo restriction repository（仅 teams-on 时传入）
    /// * `team_service` - Team service（仅 teams-on 时传入）
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        crawl_repo: Arc<dyn CrawlRepository>,
        task_repo: Arc<dyn TaskRepository>,
        scrape_result_repo: Arc<dyn ScrapeResultRepository>,
        rate_limiting_service: Arc<dyn RateLimitingService>,
        #[cfg(feature = "webhook")] webhook_repo: Arc<dyn WebhookRepository>,
        #[cfg(feature = "teams")] geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
        #[cfg(feature = "teams")] team_service: Arc<TeamService>,
    ) -> Self {
        Self {
            crawl_repo,
            task_repo,
            scrape_result_repo,
            rate_limiting_service,
            #[cfg(feature = "webhook")]
            webhook_repo,
            #[cfg(feature = "teams")]
            geo_restriction_repo,
            #[cfg(feature = "teams")]
            team_service,
        }
    }

    /// Create CrawlHandlerState from CrawlRsState.
    ///
    /// This is the preferred way to create CrawlHandlerState as it
    /// ensures consistency with the main application state.
    pub fn from_app_state(app_state: &Arc<CrawlRsState>) -> Self {
        Self {
            crawl_repo: app_state.crawl_repo.clone(),
            task_repo: app_state.task_repo.clone(),
            scrape_result_repo: app_state.result_repo.clone(),
            rate_limiting_service: app_state.rate_limiting_service.clone(),
            // R-wh-003 / T027：webhook-on 时从 app_state 取 webhook_repo
            #[cfg(feature = "webhook")]
            webhook_repo: app_state.webhook_repo.clone(),
            // R-teams-004 / T014：teams-on 时从 app_state 取 geo_restriction_repo / team_service
            #[cfg(feature = "teams")]
            geo_restriction_repo: app_state.geo_restriction_repo.clone(),
            #[cfg(feature = "teams")]
            team_service: app_state.team_service.clone(),
        }
    }

    /// Create a CrawlUseCase instance.
    ///
    /// This factory method creates a new use case with all required
    /// dependencies injected from this state.
    ///
    /// R-teams-004 / R-wh-003：feature-off 时跳过对应字段（CrawlUseCase::new 已门控参数）
    pub fn create_use_case(&self) -> CrawlUseCase {
        CrawlUseCase::new(
            self.crawl_repo.clone(),
            self.task_repo.clone(),
            self.scrape_result_repo.clone(),
            #[cfg(feature = "webhook")]
            self.webhook_repo.clone(),
            #[cfg(feature = "teams")]
            self.geo_restriction_repo.clone(),
            #[cfg(feature = "teams")]
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

    /// R-wh-003 / T027：webhook feature 关闭时不编译此 accessor 实现。
    #[cfg(feature = "webhook")]
    fn webhook_repo(&self) -> Arc<dyn WebhookRepository> {
        self.webhook_repo.clone()
    }

    /// R-teams-004 / T014：teams feature 关闭时不编译此 accessor 实现。
    #[cfg(feature = "teams")]
    fn geo_restriction_repo(&self) -> Arc<dyn GeoRestrictionRepository> {
        self.geo_restriction_repo.clone()
    }

    /// R-teams-004 / T014：teams feature 关闭时不编译此 accessor 实现。
    #[cfg(feature = "teams")]
    fn team_service(&self) -> Arc<TeamService> {
        self.team_service.clone()
    }

    fn rate_limiting_service(&self) -> Arc<dyn RateLimitingService> {
        self.rate_limiting_service.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::credits_model::CreditsTransactionType;
    use crate::domain::models::ScrapeResult;
    use crate::domain::models::{Crawl, Task};
    // R-wh-003 / T027：webhook feature 关闭时不导入 Webhook 模型
    #[cfg(feature = "webhook")]
    use crate::domain::models::Webhook;
    use crate::domain::repositories::task_repository::{RepositoryError, TaskQueryParams};
    // R-teams-004 / T014：teams feature 关闭时不导入 geo_location 相关类型
    #[cfg(feature = "teams")]
    use crate::domain::services::geo_location::{GeoLocation, GeoLocationService};
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult,
        QuotaService, RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError,
    };
    // R-teams-004 / T014：teams feature 关闭时不导入 team_service 相关类型
    #[cfg(feature = "teams")]
    use crate::domain::services::team_service::TeamGeoRestrictions;
    use async_trait::async_trait;
    use std::collections::HashSet;
    // R-teams-004 / T014：teams-off 时 IpAddr 未使用
    #[cfg(feature = "teams")]
    use std::net::IpAddr;
    use uuid::Uuid;

    // ============ No-op mocks ============
    // These mocks satisfy the trait bounds required by CrawlHandlerState.
    // Their methods are never invoked by the tests below; they exist only to
    // allow construction of the state wrapper for wiring/clone verification.

    struct MockCrawlRepository;
    #[async_trait]
    impl CrawlRepository for MockCrawlRepository {
        async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            Ok(crawl.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            Ok(crawl.clone())
        }
        async fn increment_completed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn increment_failed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn update_status(
            &self,
            _id: Uuid,
            _status: crate::domain::models::CrawlStatus,
        ) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn increment_total_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn find_by_team_id_paginated(
            &self,
            _team_id: Uuid,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<Crawl>, RepositoryError> {
            Ok(vec![])
        }
        async fn count_by_team_id(&self, _team_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
    }

    struct MockTaskRepository;
    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }
        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn mark_completed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn mark_failed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn mark_cancelled(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }
        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            Ok(HashSet::new())
        }
        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(vec![])
        }
        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }
        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }
    }

    // R-wh-003 / T027：webhook feature 关闭时不编译此 mock
    #[cfg(feature = "webhook")]
    struct MockWebhookRepository;
    #[cfg(feature = "webhook")]
    #[async_trait]
    impl WebhookRepository for MockWebhookRepository {
        async fn create(&self, webhook: &Webhook) -> Result<Webhook, RepositoryError> {
            Ok(webhook.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Webhook>, RepositoryError> {
            Ok(None)
        }
        async fn find_by_team_id(&self, _team_id: Uuid) -> Result<Vec<Webhook>, RepositoryError> {
            Ok(vec![])
        }
    }

    struct MockScrapeResultRepository;
    #[async_trait]
    impl ScrapeResultRepository for MockScrapeResultRepository {
        async fn save(&self, _result: ScrapeResult) -> anyhow::Result<()> {
            Ok(())
        }
        async fn find_by_task_id(&self, _task_id: Uuid) -> anyhow::Result<Option<ScrapeResult>> {
            Ok(None)
        }
        async fn find_by_task_ids(&self, _task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
            Ok(vec![])
        }
        async fn get_team_avg_response_time(&self, _team_id: Uuid) -> anyhow::Result<f64> {
            Ok(0.0)
        }
    }

    // R-teams-004 / T014：teams feature 关闭时不编译此 mock
    #[cfg(feature = "teams")]
    struct MockGeoRestrictionRepository;
    #[cfg(feature = "teams")]
    #[async_trait]
    impl GeoRestrictionRepository for MockGeoRestrictionRepository {
        async fn get_team_restrictions(
            &self,
            _team_id: Uuid,
        ) -> Result<
            TeamGeoRestrictions,
            crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
        > {
            Ok(TeamGeoRestrictions::default())
        }
        async fn update_team_restrictions(
            &self,
            _team_id: Uuid,
            _restrictions: &TeamGeoRestrictions,
        ) -> Result<
            (),
            crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
        > {
            Ok(())
        }
        async fn log_geo_restriction_action(
            &self,
            _team_id: Uuid,
            _ip_address: &str,
            _country_code: &str,
            _action: &str,
            _reason: &str,
        ) -> Result<
            (),
            crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
        > {
            Ok(())
        }
    }

    // R-teams-004 / T014：teams feature 关闭时不编译此 mock
    #[cfg(feature = "teams")]
    struct MockGeoLocationService;
    #[cfg(feature = "teams")]
    #[async_trait]
    impl GeoLocationService for MockGeoLocationService {
        async fn get_location(&self, _ip: &IpAddr) -> anyhow::Result<GeoLocation> {
            Ok(GeoLocation::default())
        }
    }

    struct MockRateLimitingService;
    #[async_trait]
    impl RateLimitService for MockRateLimitingService {
        async fn check_rate_limit(
            &self,
            _api_key: &str,
            _endpoint: &str,
        ) -> Result<RateLimitResult, RateLimitingError> {
            Ok(RateLimitResult::Allowed)
        }
        async fn get_team_rate_limit_config(
            &self,
            _team_id: Uuid,
        ) -> Result<RateLimitConfig, RateLimitingError> {
            Ok(RateLimitConfig::default())
        }
        async fn update_team_rate_limit_config(
            &self,
            _team_id: Uuid,
            _config: RateLimitConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
        async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
            Ok(0)
        }
    }
    #[async_trait]
    impl ConcurrencyControlService for MockRateLimitingService {
        async fn check_team_concurrency(
            &self,
            _team_id: Uuid,
            _task_id: Uuid,
        ) -> Result<ConcurrencyResult, RateLimitingError> {
            Ok(ConcurrencyResult::Allowed)
        }
        async fn release_team_concurrency_slot(
            &self,
            _team_id: Uuid,
            _task_id: Uuid,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
        async fn get_team_current_concurrency(
            &self,
            _team_id: Uuid,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
        async fn get_team_concurrency_config(
            &self,
            _team_id: Uuid,
        ) -> Result<ConcurrencyConfig, RateLimitingError> {
            Ok(ConcurrencyConfig::default())
        }
        async fn update_team_concurrency_config(
            &self,
            _team_id: Uuid,
            _config: ConcurrencyConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }
    #[async_trait]
    impl BacklogService for MockRateLimitingService {
        async fn process_backlog_tasks(&self, _team_id: Uuid) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
    }
    #[async_trait]
    impl QuotaService for MockRateLimitingService {
        async fn check_and_deduct_quota(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
        async fn get_quota_balance(&self, _team_id: Uuid) -> Result<i64, RateLimitingError> {
            Ok(0)
        }
    }
    impl RateLimitingService for MockRateLimitingService {}

    /// Build a CrawlHandlerState with no-op mocks for wiring/clone tests.
    ///
    /// R-teams-004 / R-wh-003：feature-off 时跳过对应依赖构造。
    fn build_test_state() -> CrawlHandlerState {
        let crawl_repo: Arc<dyn CrawlRepository> = Arc::new(MockCrawlRepository);
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository);
        let scrape_result_repo: Arc<dyn ScrapeResultRepository> =
            Arc::new(MockScrapeResultRepository);
        let rate_limiting_service: Arc<dyn RateLimitingService> = Arc::new(MockRateLimitingService);

        // R-teams-004 / T014：teams-on 时构造 geo_restriction_repo / team_service
        #[cfg(feature = "teams")]
        let geo_restriction_repo: Arc<dyn GeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository);
        #[cfg(feature = "teams")]
        let team_service = Arc::new(TeamService::new(
            Arc::new(MockGeoLocationService),
            geo_restriction_repo.clone(),
        ));

        // R-wh-003 / T027：webhook-on 时构造 webhook_repo
        #[cfg(feature = "webhook")]
        let webhook_repo: Arc<dyn WebhookRepository> = Arc::new(MockWebhookRepository);

        CrawlHandlerState::new(
            crawl_repo,
            task_repo,
            scrape_result_repo,
            rate_limiting_service,
            #[cfg(feature = "webhook")]
            webhook_repo,
            #[cfg(feature = "teams")]
            geo_restriction_repo,
            #[cfg(feature = "teams")]
            team_service,
        )
    }

    #[test]
    fn test_new_stores_all_dependencies() {
        let state = build_test_state();
        // Verify all fields are populated by accessing each through the trait.
        // Each accessor must return a valid Arc (not panic).
        let _ = state.crawl_repo();
        let _ = state.task_repo();
        let _ = state.result_repo();
        // R-wh-003 / T027：webhook-on 时验证 webhook_repo accessor
        #[cfg(feature = "webhook")]
        {
            let _ = state.webhook_repo();
        }
        // R-teams-004 / T014：teams-on 时验证 geo_restriction_repo / team_service accessor
        #[cfg(feature = "teams")]
        {
            let _ = state.geo_restriction_repo();
            let _ = state.team_service();
        }
        let _ = state.rate_limiting_service();
    }

    #[test]
    fn test_handler_state_trait_returns_injected_repositories() {
        let state = build_test_state();
        // The trait accessors must return Arcs pointing to the same underlying
        // objects that were injected via new(), verifying correct wiring.
        assert!(Arc::ptr_eq(&state.crawl_repo, &state.crawl_repo(),));
        assert!(Arc::ptr_eq(&state.task_repo, &state.task_repo(),));
        // R-wh-003 / T027：webhook-on 时验证 webhook_repo 注入一致性
        #[cfg(feature = "webhook")]
        {
            assert!(Arc::ptr_eq(&state.webhook_repo, &state.webhook_repo(),));
        }
        assert!(Arc::ptr_eq(&state.scrape_result_repo, &state.result_repo(),));
        // R-teams-004 / T014：teams-on 时验证 geo_restriction_repo 注入一致性
        #[cfg(feature = "teams")]
        {
            assert!(Arc::ptr_eq(
                &state.geo_restriction_repo,
                &state.geo_restriction_repo(),
            ));
        }
    }

    #[test]
    fn test_handler_state_trait_returns_injected_services() {
        let state = build_test_state();
        // R-teams-004 / T014：teams-on 时验证 team_service 注入一致性
        #[cfg(feature = "teams")]
        {
            assert!(Arc::ptr_eq(&state.team_service, &state.team_service(),));
        }
        assert!(Arc::ptr_eq(
            &state.rate_limiting_service,
            &state.rate_limiting_service(),
        ));
    }

    #[test]
    fn test_create_use_case_constructs_without_panic() {
        let state = build_test_state();
        let _use_case = state.create_use_case();
        // Reaching this point means CrawlUseCase::new accepted all injected deps.
    }

    #[test]
    fn test_clone_shares_underlying_state() {
        let state = build_test_state();
        let cloned = state.clone();
        // Clone must share the same underlying Arcs (not deep copies)
        assert!(Arc::ptr_eq(&state.crawl_repo, &cloned.crawl_repo));
        assert!(Arc::ptr_eq(&state.task_repo, &cloned.task_repo));
        // R-wh-003 / T027：webhook-on 时验证 webhook_repo clone 一致性
        #[cfg(feature = "webhook")]
        {
            assert!(Arc::ptr_eq(&state.webhook_repo, &cloned.webhook_repo));
        }
        assert!(Arc::ptr_eq(
            &state.scrape_result_repo,
            &cloned.scrape_result_repo
        ));
        // R-teams-004 / T014：teams-on 时验证 geo_restriction_repo / team_service clone 一致性
        #[cfg(feature = "teams")]
        {
            assert!(Arc::ptr_eq(
                &state.geo_restriction_repo,
                &cloned.geo_restriction_repo
            ));
            assert!(Arc::ptr_eq(&state.team_service, &cloned.team_service));
        }
        assert!(Arc::ptr_eq(
            &state.rate_limiting_service,
            &cloned.rate_limiting_service
        ));
    }

    #[test]
    fn test_clone_trait_accessor_returns_same_arc() {
        let state = build_test_state();
        let cloned = state.clone();
        // Trait accessor on the clone must return the same Arc as the original
        assert!(Arc::ptr_eq(&state.crawl_repo(), &cloned.crawl_repo()));
    }
}
