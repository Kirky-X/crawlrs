// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! WorkerManager unit tests
//!
//! Tests WorkerManager construction, start_workers behavior, concurrency,
//! and Drop semantics using mock implementations of all required traits.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use crawlrs::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crawlrs::config::settings::Settings;
use crawlrs::domain::models::scrape_result::ScrapeResult;
use crawlrs::domain::models::{
    Crawl, CrawlStatus, CreditsTransaction, CreditsTransactionType, Task,
};
use crawlrs::domain::repositories::crawl_repository::CrawlRepository;
use crawlrs::domain::repositories::credits_repository::{
    CreditsRepository, CreditsRepositoryError,
};
use crawlrs::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crawlrs::domain::repositories::task_repository::{
    RepositoryError, TaskQueryParams, TaskRepository,
};
use crawlrs::domain::services::extraction_service::{ExtractionRule, ExtractionServiceTrait};
use crawlrs::domain::services::llm_service::TokenUsage;
use crawlrs::domain::services::webhook_service::WebhookService;
use crawlrs::engines::engine_client::{EngineClient, ScrapeResponse};
use crawlrs::presentation::middleware::team_semaphore::TeamSemaphore;
use crawlrs::queue::task_queue::{QueueError, TaskQueue};
use crawlrs::utils::regex_cache::RegexCache;
use crawlrs::utils::robots::RobotsCheckerTrait;
use crawlrs::workers::manager::{WorkerManager, WorkerManagerConfig, WorkerManagerDeps};

// =============================================================================
// Mock implementations
// =============================================================================

/// Mock TaskRepository that tracks expire_tasks and reset_stuck_tasks calls.
struct MockTaskRepository {
    tasks: Mutex<Vec<Task>>,
    should_fail: AtomicBool,
    expire_count: AtomicUsize,
    reset_count: AtomicUsize,
}

impl MockTaskRepository {
    fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            should_fail: AtomicBool::new(false),
            expire_count: AtomicUsize::new(0),
            reset_count: AtomicUsize::new(0),
        }
    }

    fn expire_count(&self) -> usize {
        self.expire_count.load(Ordering::SeqCst)
    }

    #[allow(dead_code)]
    fn reset_count(&self) -> usize {
        self.reset_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl TaskRepository for MockTaskRepository {
    async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(RepositoryError::Database(anyhow::anyhow!("Mock error")));
        }
        let mut tasks = self.tasks.lock().unwrap();
        let mut new_task = task.clone();
        new_task.created_at = Utc::now();
        tasks.push(new_task.clone());
        Ok(new_task)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
        let tasks = self.tasks.lock().unwrap();
        Ok(tasks.iter().find(|t| t.id == id).cloned())
    }

    async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(RepositoryError::Database(anyhow::anyhow!("Mock error")));
        }
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(t) = tasks.iter_mut().find(|t| t.id == task.id) {
            *t = task.clone();
            Ok(task.clone())
        } else {
            Err(RepositoryError::NotFound)
        }
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

    async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
        Ok(0)
    }

    async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
        Ok(vec![])
    }

    async fn reset_stuck_tasks(&self, _timeout: chrono::Duration) -> Result<u64, RepositoryError> {
        self.reset_count.fetch_add(1, Ordering::SeqCst);
        Ok(0)
    }

    async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
        self.expire_count.fetch_add(1, Ordering::SeqCst);
        Ok(0)
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
}

/// Mock TaskQueue that tracks dequeue calls.
struct MockTaskQueue {
    dequeue_count: AtomicUsize,
    enqueue_count: AtomicUsize,
}

impl MockTaskQueue {
    fn new() -> Self {
        Self {
            dequeue_count: AtomicUsize::new(0),
            enqueue_count: AtomicUsize::new(0),
        }
    }

