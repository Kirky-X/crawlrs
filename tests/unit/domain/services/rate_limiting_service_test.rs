// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version2.0
// See LICENSE file in the project root for full license information.

//! Rate limiting service delegation tests
//!
//! Verifies that `LimiteronService` correctly implements the domain traits
//! (`RateLimitService`, `ConcurrencyControlService`, `BacklogService`,
//! `QuotaService`, and the composite `RateLimitingService`) and delegates
//! to its repositories/governor as expected.
//!
//! Per AGENTS.md: no mock library — tests use real trait impls with
//! test-specific behavior and `Arc<AtomicBool>`/`Arc<Mutex<>>` for state.

#![cfg(test)]
#![cfg(feature = "rate-limiting")]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
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
use crawlrs::domain::services::rate_limiting_service::{
    BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult, QuotaService,
    RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError, RateLimitingService,
};
use crawlrs::infrastructure::services::limiteron_service::{LimiteronService, RateLimitingConfig};

// === Mock Task Repository ===

struct MockTaskRepository {
    tasks: Arc<std::sync::Mutex<HashMap<Uuid, Task>>>,
    should_fail: Arc<AtomicBool>,
}

impl MockTaskRepository {
    fn new() -> Self {
        Self {
            tasks: Arc::new(std::sync::Mutex::new(HashMap::new())),
            should_fail: Arc::new(AtomicBool::new(false)),
        }
    }

