// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! External unit tests for presentation::state public API.
//!
//! Exercises CrawlHandlerState construction via the public `new` constructor,
//! the HandlerState trait accessor methods, Clone semantics, and
//! create_use_case wiring — all through the crawlrs crate's public exports
//! with no-op mock implementations of the required traits.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use crawlrs::application::use_cases::crawl_use_case::CrawlUseCase;
use crawlrs::domain::models::credits_model::CreditsTransactionType;
use crawlrs::domain::models::scrape_result::ScrapeResult;
use crawlrs::domain::models::{Crawl, Task, Webhook};
use crawlrs::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crawlrs::domain::repositories::task_repository::{
    RepositoryError, TaskQueryParams, TaskRepository,
};
use crawlrs::domain::repositories::{
    crawl_repository::CrawlRepository, scrape_result_repository::ScrapeResultRepository,
    webhook_repository::WebhookRepository,
};
use crawlrs::domain::services::geo_location::{GeoLocation, GeoLocationService};
use crawlrs::domain::services::rate_limiting_service::{
    BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult, QuotaService,
    RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError, RateLimitingService,
};
use crawlrs::domain::services::team_service::{TeamGeoRestrictions, TeamService};
use crawlrs::presentation::state::{CrawlHandlerState, HandlerState};