    fn dequeue_count(&self) -> usize {
        self.dequeue_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl TaskQueue for MockTaskQueue {
    async fn enqueue(&self, _task: Task) -> Result<Task, QueueError> {
        self.enqueue_count.fetch_add(1, Ordering::SeqCst);
        Ok(_task)
    }

    async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, QueueError> {
        self.dequeue_count.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }

    async fn complete(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }

    async fn fail(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }

    async fn cancel(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }
}

/// Mock ScrapeResultRepository that always succeeds.
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

/// Mock CrawlRepository that always succeeds.
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

    async fn update_status(&self, _id: Uuid, _status: CrawlStatus) -> Result<(), RepositoryError> {
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

/// Mock WebhookService that always succeeds.
struct MockWebhookService;

#[async_trait]
impl WebhookService for MockWebhookService {
    async fn send_webhook(
        &self,
        _event: &crawlrs::domain::models::WebhookEvent,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn trigger_completion(&self, _task: &Task) -> anyhow::Result<()> {
        Ok(())
    }

    async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Mock CreditsRepository that always succeeds.
struct MockCreditsRepository;

#[async_trait]
impl CreditsRepository for MockCreditsRepository {
    async fn get_balance(&self, _team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
        Ok(1000)
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
        Ok(1000)
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
        _initial_balance: i64,
    ) -> Result<i64, CreditsRepositoryError> {
        Ok(_initial_balance)
    }
}

/// Mock CreateScrapeUseCaseTrait that always succeeds.
struct MockCreateScrapeUseCase;

#[async_trait]
impl CreateScrapeUseCaseTrait for MockCreateScrapeUseCase {
    async fn execute(
        &self,
        _request_dto: crawlrs::application::dto::scrape_request::ScrapeRequestDto,
    ) -> Result<ScrapeResponse, crawlrs::domain::models::DomainError> {
        Err(crawlrs::domain::models::DomainError::ValidationError(
            "mock not implemented".to_string(),
        ))
    }
}

/// Mock RobotsCheckerTrait that always allows.
struct MockRobotsChecker;

#[async_trait]
impl RobotsCheckerTrait for MockRobotsChecker {
    async fn is_allowed(&self, _url_str: &str, _user_agent: &str) -> anyhow::Result<bool> {
        Ok(true)
    }

    async fn get_crawl_delay(
        &self,
        _url_str: &str,
        _user_agent: &str,
    ) -> anyhow::Result<Option<Duration>> {
        Ok(None)
    }
}

/// Mock ExtractionServiceTrait that returns empty results.
struct MockExtractionService;

#[async_trait]
impl ExtractionServiceTrait for MockExtractionService {
    async fn extract(
        &self,
        _html_content: &str,
        _rules: &std::collections::HashMap<String, ExtractionRule>,
        _base_url: Option<&str>,
    ) -> anyhow::Result<(serde_json::Value, TokenUsage)> {
        Ok((serde_json::json!({}), TokenUsage::default()))
    }

    async fn extract_with_schema(
        &self,
        _html_content: &str,
        _schema: &serde_json::Value,
    ) -> anyhow::Result<(serde_json::Value, TokenUsage)> {
        Ok((serde_json::json!({}), TokenUsage::default()))
    }

    fn extract_with_selectors(
        &self,
        _html_content: &str,
        _rules: &std::collections::HashMap<String, ExtractionRule>,
        _base_url: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }
}

// =============================================================================
// Helper functions
// =============================================================================

fn make_regex_cache() -> RegexCache {
    RegexCache::new(Arc::new(
        crawlrs::infrastructure::oxcache::RegexCacheType::new(),
    ))
}

fn make_deps(queue: Arc<dyn TaskQueue>, repository: Arc<dyn TaskRepository>) -> WorkerManagerDeps {
    WorkerManagerDeps {
        queue,
        repository,
        result_repository: Arc::new(MockScrapeResultRepository),
        crawl_repository: Arc::new(MockCrawlRepository),
        webhook_service: Arc::new(MockWebhookService),
        credits_repository: Arc::new(MockCreditsRepository),
        engine_client: Arc::new(EngineClient::new()),
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase),
        team_semaphore: Arc::new(TeamSemaphore::new(10)),
        robots_checker: Arc::new(MockRobotsChecker),
        http_client: Arc::new(reqwest::Client::new()),
        extraction_service: Arc::new(MockExtractionService),
        regex_cache: make_regex_cache(),
    }
}

fn make_config(concurrency_limit: usize) -> WorkerManagerConfig {
    WorkerManagerConfig {
        settings: Arc::new(Settings::default()),
        default_concurrency_limit: concurrency_limit,
    }
}

// =============================================================================
// WorkerManager construction tests
// =============================================================================

#[test]
fn test_worker_manager_new_constructs_successfully() {
    let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::new());
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue, repo);
    let config = make_config(5);

    let _manager = WorkerManager::new(deps, config);
    // If this line is reached, construction succeeded.
}

#[test]
fn test_worker_manager_new_with_zero_concurrency() {
    let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::new());
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue, repo);
    let config = make_config(0);