    fn with_task(task: Task) -> Self {
        let mut tasks = HashMap::new();
        tasks.insert(task.id, task);
        Self {
            tasks: Arc::new(std::sync::Mutex::new(tasks)),
            should_fail: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[async_trait]
impl TaskRepository for MockTaskRepository {
    async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(RepositoryError::Database(anyhow::anyhow!("mock error")));
        }
        let mut tasks = self.tasks.lock().unwrap();
        tasks.insert(task.id, task.clone());
        Ok(task.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(RepositoryError::Database(anyhow::anyhow!("mock error")));
        }
        let tasks = self.tasks.lock().unwrap();
        Ok(tasks.get(&id).cloned())
    }

    async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(RepositoryError::Database(anyhow::anyhow!("mock error")));
        }
        let mut tasks = self.tasks.lock().unwrap();
        tasks.insert(task.id, task.clone());
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
        let tasks = self.tasks.lock().unwrap();
        Ok((tasks.values().cloned().collect(), tasks.len() as u64))
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

// === Mock TasksBacklog Repository ===

struct MockTasksBacklogRepository {
    backlogs: Arc<std::sync::Mutex<Vec<TasksBacklog>>>,
    should_fail: Arc<AtomicBool>,
    create_count: Arc<std::sync::atomic::AtomicU32>,
}

impl MockTasksBacklogRepository {
    fn new() -> Self {
        Self {
            backlogs: Arc::new(std::sync::Mutex::new(Vec::new())),
            should_fail: Arc::new(AtomicBool::new(false)),
            create_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    fn create_count(&self) -> u32 {
        self.create_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl TasksBacklogRepository for MockTasksBacklogRepository {
    async fn create(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(RepositoryError::Database(anyhow::anyhow!("mock error")));
        }
        self.create_count.fetch_add(1, Ordering::SeqCst);
        let mut backlogs = self.backlogs.lock().unwrap();
        backlogs.push(backlog.clone());
        Ok(backlog.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<TasksBacklog>, RepositoryError> {
        let backlogs = self.backlogs.lock().unwrap();
        Ok(backlogs.iter().find(|b| b.id == id).cloned())
    }

    async fn find_by_task_id(
        &self,
        task_id: Uuid,
    ) -> Result<Option<TasksBacklog>, RepositoryError> {
        let backlogs = self.backlogs.lock().unwrap();
        Ok(backlogs.iter().find(|b| b.task_id == task_id).cloned())
    }

    async fn update(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(RepositoryError::Database(anyhow::anyhow!("mock error")));
        }
        let mut backlogs = self.backlogs.lock().unwrap();
        if let Some(b) = backlogs.iter_mut().find(|b| b.id == backlog.id) {
            *b = backlog.clone();
            Ok(backlog.clone())
        } else {
            Err(RepositoryError::NotFound)
        }
    }

    async fn delete(&self, id: Uuid) -> Result<(), RepositoryError> {
        let mut backlogs = self.backlogs.lock().unwrap();
        backlogs.retain(|b| b.id != id);
        Ok(())
    }

    async fn get_pending_tasks(
        &self,
        team_id: Option<Uuid>,
        limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(RepositoryError::Database(anyhow::anyhow!("mock error")));
        }
        let backlogs = self.backlogs.lock().unwrap();
        let mut result: Vec<TasksBacklog> = backlogs
            .iter()
            .filter(|b| {
                b.status == TasksBacklogStatus::Pending
                    && team_id.is_none_or(|tid| b.team_id == tid)
            })
            .cloned()
            .collect();
        // Sort by priority descending (matches repo convention)
        result.sort_by_key(|b| std::cmp::Reverse(b.priority));
        if let Some(lim) = limit {
            result.truncate(lim as usize);
        }
        Ok(result)
    }

    async fn get_expired_tasks(
        &self,
        limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError> {
        let backlogs = self.backlogs.lock().unwrap();
        let mut result: Vec<TasksBacklog> = backlogs
            .iter()
            .filter(|b| b.is_expired())
            .cloned()
            .collect();
        if let Some(lim) = limit {
            result.truncate(lim as usize);
        }
        Ok(result)
    }

    async fn count_by_status(
        &self,
        _team_id: Option<Uuid>,
        status: TasksBacklogStatus,
    ) -> Result<i64, RepositoryError> {
        let backlogs = self.backlogs.lock().unwrap();
        Ok(backlogs.iter().filter(|b| b.status == status).count() as i64)
    }

    async fn update_status_batch(
        &self,
        ids: &[Uuid],
        status: TasksBacklogStatus,
    ) -> Result<u64, RepositoryError> {
        let mut backlogs = self.backlogs.lock().unwrap();
        let mut count = 0u64;
        for b in backlogs.iter_mut() {
            if ids.contains(&b.id) {
                b.status = status;
                b.updated_at = Utc::now();
                count += 1;
            }
        }
        Ok(count)
    }
}

// === Mock Credits Repository ===

struct MockCreditsRepository {
    balances: Arc<std::sync::Mutex<HashMap<Uuid, i64>>>,
    should_fail: Arc<AtomicBool>,
    deduct_count: Arc<std::sync::atomic::AtomicU32>,
}

impl MockCreditsRepository {
    fn new() -> Self {
        Self {
            balances: Arc::new(std::sync::Mutex::new(HashMap::new())),
            should_fail: Arc::new(AtomicBool::new(false)),
            deduct_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    fn with_balance(team_id: Uuid, balance: i64) -> Self {
        let mut balances = HashMap::new();
        balances.insert(team_id, balance);
        Self {
            balances: Arc::new(std::sync::Mutex::new(balances)),
            should_fail: Arc::new(AtomicBool::new(false)),
            deduct_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    fn failing() -> Self {
        Self {
            balances: Arc::new(std::sync::Mutex::new(HashMap::new())),
            should_fail: Arc::new(AtomicBool::new(true)),
            deduct_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    fn deduct_count(&self) -> u32 {
        self.deduct_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl CreditsRepository for MockCreditsRepository {
    async fn get_balance(&self, team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(CreditsRepositoryError::DatabaseError(
                "mock error".to_string(),
            ));
        }
        let balances = self.balances.lock().unwrap();
        Ok(*balances.get(&team_id).unwrap_or(&0))
    }

    async fn deduct_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<(), CreditsRepositoryError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(CreditsRepositoryError::DatabaseError(
                "mock error".to_string(),
            ));
        }
        self.deduct_count.fetch_add(1, Ordering::SeqCst);
        let mut balances = self.balances.lock().unwrap();
        let balance = balances.entry(team_id).or_insert(0);
        *balance -= amount;
        Ok(())
    }

    async fn add_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<i64, CreditsRepositoryError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(CreditsRepositoryError::DatabaseError(
                "mock error".to_string(),
            ));
        }
        let mut balances = self.balances.lock().unwrap();
        let balance = balances.entry(team_id).or_insert(0);
        *balance += amount;
        Ok(*balance)
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
        team_id: Uuid,
        initial_balance: i64,
    ) -> Result<i64, CreditsRepositoryError> {
        let mut balances = self.balances.lock().unwrap();
        balances.insert(team_id, initial_balance);
        Ok(initial_balance)
    }
}

// === Helpers ===

fn make_test_task() -> Task {
    Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id: Uuid::new_v4(),
        api_key_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: Utc::now(),
        lock_token: None,
        lock_expires_at: None,
    }
}

async fn make_service(
    task_repo: Arc<MockTaskRepository>,
    backlog_repo: Arc<MockTasksBacklogRepository>,
    credits_repo: Arc<MockCreditsRepository>,
    config: RateLimitingConfig,
) -> LimiteronService {
    LimiteronService::new(
        task_repo as Arc<dyn TaskRepository>,
        backlog_repo as Arc<dyn TasksBacklogRepository>,
        credits_repo as Arc<dyn CreditsRepository>,
        config,
    )
    .await
    .expect("Failed to create LimiteronService")
}

fn disabled_rate_config() -> RateLimitingConfig {
    let mut config = RateLimitingConfig::default();
    config.rate_limit.enabled = false;
    config.concurrency.enabled = false;
    config
}

// === Trait implementation compile-time checks ===
// These verify that LimiteronService implements all the domain traits.

#[test]
fn test_limiteron_service_implements_rate_limit_service() {
    fn _assert_impl<T: RateLimitService>() {}
    _assert_impl::<LimiteronService>();
}

#[test]
fn test_limiteron_service_implements_concurrency_control_service() {
    fn _assert_impl<T: ConcurrencyControlService>() {}
    _assert_impl::<LimiteronService>();
}

#[test]
fn test_limiteron_service_implements_backlog_service() {
    fn _assert_impl<T: BacklogService>() {}
    _assert_impl::<LimiteronService>();
}

#[test]
fn test_limiteron_service_implements_quota_service() {
    fn _assert_impl<T: QuotaService>() {}
    _assert_impl::<LimiteronService>();
}

#[test]
fn test_limiteron_service_implements_composite_rate_limiting_service() {
    fn _assert_impl<T: RateLimitingService>() {}
    _assert_impl::<LimiteronService>();
}

// === Construction tests ===

#[tokio::test]
async fn test_new_with_default_config_succeeds() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let config = RateLimitingConfig::default();

    let service = LimiteronService::new(
        task_repo as Arc<dyn TaskRepository>,
        backlog_repo as Arc<dyn TasksBacklogRepository>,
        credits_repo as Arc<dyn CreditsRepository>,
        config,
    )
    .await;

    assert!(
        service.is_ok(),
        "construction with default config should succeed"
    );
}

#[tokio::test]
async fn test_new_preserves_config() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());

    let mut config = RateLimitingConfig::default();
    config.rate_limit.requests_per_second = 42;
    config.concurrency.max_concurrent_per_team = 7;

    let service = make_service(task_repo, backlog_repo, credits_repo, config).await;

    // Verify config is returned via the trait accessor
    let team_id = Uuid::new_v4();
    let rl_config = service.get_team_rate_limit_config(team_id).await.unwrap();
    assert_eq!(rl_config.requests_per_second, 42);

    let cc_config = service.get_team_concurrency_config(team_id).await.unwrap();
    assert_eq!(cc_config.max_concurrent_per_team, 7);
}

// === RateLimitService delegation tests ===

#[tokio::test]
async fn test_check_rate_limit_disabled_returns_allowed() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let result = service.check_rate_limit("api_key_1", "/extract").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), RateLimitResult::Allowed);
}

#[tokio::test]
async fn test_check_rate_limit_enabled_returns_allowed_under_threshold() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());

    // Rate limit enabled but with high capacity — first request should pass
    let mut config = RateLimitingConfig::default();
    config.rate_limit.enabled = true;
    config.rate_limit.bucket_capacity = Some(1000);
    config.rate_limit.requests_per_second = 1000;
    config.concurrency.enabled = false;

    let service = make_service(task_repo, backlog_repo, credits_repo, config).await;

    let result = service.check_rate_limit("api_key_1", "/extract").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), RateLimitResult::Allowed);
}

