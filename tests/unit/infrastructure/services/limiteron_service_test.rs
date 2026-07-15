// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Limiteron service infrastructure tests
//!
//! Exercises `LimiteronService` against a **real** `limiteron::Governor` backed
//! by `MemoryStorage`, complementing the domain-layer delegation tests in
//! `tests/unit/domain/services/rate_limiting_service_test.rs` (which use mock
//! repositories and therefore never drive the Governor's `Decision` variants).
//!
//! Covered here:
//! - `RateLimitingConfig::default()` field verification (public struct)
//! - `LimiteronService::new()` success paths with real Governor
//! - `check_rate_limit` against the real Governor:
//!     * disabled → short-circuits to `Allowed`
//!     * enabled, any capacity → `Allowed` via the fail-open path
//!
//! # Source code limitation (documented, not fixable from tests)
//!
//! `build_request_context()` sets `ip: None`, `client_ip: None`, and an *empty*
//! `headers` map. The Governor's default `CompositeExtractor`
//! (`UserIdExtractor` from `X-User-Id`, `IpExtractor` from `client_ip`/headers,
//! `ApiKeyExtractor` from `X-API-Key`) therefore cannot extract any identifier
//! and returns `None`, which the Governor turns into
//! `Err(LimiteronError::ConfigError)`. The service catches this in its `Err`
//! arm and fails open to `RateLimitResult::Allowed`. Consequently the
//! `Decision::Allowed` / `Decision::Rejected` / `Decision::Banned` branches in
//! `check_rate_limit` are unreachable through the public service API without
//! modifying `src/`. This is pinned by
//! `test_check_rate_limit_fails_open_when_governor_cannot_extract_identifier`.
//!
//! The repository traits are required by the constructor but unused by
//! `check_rate_limit`, so minimal no-op stubs are sufficient here.

#![cfg(test)]
#![cfg(feature = "rate-limiting")]

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use crawlrs::domain::models::credits_model::{CreditsTransaction, CreditsTransactionType};
use crawlrs::domain::models::task_domain::{TaskStatus, TaskType};
use crawlrs::domain::models::task_model::Task;
use crawlrs::domain::repositories::credits_repository::{
    CreditsRepository, CreditsRepositoryError,
};
use crawlrs::domain::repositories::task_repository::{
    RepositoryError, TaskQueryParams, TaskRepository,
};
use crawlrs::domain::repositories::tasks_backlog_repository::{
    TasksBacklog, TasksBacklogRepository, TasksBacklogStatus,
};
use crawlrs::domain::services::rate_limiting_service::{RateLimitResult, RateLimitService};
use crawlrs::infrastructure::services::limiteron_service::{LimiteronService, RateLimitingConfig};

// =============================================================================
// Minimal no-op repository stubs
// `check_rate_limit` never touches these repositories; they exist only to
// satisfy `LimiteronService::new`'s constructor signature.
// =============================================================================

struct StubTaskRepository;