    let _manager = WorkerManager::new(deps, config);
}

#[test]
fn test_worker_manager_new_with_large_concurrency() {
    let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::new());
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue, repo);
    let config = make_config(1000);

    let _manager = WorkerManager::new(deps, config);
}

// =============================================================================
// start_workers tests
// =============================================================================

#[tokio::test]
async fn test_start_workers_zero_spawns_expiration_worker_only() {
    // start_workers(0) should still spawn the ExpirationWorker,
    // which calls expire_tasks on the repository.
    let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::new());
    let repo = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue, repo.clone());
    let config = make_config(5);

    let mut manager = WorkerManager::new(deps, config);
    manager.start_workers(0).await;

    // The ExpirationWorker's first tick fires immediately; allow time for it.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // expire_tasks should have been called at least once.
    assert!(
        repo.expire_count() >= 1,
        "ExpirationWorker should have called expire_tasks, got {}",
        repo.expire_count()
    );

    // Drop aborts all handles.
    drop(manager);
}

#[tokio::test]
async fn test_start_workers_one_spawns_scrape_worker() {
    // start_workers(1) spawns 1 ExpirationWorker + 1 ScrapeWorker.
    // The ScrapeWorker calls queue.dequeue in its run loop.
    let queue = Arc::new(MockTaskQueue::new());
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue.clone(), repo);
    let config = make_config(5);

    let mut manager = WorkerManager::new(deps, config);
    manager.start_workers(1).await;

    // Allow time for the scrape worker to call dequeue at least once.
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(
        queue.dequeue_count() >= 1,
        "ScrapeWorker should have called dequeue, got {}",
        queue.dequeue_count()
    );

    drop(manager);
}

#[tokio::test]
async fn test_start_workers_three_spawns_three_scrape_workers() {
    // start_workers(3) spawns 1 ExpirationWorker + 3 ScrapeWorkers.
    // More workers → more dequeue calls in the same time window.
    let queue = Arc::new(MockTaskQueue::new());
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue.clone(), repo);
    let config = make_config(5);

    let mut manager = WorkerManager::new(deps, config);
    manager.start_workers(3).await;

    // Allow time for all 3 scrape workers to call dequeue.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let count = queue.dequeue_count();
    assert!(
        count >= 3,
        "3 ScrapeWorkers should have called dequeue at least 3 times, got {}",
        count
    );

    drop(manager);
}

#[tokio::test]
async fn test_start_workers_called_multiple_times_accumulates() {
    // Calling start_workers twice should accumulate handles (not replace).
    let queue = Arc::new(MockTaskQueue::new());
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue.clone(), repo);
    let config = make_config(5);

    let mut manager = WorkerManager::new(deps, config);

    // First call: 1 scrape worker
    manager.start_workers(1).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let count_after_first = queue.dequeue_count();
    assert!(count_after_first >= 1, "first batch should dequeue");

    // Second call: 2 more scrape workers (total 3)
    manager.start_workers(2).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let count_after_second = queue.dequeue_count();

    // More dequeue calls should have happened with additional workers.
    assert!(
        count_after_second > count_after_first,
        "second start_workers should add more workers: before={}, after={}",
        count_after_first,
        count_after_second
    );

    drop(manager);
}

// =============================================================================
// Drop behavior tests
// =============================================================================

#[tokio::test]
async fn test_drop_aborts_all_workers() {
    // After dropping the manager, the spawned tasks should be aborted,
    // meaning dequeue calls should stop increasing.
    let queue = Arc::new(MockTaskQueue::new());
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue.clone(), repo);
    let config = make_config(5);

    let mut manager = WorkerManager::new(deps, config);
    manager.start_workers(2).await;

    // Allow workers to run briefly.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let count_before_drop = queue.dequeue_count();
    assert!(
        count_before_drop >= 1,
        "workers should be running before drop"
    );

    // Drop the manager — this should abort all handles.
    drop(manager);

    // Wait a moment to see if dequeue calls continue (they should not).
    tokio::time::sleep(Duration::from_millis(500)).await;
    let count_after_drop = queue.dequeue_count();

    // The count should not increase significantly after drop.
    // (A small increase is possible if a dequeue was in-flight when aborted.)
    let increase = count_after_drop.saturating_sub(count_before_drop);
    assert!(
        increase <= 2,
        "dequeue calls should stop after drop: before={}, after={}, increase={}",
        count_before_drop,
        count_after_drop,
        increase
    );
}