// =============================================================================
// No-op mock implementations
// =============================================================================

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
        _status: crawlrs::domain::models::CrawlStatus,
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
    async fn reset_stuck_tasks(&self, _timeout: chrono::Duration) -> Result<u64, RepositoryError> {
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

struct MockWebhookRepository;
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

struct MockGeoRestrictionRepository;
#[async_trait]
impl GeoRestrictionRepository for MockGeoRestrictionRepository {
    async fn get_team_restrictions(
        &self,
        _team_id: Uuid,
    ) -> Result<
        TeamGeoRestrictions,
        crawlrs::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
    > {
        Ok(TeamGeoRestrictions::default())
    }
    async fn update_team_restrictions(
        &self,
        _team_id: Uuid,
        _restrictions: &TeamGeoRestrictions,
    ) -> Result<
        (),
        crawlrs::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
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
        crawlrs::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
    > {
        Ok(())
    }
}

struct MockGeoLocationService;
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
    async fn get_team_current_concurrency(&self, _team_id: Uuid) -> Result<u32, RateLimitingError> {
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

fn build_test_state() -> CrawlHandlerState {
    let crawl_repo: Arc<dyn CrawlRepository> = Arc::new(MockCrawlRepository);
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository);
    let webhook_repo: Arc<dyn WebhookRepository> = Arc::new(MockWebhookRepository);
    let scrape_result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(MockScrapeResultRepository);
    let geo_restriction_repo: Arc<dyn GeoRestrictionRepository> =
        Arc::new(MockGeoRestrictionRepository);
    let team_service = Arc::new(TeamService::new(
        Arc::new(MockGeoLocationService),
        geo_restriction_repo.clone(),
    ));
    let rate_limiting_service: Arc<dyn RateLimitingService> = Arc::new(MockRateLimitingService);
    CrawlHandlerState::new(
        crawl_repo,
        task_repo,
        webhook_repo,
        scrape_result_repo,
        geo_restriction_repo,
        team_service,
        rate_limiting_service,
    )
}

// =============================================================================
// CrawlHandlerState::new — construction and field access
// =============================================================================

#[test]
fn tc_new_construction_all_fields_populated() {
    let state = build_test_state();
    // Access every field — must not panic and must return valid Arcs.
    let _ = &state.crawl_repo;
    let _ = &state.task_repo;
    let _ = &state.webhook_repo;
    let _ = &state.scrape_result_repo;
    let _ = &state.geo_restriction_repo;
    let _ = &state.team_service;
    let _ = &state.rate_limiting_service;
}

#[test]
fn tc_new_injected_repositories_are_returned_by_trait() {
    let state = build_test_state();
    // Trait accessors must return the same Arcs injected via new().
    assert!(Arc::ptr_eq(&state.crawl_repo, &state.crawl_repo()));
    assert!(Arc::ptr_eq(&state.task_repo, &state.task_repo()));
    assert!(Arc::ptr_eq(&state.webhook_repo, &state.webhook_repo()));
    assert!(Arc::ptr_eq(&state.scrape_result_repo, &state.result_repo()));
    assert!(Arc::ptr_eq(
        &state.geo_restriction_repo,
        &state.geo_restriction_repo()
    ));
}

#[test]
fn tc_new_injected_services_are_returned_by_trait() {
    let state = build_test_state();
    assert!(Arc::ptr_eq(&state.team_service, &state.team_service()));
    assert!(Arc::ptr_eq(
        &state.rate_limiting_service,
        &state.rate_limiting_service()
    ));
}

// =============================================================================
// HandlerState trait — all accessor methods
// =============================================================================

#[test]
fn tc_handler_state_task_repo_returns_injected() {
    let state = build_test_state();
    let repo = state.task_repo();
    // The accessor returns a clone of the Arc — must point to the same data.
    assert!(Arc::ptr_eq(&repo, &state.task_repo));
}

#[test]
fn tc_handler_state_crawl_repo_returns_injected() {
    let state = build_test_state();
    assert!(Arc::ptr_eq(&state.crawl_repo(), &state.crawl_repo));
}

#[test]
fn tc_handler_state_result_repo_returns_scrape_result_repo() {
    // result_repo() must return the scrape_result_repo field (different name).
    let state = build_test_state();
    assert!(Arc::ptr_eq(&state.result_repo(), &state.scrape_result_repo));
}

#[test]
fn tc_handler_state_webhook_repo_returns_injected() {
    let state = build_test_state();
    assert!(Arc::ptr_eq(&state.webhook_repo(), &state.webhook_repo));
}

#[test]
fn tc_handler_state_geo_restriction_repo_returns_injected() {
    let state = build_test_state();
    assert!(Arc::ptr_eq(
        &state.geo_restriction_repo(),
        &state.geo_restriction_repo
    ));
}

#[test]
fn tc_handler_state_team_service_returns_injected() {
    let state = build_test_state();
    assert!(Arc::ptr_eq(&state.team_service(), &state.team_service));
}

#[test]
fn tc_handler_state_rate_limiting_service_returns_injected() {
    let state = build_test_state();
    assert!(Arc::ptr_eq(
        &state.rate_limiting_service(),
        &state.rate_limiting_service
    ));
}

// =============================================================================
// CrawlHandlerState::create_use_case
// =============================================================================

#[test]
fn tc_create_use_case_returns_crawl_use_case() {
    let state = build_test_state();
    let use_case: CrawlUseCase = state.create_use_case();
    // Reaching this line means CrawlUseCase::new accepted all injected deps.
    drop(use_case);
}

#[test]
fn tc_create_use_case_does_not_panic_on_multiple_calls() {
    let state = build_test_state();
    let _ = state.create_use_case();
    let _ = state.create_use_case();
    let _ = state.create_use_case();
}

// =============================================================================
// CrawlHandlerState Clone semantics
// =============================================================================

#[test]
fn tc_clone_shares_all_underlying_arcs() {
    let state = build_test_state();
    let cloned = state.clone();
    assert!(Arc::ptr_eq(&state.crawl_repo, &cloned.crawl_repo));
    assert!(Arc::ptr_eq(&state.task_repo, &cloned.task_repo));
    assert!(Arc::ptr_eq(&state.webhook_repo, &cloned.webhook_repo));
    assert!(Arc::ptr_eq(
        &state.scrape_result_repo,
        &cloned.scrape_result_repo
    ));
    assert!(Arc::ptr_eq(
        &state.geo_restriction_repo,
        &cloned.geo_restriction_repo
    ));
    assert!(Arc::ptr_eq(&state.team_service, &cloned.team_service));
    assert!(Arc::ptr_eq(
        &state.rate_limiting_service,
        &cloned.rate_limiting_service
    ));
}

#[test]
fn tc_clone_trait_accessor_returns_same_arc_as_original() {
    let state = build_test_state();
    let cloned = state.clone();
    assert!(Arc::ptr_eq(&state.crawl_repo(), &cloned.crawl_repo()));
    assert!(Arc::ptr_eq(&state.task_repo(), &cloned.task_repo()));
}