#[async_trait]
impl TaskRepository for StubTaskRepository {
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

struct StubBacklogRepository;

#[async_trait]
impl TasksBacklogRepository for StubBacklogRepository {
    async fn create(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
        Ok(backlog.clone())
    }
    async fn find_by_id(&self, _id: Uuid) -> Result<Option<TasksBacklog>, RepositoryError> {
        Ok(None)
    }
    async fn find_by_task_id(
        &self,
        _task_id: Uuid,
    ) -> Result<Option<TasksBacklog>, RepositoryError> {
        Ok(None)
    }
    async fn update(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
        Ok(backlog.clone())
    }
    async fn delete(&self, _id: Uuid) -> Result<(), RepositoryError> {
        Ok(())
    }
    async fn get_pending_tasks(
        &self,
        _team_id: Option<Uuid>,
        _limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError> {
        Ok(vec![])
    }
    async fn get_expired_tasks(
        &self,
        _limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError> {
        Ok(vec![])
    }
    async fn count_by_status(
        &self,
        _team_id: Option<Uuid>,
        _status: TasksBacklogStatus,
    ) -> Result<i64, RepositoryError> {
        Ok(0)
    }
    async fn update_status_batch(
        &self,
        _ids: &[Uuid],
        _status: TasksBacklogStatus,
    ) -> Result<u64, RepositoryError> {
        Ok(0)
    }
}

struct StubCreditsRepository;

#[async_trait]
impl CreditsRepository for StubCreditsRepository {
    async fn get_balance(&self, _team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
        Ok(0)
    }
    async fn deduct_credits(
        &self,
        _team_id: Uuid,
        _amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<(), CreditsRepositoryError> {
        Ok(())
    }
    async fn add_credits(
        &self,
        _team_id: Uuid,
        _amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<i64, CreditsRepositoryError> {
        Ok(0)
    }
    async fn get_transaction_history(
        &self,
        _team_id: Uuid,
        _limit: Option<u32>,
    ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
        Ok(vec![])
    }
    async fn initialize_team_credits(
        &self,
        _team_id: Uuid,
        initial_balance: i64,
    ) -> Result<i64, CreditsRepositoryError> {
        Ok(initial_balance)
    }
}

// =============================================================================
// Harness
// =============================================================================

async fn make_service(config: RateLimitingConfig) -> LimiteronService {
    LimiteronService::new(
        Arc::new(StubTaskRepository) as Arc<dyn TaskRepository>,
        Arc::new(StubBacklogRepository) as Arc<dyn TasksBacklogRepository>,
        Arc::new(StubCreditsRepository) as Arc<dyn CreditsRepository>,
        config,
    )
    .await
    .expect("Failed to build LimiteronService with real Governor")
}

// =============================================================================
// RateLimitingConfig::default() tests
// =============================================================================

#[test]
fn test_rate_limiting_config_default_has_expected_values() {
    let config = RateLimitingConfig::default();

    // Top-level timing fields
    assert_eq!(config.backlog_process_interval_seconds, 30);
    assert_eq!(config.rate_limit_ttl_seconds, 3600);

    // Nested RateLimitConfig defaults
    assert!(config.rate_limit.enabled);
    assert_eq!(config.rate_limit.requests_per_second, 10);
    assert_eq!(config.rate_limit.requests_per_minute, 100);
    assert_eq!(config.rate_limit.requests_per_hour, 1000);
    assert_eq!(config.rate_limit.bucket_capacity, Some(100));

    // Nested ConcurrencyConfig defaults
    assert!(config.concurrency.enabled);
    assert_eq!(config.concurrency.max_concurrent_tasks, 100);
    assert_eq!(config.concurrency.max_concurrent_per_team, 10);
    assert_eq!(config.concurrency.lock_timeout_seconds, 300);
}

#[test]
fn test_rate_limiting_config_default_is_independently_mutable() {
    // Default-derived configs must be independently mutable (Clone, no shared state).
    let mut config = RateLimitingConfig::default();
    config.rate_limit.requests_per_second = 42;
    config.concurrency.max_concurrent_per_team = 7;

    // Verify the mutations took effect on `config`.
    assert_eq!(config.rate_limit.requests_per_second, 42);
    assert_eq!(config.concurrency.max_concurrent_per_team, 7);

    // A fresh default must be unaffected by the above mutations.
    let fresh = RateLimitingConfig::default();
    assert_eq!(fresh.rate_limit.requests_per_second, 10);
    assert_eq!(fresh.concurrency.max_concurrent_per_team, 10);
}

// =============================================================================
// LimiteronService::new() tests
// =============================================================================

#[tokio::test]
async fn test_new_with_default_config_builds_real_governor() {
    // Constructor must succeed and produce a usable Governor (real MemoryStorage).
    let service = make_service(RateLimitingConfig::default()).await;

    // Smoke test: a rate-limit check against the real Governor must succeed.
    let result = service.check_rate_limit("test-api-key", "/v1/scrape").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), RateLimitResult::Allowed);
}

#[tokio::test]
async fn test_new_with_custom_low_capacity_config_succeeds() {
    let mut config = RateLimitingConfig::default();
    config.rate_limit.bucket_capacity = Some(2);
    config.rate_limit.requests_per_second = 2;

    let service = make_service(config).await;
    let result = service.check_rate_limit("k", "/v1/extract").await;
    assert!(result.is_ok());
}

// =============================================================================
// RateLimitService::check_rate_limit tests (real Governor)
// =============================================================================

#[tokio::test]
async fn test_check_rate_limit_disabled_short_circuits_to_allowed() {
    // When rate limiting is disabled globally, the Governor is never consulted.
    let mut config = RateLimitingConfig::default();
    config.rate_limit.enabled = false;
    let service = make_service(config).await;

    let result = service.check_rate_limit("any-key", "/v1/extract").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), RateLimitResult::Allowed);
}

#[tokio::test]
async fn test_check_rate_limit_allowed_under_capacity() {
    // NOTE: Due to `build_request_context()` setting `ip: None`, `client_ip: None`,
    // and an empty `headers` map, the Governor's default `CompositeExtractor`
    // cannot extract an identifier → `Err(ConfigError)` → fail-open → `Allowed`.
    // This test therefore exercises the fail-open `Err` arm, NOT the
    // `Decision::Allowed` branch. See the module docs for the full explanation.
    let mut config = RateLimitingConfig::default();
    config.rate_limit.enabled = true;
    config.rate_limit.bucket_capacity = Some(1000);
    config.rate_limit.requests_per_second = 1000;
    let service = make_service(config).await;

    let result = service
        .check_rate_limit("key-under-cap", "/v1/extract")
        .await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), RateLimitResult::Allowed);
}