#[tokio::test]
async fn test_get_team_rate_limit_config_returns_stored_config() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());

    let mut config = RateLimitingConfig::default();
    config.rate_limit.requests_per_second = 25;
    config.rate_limit.requests_per_minute = 250;
    config.rate_limit.requests_per_hour = 2500;

    let service = make_service(task_repo, backlog_repo, credits_repo, config).await;

    let team_id = Uuid::new_v4();
    let result = service.get_team_rate_limit_config(team_id).await;
    assert!(result.is_ok());
    let returned = result.unwrap();
    assert_eq!(returned.requests_per_second, 25);
    assert_eq!(returned.requests_per_minute, 250);
    assert_eq!(returned.requests_per_hour, 2500);
}

#[tokio::test]
async fn test_update_team_rate_limit_config_is_noop_returns_ok() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let team_id = Uuid::new_v4();
    let new_config = RateLimitConfig::default();
    let result = service
        .update_team_rate_limit_config(team_id, new_config)
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_cleanup_expired_rate_limits_returns_zero() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let result = service.cleanup_expired_rate_limits().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}

// === ConcurrencyControlService delegation tests ===

#[tokio::test]
async fn test_check_team_concurrency_disabled_returns_allowed() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let result = service.check_team_concurrency(team_id, task_id).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ConcurrencyResult::Allowed);
}

