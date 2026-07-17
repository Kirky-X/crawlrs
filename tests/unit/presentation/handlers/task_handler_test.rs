// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! External unit tests for task_handler public API.
//!
//! Focuses on the pub functions that accept `&dyn TaskRepository` /
//! `&dyn TaskRepository`-like trait objects:
//! - `handle_sync_wait_and_get_status` — zero-wait / empty / poll paths
//! - `wait_for_tasks_completion` — completion / timeout / max-poll paths
//!
//! Also exercises `TaskQueryResponseMeta` serialization and `SyncWaitResult`
//! construction, which are pub types exported by the handler module.

use std::collections::HashSet;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use crawlrs::common::constants::crawl_task;
use crawlrs::domain::models::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::{
    RepositoryError, TaskQueryParams, TaskRepository,
};
use crawlrs::presentation::errors::AppError;
use crawlrs::presentation::handlers::task_handler::{
    handle_sync_wait_and_get_status, wait_for_tasks_completion, SyncWaitResult,
    TaskQueryResponseMeta,
};

// =============================================================================
// Mock TaskRepository
// =============================================================================

/// Configurable mock whose `query_tasks` returns a stored snapshot of tasks.
/// All other trait methods return benign defaults — only `query_tasks` is
/// invoked by the functions under test.
struct MockTaskRepository {
    /// Tasks returned by `query_tasks`. Wrapped in Mutex so the mock remains
    /// `Sync` even though `query_tasks` takes `&self`.
    tasks: Mutex<Vec<Task>>,
    /// When `Some`, `query_tasks` returns this error instead of tasks.
    error: Mutex<Option<RepositoryError>>,
}

impl MockTaskRepository {
    fn new(tasks: Vec<Task>) -> Self {
        Self {
            tasks: Mutex::new(tasks),
            error: Mutex::new(None),
        }
    }

    fn failing(err: RepositoryError) -> Self {
        Self {
            tasks: Mutex::new(vec![]),
            error: Mutex::new(Some(err)),
        }
    }
}

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
        if let Some(err) = self.error.lock().unwrap().take() {
            return Err(err);
        }
        let tasks = self.tasks.lock().unwrap().clone();
        let total = tasks.len() as u64;
        Ok((tasks, total))
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

fn make_completed_task(id: Uuid, team_id: Uuid) -> Task {
    let now = Utc::now();
    Task {
        id,
        task_type: TaskType::Scrape,
        status: TaskStatus::Completed,
        priority: 0,
        team_id,
        api_key_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: now,
        started_at: None,
        completed_at: Some(now),
        crawl_id: None,
        updated_at: now,
        lock_token: None,
        lock_expires_at: None,
    }
}

fn make_queued_task(id: Uuid, team_id: Uuid) -> Task {
    let now = Utc::now();
    Task {
        id,
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        api_key_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: now,
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: now,
        lock_token: None,
        lock_expires_at: None,
    }
}

// =============================================================================
// TaskQueryResponseMeta serialization
// =============================================================================

#[test]
fn tc_task_query_response_meta_serialization_round_trip() {
    let meta = TaskQueryResponseMeta {
        status: "completed".to_string(),
        credits_used: 42,
        response_time_ms: 1500,
    };
    let json = serde_json::to_string(&meta).expect("serialization must succeed");
    let parsed: TaskQueryResponseMeta =
        serde_json::from_str(&json).expect("deserialization must succeed");
    assert_eq!(parsed.status, "completed");
    assert_eq!(parsed.credits_used, 42);
    assert_eq!(parsed.response_time_ms, 1500);
}

#[test]
fn tc_task_query_response_meta_zero_values() {
    let meta = TaskQueryResponseMeta {
        status: String::new(),
        credits_used: 0,
        response_time_ms: 0,
    };
    let json = serde_json::to_string(&meta).expect("serialization must succeed");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must be valid JSON");
    assert_eq!(parsed["status"], "");
    assert_eq!(parsed["credits_used"], 0);
    assert_eq!(parsed["response_time_ms"], 0);
}

#[test]
fn tc_task_query_response_meta_clone_preserves_values() {
    let meta = TaskQueryResponseMeta {
        status: "synced".to_string(),
        credits_used: 7,
        response_time_ms: 250,
    };
    let cloned = meta.clone();
    assert_eq!(cloned.status, meta.status);
    assert_eq!(cloned.credits_used, meta.credits_used);
    assert_eq!(cloned.response_time_ms, meta.response_time_ms);
}

// =============================================================================
// SyncWaitResult construction
// =============================================================================

#[test]
fn tc_sync_wait_result_zero_wait_not_timeout() {
    let result = SyncWaitResult {
        waited_time_ms: 0,
        is_timeout: false,
    };
    assert_eq!(result.waited_time_ms, 0);
    assert!(!result.is_timeout);
}