#[tokio::test]
async fn test_check_rate_limit_fails_open_when_governor_cannot_extract_identifier() {
    // SOURCE CODE LIMITATION (documented, not fixable from tests without
    // modifying `src/`):
    //
    // `LimiteronService::build_request_context()` constructs a `RequestContext`
    // with `ip: None`, `client_ip: None`, and an *empty* `headers` map. The
    // Governor's default `CompositeExtractor` (built in `governor.rs` when no
    // custom extractor is supplied) tries, in order:
    //   1. `UserIdExtractor::from_header("X-User-Id")` — reads `headers["X-User-Id"]`
    //   2. `IpExtractor::builder().build()`            — reads `headers[*]`, then `client_ip`
    //   3. `ApiKeyExtractor::from_header("X-API-Key")`  — reads `headers["X-API-Key"]`
    //
    // With empty headers and `client_ip: None`, every extractor returns `None`,
    // so `identifier_extractor.extract(context)` returns `None`, which the
    // Governor maps to `Err(LimiteronError::ConfigError("Failed to extract
    // identifier"))`. The service's `Err` arm then fails open to
    // `RateLimitResult::Allowed`.
    //
    // This means the `Decision::Rejected` / `Decision::Banned` branches in
    // `check_rate_limit` are UNREACHABLE through the public service API.
    //
    // This test pins that behavior: even with capacity=1 / refill=1, a burst of
    // requests all return `Allowed` (via fail-open), never `Denied`.
    let mut config = RateLimitingConfig::default();
    config.rate_limit.enabled = true;
    config.rate_limit.bucket_capacity = Some(1);
    config.rate_limit.requests_per_second = 1;
    let service = make_service(config).await;

    for _ in 0..10 {
        let result = service
            .check_rate_limit("key-a", "/v1/extract")
            .await
            .expect("check_rate_limit must not return an Err");
        assert_eq!(
            result,
            RateLimitResult::Allowed,
            "expected fail-open Allowed (identifier extraction fails), got {:?}",
            result
        );
    }
}

#[tokio::test]
async fn test_check_rate_limit_handles_short_api_key_safely() {
    // The service slices api_key[..min(8, len)] for logging; an api_key shorter
    // than 8 chars must not panic (regression guard for the debug!/warn! macros).
    let mut config = RateLimitingConfig::default();
    config.rate_limit.enabled = true;
    config.rate_limit.bucket_capacity = Some(1000);
    config.rate_limit.requests_per_second = 1000;
    let service = make_service(config).await;

    let result = service.check_rate_limit("ab", "/v1/extract").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), RateLimitResult::Allowed);
}

// Silence unused-import / dead-code warnings for stub fields only referenced via
// trait dispatch in tests that exercise the real Governor (these types are part
// of the harness and are intentionally minimal).
#[allow(dead_code)]
fn _ensure_traits_are_object_safe() {
    let _ = std::any::TypeId::of::<dyn TaskRepository>();
    let _ = std::any::TypeId::of::<dyn TasksBacklogRepository>();
    let _ = std::any::TypeId::of::<dyn CreditsRepository>();
}

// Silence unused warnings for stub-only imports.
#[allow(dead_code)]
fn _touch_unused() {
    let _ = TaskStatus::Queued;
    let _ = TaskType::Scrape;
}