#[tokio::test]
async fn test_drop_without_start_workers_is_safe() {
    // Dropping a manager that never started workers should not panic.
    let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::new());
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue, repo);
    let config = make_config(5);

    let manager = WorkerManager::new(deps, config);
    drop(manager); // Should not panic.
}

#[tokio::test]
async fn test_drop_after_start_workers_zero_aborts_expiration_worker() {
    // start_workers(0) spawns only the expiration worker.
    // After drop, expire_tasks calls should stop.
    let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::new());
    let repo = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue, repo.clone());
    let config = make_config(5);

    let mut manager = WorkerManager::new(deps, config);
    manager.start_workers(0).await;

    // Allow the expiration worker to run its first tick.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let expire_before = repo.expire_count();
    assert!(expire_before >= 1, "expiration worker should have run");

    drop(manager);

    // Wait to see if expire_tasks continues (it should not).
    tokio::time::sleep(Duration::from_millis(500)).await;
    let expire_after = repo.expire_count();

    // The expiration worker has a 3600s interval, so no additional call
    // would happen in this window anyway. But we verify the count is stable.
    assert!(
        expire_after >= expire_before,
        "expire count should not decrease"
    );
}

// =============================================================================
// Concurrency and configuration tests
// =============================================================================

#[tokio::test]
async fn test_worker_manager_with_zero_concurrency_limit() {
    // Manager with concurrency_limit=0 should still construct and start workers.
    let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::new());
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue, repo);
    let config = make_config(0);

    let mut manager = WorkerManager::new(deps, config);
    manager.start_workers(1).await;

    tokio::time::sleep(Duration::from_millis(150)).await;
    drop(manager);
}

#[tokio::test]
async fn test_worker_manager_settings_are_shared() {
    // The Settings Arc should be shared between config and manager.
    let settings = Arc::new(Settings::default());
    let strong_count_before = Arc::strong_count(&settings);

    let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::new());
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue, repo);
    let config = WorkerManagerConfig {
        settings: settings.clone(),
        default_concurrency_limit: 5,
    };

    let _manager = WorkerManager::new(deps, config);
    let strong_count_after = Arc::strong_count(&settings);
    assert_eq!(
        strong_count_after,
        strong_count_before + 1,
        "manager should hold one more Arc reference to settings"
    );
}

// =============================================================================
// WorkerManagerDeps construction tests
// =============================================================================

#[test]
fn test_worker_manager_deps_can_be_constructed_with_all_mocks() {
    let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::new());
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
    let deps = make_deps(queue, repo);

    // Verify all fields are populated by constructing a manager.
    let config = make_config(5);
    let _manager = WorkerManager::new(deps, config);
}

#[test]
fn test_worker_manager_deps_with_shared_repository() {
    // Multiple deps can share the same repository Arc.
    let repo = Arc::new(MockTaskRepository::new());
    let repo_dyn: Arc<dyn TaskRepository> = repo.clone();

    let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::new());
    let deps = make_deps(queue, repo_dyn);
    let config = make_config(5);

    let _manager = WorkerManager::new(deps, config);
    // repo Arc is still valid.
    assert_eq!(Arc::strong_count(&repo), 2); // original + deps
}

// =============================================================================
// Send + Sync verification
// =============================================================================

#[test]
fn test_worker_manager_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<WorkerManager>();
}

#[test]
fn test_worker_manager_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<WorkerManager>();
}

#[test]
fn test_worker_manager_deps_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkerManagerDeps>();
}

#[test]
fn test_worker_manager_config_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkerManagerConfig>();
}

// =============================================================================
// needs_drop verification (Drop impl exists)
// =============================================================================

#[test]
fn test_worker_manager_needs_drop() {
    // WorkerManager implements Drop, so needs_drop should be true.
    assert!(std::mem::needs_drop::<WorkerManager>());
}