#[tokio::test]
async fn test_check_team_concurrency_task_not_found_returns_database_error() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());

    // Enable concurrency, but task does not exist in repo
    let mut config = disabled_rate_config();
    config.concurrency.enabled = true;
    config.concurrency.max_concurrent_per_team = 10;

    let service = make_service(task_repo, backlog_repo, credits_repo, config).await;

    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let result = service.check_team_concurrency(team_id, task_id).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        RateLimitingError::DatabaseError
    ));
}

#[tokio::test]
async fn test_check_team_concurrency_task_repo_failure_returns_database_error() {
    let task_repo = Arc::new(MockTaskRepository::new());
    task_repo.should_fail.store(true, Ordering::SeqCst);
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());

    let mut config = disabled_rate_config();
    config.concurrency.enabled = true;

    let service = make_service(task_repo, backlog_repo, credits_repo, config).await;

    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let result = service.check_team_concurrency(team_id, task_id).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        RateLimitingError::DatabaseError
    ));
}

#[tokio::test]
async fn test_check_team_concurrency_under_limit_returns_allowed() {
    let task = make_test_task();
    let task_repo = Arc::new(MockTaskRepository::with_task(task.clone()));
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());

    let mut config = disabled_rate_config();
    config.concurrency.enabled = true;
    config.concurrency.max_concurrent_per_team = 10;

    let service = make_service(task_repo, backlog_repo, credits_repo, config).await;

    // get_team_current_concurrency returns 0, so 0 < 10 → Allowed
    let result = service.check_team_concurrency(task.team_id, task.id).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ConcurrencyResult::Allowed);
}

#[tokio::test]
async fn test_check_team_concurrency_at_limit_returns_queued() {
    let task = make_test_task();
    let task_repo = Arc::new(MockTaskRepository::with_task(task.clone()));
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());

    let mut config = disabled_rate_config();
    config.concurrency.enabled = true;
    // Set max_concurrent_per_team = 0 so that 0 < 0 is false → queue path
    config.concurrency.max_concurrent_per_team = 0;

    let service = make_service(task_repo, backlog_repo.clone(), credits_repo, config).await;

    let result = service.check_team_concurrency(task.team_id, task.id).await;
    assert!(result.is_ok());
    match result.unwrap() {
        ConcurrencyResult::Queued { backlog_id } => {
            // Verify backlog was created in repo
            let found = backlog_repo.find_by_id(backlog_id).await.unwrap();
            assert!(found.is_some(), "backlog should be persisted in repo");
            let bl = found.unwrap();
            assert_eq!(bl.task_id, task.id);
            assert_eq!(bl.team_id, task.team_id);
        }
        other => panic!("expected Queued, got {:?}", other),
    }
    assert_eq!(backlog_repo.create_count(), 1);
}

#[tokio::test]
async fn test_check_team_concurrency_at_limit_backlog_failure_returns_database_error() {
    let task = make_test_task();
    let task_repo = Arc::new(MockTaskRepository::with_task(task.clone()));
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    backlog_repo.should_fail.store(true, Ordering::SeqCst);
    let credits_repo = Arc::new(MockCreditsRepository::new());

    let mut config = disabled_rate_config();
    config.concurrency.enabled = true;
    config.concurrency.max_concurrent_per_team = 0;

    let service = make_service(task_repo, backlog_repo, credits_repo, config).await;

    let result = service.check_team_concurrency(task.team_id, task.id).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        RateLimitingError::DatabaseError
    ));
}

#[tokio::test]
async fn test_get_team_current_concurrency_returns_zero() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let team_id = Uuid::new_v4();
    let result = service.get_team_current_concurrency(team_id).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}

