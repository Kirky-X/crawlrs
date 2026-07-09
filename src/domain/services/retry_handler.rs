// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Retry Handler Service
//!
//! Provides unified task failure handling with retry logic.
//! Consolidates retry patterns from scrape_worker, webhook_worker, and rate_limiting_service.

use crate::domain::models::{Task, TaskStatus};
use crate::domain::repositories::task_repository::TaskRepository;
use crate::utils::retry_policy::RetryPolicy;
use chrono::{Duration, Utc};
use std::sync::Arc;
use log::{info, warn};

/// Result of handling task failure
pub enum HandleFailureResult {
    /// Task was marked for retry
    Retried {
        attempt_count: u32,
        next_retry_at: chrono::DateTime<Utc>,
    },
    /// Task exceeded max retries and was marked as failed
    Failed,
    /// Error occurred during handling
    Error(anyhow::Error),
}

/// Unified handler for task failure with retry logic.
///
/// This handler consolidates the retry pattern that was repeated across
/// multiple workers (scrape_worker, webhook_worker).
pub struct RetryHandler {
    repository: Arc<dyn TaskRepository>,
    retry_policy: RetryPolicy,
}

impl RetryHandler {
    /// Create a new RetryHandler
    pub fn new(repository: Arc<dyn TaskRepository>, retry_policy: RetryPolicy) -> Self {
        Self {
            repository,
            retry_policy,
        }
    }

    /// Create a RetryHandler with default policy
    pub fn with_default_policy(repository: Arc<dyn TaskRepository>) -> Self {
        Self::new(repository, RetryPolicy::default())
    }