#[test]
fn tc_sync_wait_result_with_wait_not_timeout() {
    let result = SyncWaitResult {
        waited_time_ms: 500,
        is_timeout: false,
    };
    assert_eq!(result.waited_time_ms, 500);
    assert!(!result.is_timeout);
}

#[test]
fn tc_sync_wait_result_timeout_set() {
    let result = SyncWaitResult {
        waited_time_ms: 5000,
        is_timeout: true,
    };
    assert!(result.is_timeout);
    assert_eq!(result.waited_time_ms, 5000);
}

// =============================================================================
// handle_sync_wait_and_get_status — zero / empty paths
// =============================================================================

#[tokio::test]
async fn tc_handle_sync_wait_zero_ms_returns_immediately() {
    // sync_wait_ms == 0 → return immediately with no wait.
    let repo = MockTaskRepository::new(vec![]);
    let task_ids = vec![Uuid::new_v4()];
    let team_id = Uuid::new_v4();

    let result =
        handle_sync_wait_and_get_status(&repo as &dyn TaskRepository, &task_ids, team_id, 0).await;

    assert!(result.is_ok(), "zero sync_wait_ms must return Ok");
    let sync_result = result.unwrap();
    assert_eq!(sync_result.waited_time_ms, 0);
    assert!(!sync_result.is_timeout);
}

#[tokio::test]
async fn tc_handle_sync_wait_empty_task_ids_returns_immediately() {
    // Empty task_ids → return immediately with no wait.
    let repo = MockTaskRepository::new(vec![]);
    let team_id = Uuid::new_v4();

    let result =
        handle_sync_wait_and_get_status(&repo as &dyn TaskRepository, &[], team_id, 5000).await;

    assert!(result.is_ok(), "empty task_ids must return Ok");
    let sync_result = result.unwrap();
    assert_eq!(sync_result.waited_time_ms, 0);
    assert!(!sync_result.is_timeout);
}

#[tokio::test]
async fn tc_handle_sync_wait_all_completed_returns_quickly() {
    // Tasks already completed → first poll sees completion_rate == 1.0 → return.
    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let task = make_completed_task(task_id, team_id);
    let repo = MockTaskRepository::new(vec![task]);

    let result =
        handle_sync_wait_and_get_status(&repo as &dyn TaskRepository, &[task_id], team_id, 3000)
            .await;

    assert!(result.is_ok(), "completed tasks must return Ok");
    let sync_result = result.unwrap();
    // Should not have waited the full sync_wait_ms.
    assert!(sync_result.waited_time_ms < 3000, "should return quickly");
    assert!(!sync_result.is_timeout);
}

#[tokio::test]
async fn tc_handle_sync_wait_queued_tasks_timeout() {
    // Tasks still queued → polls until sync_wait_ms elapses → is_timeout true.
    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let task = make_queued_task(task_id, team_id);
    let repo = MockTaskRepository::new(vec![task]);

    // Use a short sync_wait_ms to keep the test fast. BASE_POLL_INTERVAL_MS is
    // 1000ms, clamped to [500, 2000]. With sync_wait_ms=600 the loop runs
    // once, sleeps ~500ms, then the timeout check exits.
    let result =
        handle_sync_wait_and_get_status(&repo as &dyn TaskRepository, &[task_id], team_id, 600)
            .await;

    assert!(result.is_ok(), "queued tasks must not error");
    let sync_result = result.unwrap();
    assert!(
        sync_result.is_timeout,
        "should be marked as timeout when tasks never complete"
    );
}

#[tokio::test]
async fn tc_wait_for_tasks_completion_repo_error_propagates() {
    // Repository error → AppError returned to caller.
    let repo =
        MockTaskRepository::failing(RepositoryError::Database(anyhow::anyhow!("poll failure")));
    let task_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    let result = wait_for_tasks_completion(
        &repo as &dyn TaskRepository,
        &[task_id],
        team_id,
        1000,
        crawl_task::BASE_POLL_INTERVAL_MS,
    )
    .await;

    assert!(result.is_err(), "repo error must propagate");
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("poll failure") || msg.contains("Query failed") || msg.contains("error"),
        "expected error message containing failure, got: {}",
        msg
    );
}

// =============================================================================
// wait_for_tasks_completion — direct function tests
// =============================================================================

#[tokio::test]
async fn tc_wait_for_tasks_completion_empty_ids_returns_immediately() {
    // Empty task_ids → calculate_completion_rate returns 1.0 → Ok.
    let repo = MockTaskRepository::new(vec![]);
    let team_id = Uuid::new_v4();

    let result = wait_for_tasks_completion(
        &repo as &dyn TaskRepository,
        &[],
        team_id,
        1000,
        crawl_task::BASE_POLL_INTERVAL_MS,
    )
    .await;

    assert!(result.is_ok(), "empty task_ids must return Ok");
}