#[tokio::test]
async fn test_get_team_concurrency_config_returns_stored_config() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());

    let mut config = disabled_rate_config();
    config.concurrency.max_concurrent_tasks = 50;
    config.concurrency.max_concurrent_per_team = 5;
    config.concurrency.lock_timeout_seconds = 120;

    let service = make_service(task_repo, backlog_repo, credits_repo, config).await;

    let team_id = Uuid::new_v4();
    let result = service.get_team_concurrency_config(team_id).await;
    assert!(result.is_ok());
    let returned = result.unwrap();
    assert_eq!(returned.max_concurrent_tasks, 50);
    assert_eq!(returned.max_concurrent_per_team, 5);
    assert_eq!(returned.lock_timeout_seconds, 120);
}

#[tokio::test]
async fn test_update_team_concurrency_config_is_noop_returns_ok() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let team_id = Uuid::new_v4();
    let new_config = ConcurrencyConfig::default();
    let result = service
        .update_team_concurrency_config(team_id, new_config)
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_release_team_concurrency_slot_returns_ok() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let result = service
        .release_team_concurrency_slot(team_id, task_id)
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_release_team_concurrency_slot_processes_backlog() {
    // When releasing a slot, process_backlog_tasks is invoked.
    // With no pending backlogs, it returns 0.
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let result = service
        .release_team_concurrency_slot(team_id, task_id)
        .await;
    assert!(result.is_ok());
}

// === BacklogService delegation tests ===

#[tokio::test]
async fn test_process_backlog_tasks_empty_returns_zero() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let team_id = Uuid::new_v4();
    let result = service.process_backlog_tasks(team_id).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}

#[tokio::test]
async fn test_process_backlog_tasks_repo_failure_returns_database_error() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    backlog_repo.should_fail.store(true, Ordering::SeqCst);
    let credits_repo = Arc::new(MockCreditsRepository::new());

    let mut config = disabled_rate_config();
    config.concurrency.enabled = true;

    let service = make_service(task_repo, backlog_repo, credits_repo, config).await;

    let team_id = Uuid::new_v4();
    let result = service.process_backlog_tasks(team_id).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        RateLimitingError::DatabaseError
    ));
}

#[tokio::test]
async fn test_process_backlog_tasks_skips_expired_entries() {
    // Insert an expired backlog directly into the mock repo.
    // process_backlog_tasks should detect it as expired, mark it expired, and skip.
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());

    let team_id = Uuid::new_v4();
    let mut expired_backlog = TasksBacklog::new(
        Uuid::new_v4(),
        team_id,
        "scrape".to_string(),
        1,
        serde_json::json!({}),
        Some(Utc::now() - chrono::Duration::hours(1)), // expired
    );
    // Mark as processing so the code path that updates status can mark it expired
    expired_backlog.mark_processing().unwrap();

    // Directly inject into the mock's internal storage
    {
        let mut backlogs = backlog_repo.backlogs.lock().unwrap();
        backlogs.push(expired_backlog.clone());
    }

    let mut config = disabled_rate_config();
    config.concurrency.enabled = true;
    config.concurrency.max_concurrent_per_team = 10;

    let service = make_service(task_repo, backlog_repo, credits_repo, config).await;

    let result = service.process_backlog_tasks(team_id).await;
    assert!(result.is_ok());
    // Expired entries are skipped, not counted as processed
    assert_eq!(result.unwrap(), 0);
}

// === QuotaService delegation tests ===

#[tokio::test]
async fn test_get_quota_balance_returns_stored_balance() {
    let team_id = Uuid::new_v4();
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::with_balance(team_id, 500));

    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let result = service.get_quota_balance(team_id).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 500);
}

#[tokio::test]
async fn test_get_quota_balance_repo_failure_returns_credits_error() {
    let team_id = Uuid::new_v4();
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::failing());

    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let result = service.get_quota_balance(team_id).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        RateLimitingError::CreditsError
    ));
}

#[tokio::test]
async fn test_check_and_deduct_quota_sufficient_balance_succeeds() {
    let team_id = Uuid::new_v4();
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));

    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo.clone(),
        disabled_rate_config(),
    )
    .await;

    let result = service
        .check_and_deduct_quota(
            team_id,
            30,
            CreditsTransactionType::Scrape,
            "test deduction".to_string(),
            Some(Uuid::new_v4()),
        )
        .await;
    assert!(result.is_ok());
    assert_eq!(credits_repo.deduct_count(), 1);

    // Verify balance was actually deducted
    let balance = service.get_quota_balance(team_id).await.unwrap();
    assert_eq!(balance, 70);
}