    /// Handle task failure with retry logic
    ///
    /// # Arguments
    ///
    /// * `task` - The task that failed
    ///
    /// # Returns
    ///
    /// Result indicating whether task was retried or marked as failed
    pub async fn handle_failure(&self, task: &mut Task) -> HandleFailureResult {
        let new_attempt_count = (task.attempt_count + 1) as u32;

        if !self.retry_policy.should_retry(new_attempt_count) {
            warn!(
                "Task {} exceeded max retries ({}), marking as failed",
                task.id, task.max_retries
            );

            task.attempt_count = new_attempt_count as i32;
            task.retry_count += 1;
            task.status = TaskStatus::Failed;
            task.completed_at = Some(Utc::now());

            if let Err(e) = self.repository.update(task).await {
                return HandleFailureResult::Error(e.into());
            }

            return HandleFailureResult::Failed;
        }

        // Calculate backoff and schedule next retry
        let backoff = self.retry_policy.calculate_backoff(new_attempt_count);
        let next_retry = Utc::now() + Duration::milliseconds(backoff.as_millis() as i64);

        task.attempt_count = new_attempt_count as i32;
        task.retry_count += 1;
        task.scheduled_at = Some(next_retry);
        task.status = TaskStatus::Queued;
        task.started_at = None;
        task.completed_at = None;

        if let Err(e) = self.repository.update(task).await {
            return HandleFailureResult::Error(e.into());
        }

        info!(
            "Scheduled retry {}/{} for task {} in {:?}",
            new_attempt_count, task.max_retries, task.id, backoff
        );

        HandleFailureResult::Retried {
            attempt_count: new_attempt_count,
            next_retry_at: next_retry,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{Task, TaskStatus, TaskType};
    use crate::domain::repositories::task_repository::{
        RepositoryError, TaskQueryParams, TaskRepository,
    };
    use crate::utils::retry_policy::RetryPolicy;
    use async_trait::async_trait;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::Mutex;
    use uuid::Uuid;

    /// Mock TaskRepository that records `update` calls and can simulate failures.
    struct MockTaskRepository {
        /// Last task passed to `update` (None if never called)
        last_updated: Mutex<Option<Task>>,
        /// When true, `update` returns an error
        fail_update: Mutex<bool>,
    }

    impl MockTaskRepository {
        fn new() -> Self {
            Self {
                last_updated: Mutex::new(None),
                fail_update: Mutex::new(false),
            }
        }

        fn set_fail_update(&self, fail: bool) {
            *self.fail_update.lock().expect("lock fail_update") = fail;
        }

        fn last_updated_task(&self) -> Option<Task> {
            self.last_updated.lock().expect("lock last_updated").clone()
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, _task: &Task) -> Result<Task, RepositoryError> {
            Err(RepositoryError::Database(anyhow::anyhow!("not implemented")))
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            if *self.fail_update.lock().expect("lock fail_update") {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock update failure"
                )));
            }
            *self.last_updated.lock().expect("lock last_updated") = Some(task.clone());
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

    /// Build a minimal Task for retry tests
    fn make_task(attempt_count: i32, max_retries: i32) -> Task {
        let mut task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        task.attempt_count = attempt_count;
        task.max_retries = max_retries;
        task
    }

    // ========== RetryHandler::new / with_default_policy tests ==========

    #[test]
    fn test_new_stores_repository_and_policy() {
        let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
        let policy = RetryPolicy::fast();
        let handler = RetryHandler::new(repo.clone(), policy.clone());
        // No direct accessor; verify via behavior in later tests.
        // Just ensure construction does not panic.
        let _ = &handler;
    }

    #[test]
    fn test_with_default_policy_uses_default_retry_policy() {
        let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
        let handler = RetryHandler::with_default_policy(repo.clone());
        let _ = &handler;
    }

    // ========== handle_failure - retry path ==========

    #[tokio::test]
    async fn test_handle_failure_retries_when_below_max() {
        let mock = Arc::new(MockTaskRepository::new());
        let repo: Arc<dyn TaskRepository> = mock.clone();
        // Policy: max_retries=5, no jitter for deterministic timing
        let mut policy = RetryPolicy::default();
        policy.enable_jitter = false;
        let handler = RetryHandler::new(repo, policy);

        let before = Utc::now();
        let mut task = make_task(0, 5);
        let result = handler.handle_failure(&mut task).await;

        match result {
            HandleFailureResult::Retried {
                attempt_count,
                next_retry_at,
            } => {
                assert_eq!(attempt_count, 1, "attempt_count should be incremented to 1");
                assert!(
                    next_retry_at > before,
                    "next_retry_at should be in the future"
                );
            }
            _ => panic!("expected Retried, got different variant"),
        }

        // Task should be mutated correctly
        assert_eq!(task.attempt_count, 1);
        assert_eq!(task.retry_count, 1);
        assert_eq!(task.status, TaskStatus::Queued);
        assert!(task.scheduled_at.is_some(), "scheduled_at should be set");
        assert!(task.started_at.is_none(), "started_at should be cleared");
        assert!(
            task.completed_at.is_none(),
            "completed_at should be cleared"
        );

        // Repository should have received the updated task
        let updated = mock
            .last_updated_task()
            .expect("update should have been called");
        assert_eq!(updated.id, task.id);
        assert_eq!(updated.status, TaskStatus::Queued);
    }

    #[tokio::test]
    async fn test_handle_failure_retries_increments_attempt_count_correctly() {
        let mock = Arc::new(MockTaskRepository::new());
        let repo: Arc<dyn TaskRepository> = mock.clone();
        let mut policy = RetryPolicy::default();
        policy.enable_jitter = false;
        let handler = RetryHandler::new(repo, policy);

        // Start at attempt_count=2, should retry (3 < 5)
        let mut task = make_task(2, 5);
        let result = handler.handle_failure(&mut task).await;

        match result {
            HandleFailureResult::Retried { attempt_count, .. } => {
                assert_eq!(attempt_count, 3, "attempt_count should be 3 after increment");
            }
            _ => panic!("expected Retried, got different variant"),
        }
        assert_eq!(task.attempt_count, 3);
        assert_eq!(task.retry_count, 1);
    }

    // ========== handle_failure - failed path ==========

    #[tokio::test]
    async fn test_handle_failure_fails_when_max_retries_exceeded() {
        let mock = Arc::new(MockTaskRepository::new());
        let repo: Arc<dyn TaskRepository> = mock.clone();
        // Policy max_retries=0 means should_retry(1) = 1 < 0 = false -> immediate fail
        let mut policy = RetryPolicy::default();
        policy.max_retries = 0;
        let handler = RetryHandler::new(repo, policy);

        let before = Utc::now();
        // attempt_count=0 -> new_attempt_count=1, should_retry(1)=false (policy max_retries=0)
        let mut task = make_task(0, 5);
        let result = handler.handle_failure(&mut task).await;

        match result {
            HandleFailureResult::Failed => { /* expected */ }
            _ => panic!("expected Failed, got different variant"),
        }

        // Task should be marked as failed
        assert_eq!(task.attempt_count, 1);
        assert_eq!(task.retry_count, 1);
        assert_eq!(task.status, TaskStatus::Failed);
        assert!(
            task.completed_at.is_some(),
            "completed_at should be set on failure"
        );
        assert!(
            task.completed_at.expect("completed_at set") >= before,
            "completed_at should be ~now"
        );

        // Repository should have received the failed task
        let updated = mock
            .last_updated_task()
            .expect("update should have been called");
        assert_eq!(updated.status, TaskStatus::Failed);
    }

    #[tokio::test]
    async fn test_handle_failure_fails_when_attempt_reaches_max() {
        let mock = Arc::new(MockTaskRepository::new());
        let repo: Arc<dyn TaskRepository> = mock.clone();
        let handler = RetryHandler::new(repo, RetryPolicy::default());

        // Default policy: max_retries=5
        // attempt_count=4 -> new_attempt_count=5, should_retry(5) = 5 < 5 = false -> fail
        let mut task = make_task(4, 5);
        let result = handler.handle_failure(&mut task).await;

        assert!(
            matches!(result, HandleFailureResult::Failed),
            "should fail when attempt_count reaches max_retries"
        );
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(task.attempt_count, 5);
    }

    // ========== handle_failure - error path ==========

    #[tokio::test]
    async fn test_handle_failure_returns_error_when_update_fails_on_retry() {
        let mock = Arc::new(MockTaskRepository::new());
        mock.set_fail_update(true);
        let repo: Arc<dyn TaskRepository> = mock.clone();
        let handler = RetryHandler::new(repo, RetryPolicy::default());

        let mut task = make_task(0, 5);
        let result = handler.handle_failure(&mut task).await;

        match result {
            HandleFailureResult::Error(err) => {
                assert!(
                    err.to_string().contains("mock update failure"),
                    "error should contain mock message: {}",
                    err
                );
            }
            _ => panic!("expected Error, got different variant"),
        }

        // Task should still have been mutated (attempt_count incremented, status set to Queued)
        // even though the repository update failed
        assert_eq!(task.attempt_count, 1);
        assert_eq!(task.retry_count, 1);
        assert_eq!(task.status, TaskStatus::Queued);
    }

    #[tokio::test]
    async fn test_handle_failure_returns_error_when_update_fails_on_fail() {
        let mock = Arc::new(MockTaskRepository::new());
        mock.set_fail_update(true);
        let repo: Arc<dyn TaskRepository> = mock.clone();
        // Policy max_retries=0 -> immediate fail path
        let mut policy = RetryPolicy::default();
        policy.max_retries = 0;
        let handler = RetryHandler::new(repo, policy);

        // attempt_count=0 -> new_attempt_count=1, should_retry(1)=false (policy max_retries=0)
        let mut task = make_task(0, 5);
        let result = handler.handle_failure(&mut task).await;

        match result {
            HandleFailureResult::Error(err) => {
                assert!(
                    err.to_string().contains("mock update failure"),
                    "error should contain mock message: {}",
                    err
                );
            }
            _ => panic!("expected Error, got different variant"),
        }

        // Task should still have been mutated (status set to Failed) before the error
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(task.attempt_count, 1);
        assert!(task.completed_at.is_some());
    }

    // ========== HandleFailureResult variant coverage ==========

    #[test]
    fn test_handle_failure_result_reried_carries_correct_fields() {
        let now = Utc::now();
        let result = HandleFailureResult::Retried {
            attempt_count: 3,
            next_retry_at: now,
        };
        match result {
            HandleFailureResult::Retried {
                attempt_count,
                next_retry_at,
            } => {
                assert_eq!(attempt_count, 3);
                assert_eq!(next_retry_at, now);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_handle_failure_result_failed_is_unit() {
        let result = HandleFailureResult::Failed;
        assert!(matches!(result, HandleFailureResult::Failed));
    }

    #[test]
    fn test_handle_failure_result_error_carries_message() {
        let result = HandleFailureResult::Error(anyhow::anyhow!("boom"));
        match result {
            HandleFailureResult::Error(e) => assert!(e.to_string().contains("boom")),
            _ => panic!("wrong variant"),
        }
    }
}