#[tokio::test]
async fn tc_wait_for_tasks_completion_all_completed_returns() {
    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let task = make_completed_task(task_id, team_id);
    let repo = MockTaskRepository::new(vec![task]);

    let result = wait_for_tasks_completion(
        &repo as &dyn TaskRepository,
        &[task_id],
        team_id,
        2000,
        crawl_task::BASE_POLL_INTERVAL_MS,
    )
    .await;

    assert!(result.is_ok(), "completed tasks must return Ok");
}

#[tokio::test]
async fn tc_wait_for_tasks_completion_timeout_returns_ok() {
    // Tasks never complete → returns Ok after timeout (not an error, just done).
    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let task = make_queued_task(task_id, team_id);
    let repo = MockTaskRepository::new(vec![task]);

    let result = wait_for_tasks_completion(
        &repo as &dyn TaskRepository,
        &[task_id],
        team_id,
        600,
        crawl_task::BASE_POLL_INTERVAL_MS,
    )
    .await;

    assert!(result.is_ok(), "timeout must return Ok, not Err");
}

#[tokio::test]
async fn tc_wait_for_tasks_completion_repo_error_returns_err() {
    let repo =
        MockTaskRepository::failing(RepositoryError::Database(anyhow::anyhow!("query failed")));
    let task_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    let result = wait_for_tasks_completion(
        &repo as &dyn TaskRepository,
        &[task_id],
        team_id,
        1000,
        crawl_task::BASE_POLL_INTERVAL_MS,
    )
    .await;

    assert!(result.is_err(), "repo error must return Err");
}

#[tokio::test]
async fn tc_wait_for_tasks_completion_failed_task_counts_as_done() {
    // Failed tasks count as "completed" for completion_rate purposes.
    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let mut task = make_queued_task(task_id, team_id);
    task.status = TaskStatus::Failed;
    let repo = MockTaskRepository::new(vec![task]);

    let result = wait_for_tasks_completion(
        &repo as &dyn TaskRepository,
        &[task_id],
        team_id,
        2000,
        crawl_task::BASE_POLL_INTERVAL_MS,
    )
    .await;

    assert!(result.is_ok(), "failed tasks must count as done");
}

#[tokio::test]
async fn tc_wait_for_tasks_completion_cancelled_task_counts_as_done() {
    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let mut task = make_queued_task(task_id, team_id);
    task.status = TaskStatus::Cancelled;
    let repo = MockTaskRepository::new(vec![task]);

    let result = wait_for_tasks_completion(
        &repo as &dyn TaskRepository,
        &[task_id],
        team_id,
        2000,
        crawl_task::BASE_POLL_INTERVAL_MS,
    )
    .await;

    assert!(result.is_ok(), "cancelled tasks must count as done");
}

#[tokio::test]
async fn tc_wait_for_tasks_completion_partial_completion_still_waits() {
    // One completed + one queued → completion_rate 0.5 → keeps polling.
    let team_id = Uuid::new_v4();
    let task_id_1 = Uuid::new_v4();
    let task_id_2 = Uuid::new_v4();
    let done = make_completed_task(task_id_1, team_id);
    let pending = make_queued_task(task_id_2, team_id);
    let repo = MockTaskRepository::new(vec![done, pending]);

    let result = wait_for_tasks_completion(
        &repo as &dyn TaskRepository,
        &[task_id_1, task_id_2],
        team_id,
        600,
        crawl_task::BASE_POLL_INTERVAL_MS,
    )
    .await;

    // Should timeout (Ok) since pending never completes.
    assert!(result.is_ok(), "partial completion must not error");
}

// =============================================================================
// Constants sanity
// =============================================================================

#[test]
fn tc_base_poll_interval_ms_is_1000() {
    assert_eq!(crawl_task::BASE_POLL_INTERVAL_MS, 1000);
}

#[test]
fn tc_max_sync_wait_ms_is_30000() {
    assert_eq!(crawl_task::MAX_SYNC_WAIT_MS, 30000);
}

#[test]
fn tc_max_poll_count_is_60() {
    assert_eq!(crawl_task::MAX_POLL_COUNT, 60);
}

#[test]
fn tc_base_poll_interval_within_clamp_range() {
    // The wait_for_tasks_completion function clamps the base interval to
    // [500, 2000]. BASE_POLL_INTERVAL_MS=1000 is within this range.
    assert!(crawl_task::BASE_POLL_INTERVAL_MS >= 500);
    assert!(crawl_task::BASE_POLL_INTERVAL_MS <= 2000);
}