#[tokio::test]
async fn test_check_and_deduct_quota_insufficient_balance_returns_exceeded() {
    let team_id = Uuid::new_v4();
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::with_balance(team_id, 10));

    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo.clone(),
        disabled_rate_config(),
    )
    .await;

    let result = service
        .check_and_deduct_quota(
            team_id,
            50,
            CreditsTransactionType::Scrape,
            "test deduction".to_string(),
            None,
        )
        .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        RateLimitingError::RateLimitExceeded(msg) => {
            assert!(msg.contains("Insufficient credits"));
            assert!(msg.contains("required 50"));
            assert!(msg.contains("available 10"));
        }
        other => panic!("expected RateLimitExceeded, got {:?}", other),
    }
    // Deduct should NOT have been called
    assert_eq!(credits_repo.deduct_count(), 0);
}

#[tokio::test]
async fn test_check_and_deduct_quota_repo_failure_returns_credits_error() {
    let team_id = Uuid::new_v4();
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::failing());

    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let result = service
        .check_and_deduct_quota(
            team_id,
            10,
            CreditsTransactionType::Scrape,
            "test".to_string(),
            None,
        )
        .await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        RateLimitingError::CreditsError
    ));
}

#[tokio::test]
async fn test_check_and_deduct_quota_exact_balance_succeeds() {
    let team_id = Uuid::new_v4();
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::with_balance(team_id, 50));

    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo.clone(),
        disabled_rate_config(),
    )
    .await;

    let result = service
        .check_and_deduct_quota(
            team_id,
            50,
            CreditsTransactionType::Extract,
            "exact balance".to_string(),
            None,
        )
        .await;
    assert!(result.is_ok());
    let balance = service.get_quota_balance(team_id).await.unwrap();
    assert_eq!(balance, 0);
}

#[tokio::test]
async fn test_check_and_deduct_quota_zero_amount_succeeds() {
    let team_id = Uuid::new_v4();
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));

    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo.clone(),
        disabled_rate_config(),
    )
    .await;

    let result = service
        .check_and_deduct_quota(
            team_id,
            0,
            CreditsTransactionType::Crawl,
            "zero deduction".to_string(),
            None,
        )
        .await;
    assert!(result.is_ok());
    assert_eq!(credits_repo.deduct_count(), 1);
    let balance = service.get_quota_balance(team_id).await.unwrap();
    assert_eq!(balance, 100);
}

// === Composite RateLimitingService trait delegation tests ===
// Verify that calling through the composite trait object dispatches to LimiteronService.

#[tokio::test]
async fn test_composite_trait_dispatches_check_rate_limit() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    // Use the composite trait as a trait object
    let composite: Arc<dyn RateLimitingService> = Arc::new(service);
    let result = composite.check_rate_limit("key", "/ep").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), RateLimitResult::Allowed);
}

#[tokio::test]
async fn test_composite_trait_dispatches_check_team_concurrency() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let composite: Arc<dyn RateLimitingService> = Arc::new(service);
    let result = composite
        .check_team_concurrency(Uuid::new_v4(), Uuid::new_v4())
        .await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ConcurrencyResult::Allowed);
}

#[tokio::test]
async fn test_composite_trait_dispatches_process_backlog_tasks() {
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::new());
    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;

    let composite: Arc<dyn RateLimitingService> = Arc::new(service);
    let result = composite.process_backlog_tasks(Uuid::new_v4()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}

#[tokio::test]
async fn test_composite_trait_dispatches_check_and_deduct_quota() {
    let team_id = Uuid::new_v4();
    let task_repo = Arc::new(MockTaskRepository::new());
    let backlog_repo = Arc::new(MockTasksBacklogRepository::new());
    let credits_repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));

    let service = make_service(
        task_repo,
        backlog_repo,
        credits_repo,
        disabled_rate_config(),
    )
    .await;
    let composite: Arc<dyn RateLimitingService> = Arc::new(service);

    let result = composite
        .check_and_deduct_quota(
            team_id,
            20,
            CreditsTransactionType::Scrape,
            "via composite".to_string(),
            None,
        )
        .await;
    assert!(result.is_ok());

    let balance = composite.get_quota_balance(team_id).await.unwrap();
    assert_eq!(balance, 80);
}
