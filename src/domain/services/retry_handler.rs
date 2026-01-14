// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Retry Handler Service
//!
//! Provides unified task failure handling with retry logic.
//! Consolidates retry patterns from scrape_worker, webhook_worker, and rate_limiting_service.

use crate::domain::models::task::{Task, TaskStatus};
use crate::domain::repositories::task_repository::TaskRepository;
use crate::utils::retry_policy::RetryPolicy;
use chrono::{Duration, Utc};
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

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
pub struct RetryHandler<R: TaskRepository> {
    repository: Arc<R>,
    retry_policy: RetryPolicy,
}

impl<R: TaskRepository> RetryHandler<R> {
    /// Create a new RetryHandler
    pub fn new(repository: Arc<R>, retry_policy: RetryPolicy) -> Self {
        Self {
            repository,
            retry_policy,
        }
    }

    /// Create a RetryHandler with default policy
    pub fn with_default_policy(repository: Arc<R>) -> Self {
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
        let new_attempt_count = task.attempt_count + 1;

        if !self.retry_policy.should_retry(new_attempt_count) {
            warn!(
                "Task {} exceeded max retries ({}), marking as failed",
                task.id, task.max_retries
            );

            if let Err(e) = self.repository.mark_failed(task.id).await {
                return HandleFailureResult::Error(e);
            }

            return HandleFailureResult::Failed;
        }

        // Calculate backoff and schedule next retry
        let backoff = self.retry_policy.calculate_backoff(new_attempt_count);
        let next_retry = Utc::now() + Duration::milliseconds(backoff.as_millis() as i64);

        task.attempt_count = new_attempt_count;
        task.scheduled_at = Some(next_retry.into());
        task.status = TaskStatus::Queued;

        if let Err(e) = self.repository.update(task).await {
            return HandleFailureResult::Error(e);
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

    /// Handle task failure with custom team_id (for tasks without team_id field)
    pub async fn handle_failure_by_id(
        &self,
        task_id: Uuid,
        team_id: Uuid,
        max_retries: u32,
    ) -> HandleFailureResult {
        let new_attempt_count = 1; // First attempt for new failure tracking

        if !self.retry_policy.should_retry(new_attempt_count) {
            if let Err(e) = self.repository.mark_failed(task_id).await {
                return HandleFailureResult::Error(e);
            }
            return HandleFailureResult::Failed;
        }

        let backoff = self.retry_policy.calculate_backoff(new_attempt_count);
        let next_retry = Utc::now() + Duration::milliseconds(backoff.as_millis() as i64);

        if let Err(e) = self
            .repository
            .update_status_and_schedule(task_id, TaskStatus::Queued, Some(next_retry.into()))
            .await
        {
            return HandleFailureResult::Error(e);
        }

        HandleFailureResult::Retried {
            attempt_count: new_attempt_count,
            next_retry_at: next_retry,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::task::TaskStatus;
    use async_trait::async_trait;
    use uuid::Uuid;

    struct MockTaskRepository {
        updated_tasks: Arc<std::sync::Mutex<Vec<Task>>>,
        failed_tasks: Arc<std::sync::Mutex<Vec<Uuid>>>,
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, anyhow::Error> {
            unimplemented!()
        }

        async fn create(&self, _task: &Task) -> Result<Task, anyhow::Error> {
            unimplemented!()
        }

        async fn update(&self, task: &Task) -> Result<(), anyhow::Error> {
            self.updated_tasks.lock().unwrap().push(task.clone());
            Ok(())
        }

        async fn mark_failed(&self, task_id: Uuid) -> Result<(), anyhow::Error> {
            self.failed_tasks.lock().unwrap().push(task_id);
            Ok(())
        }

        async fn find_pending(&self, _limit: i32) -> Result<Vec<Task>, anyhow::Error> {
            unimplemented!()
        }

        async fn find_by_status(
            &self,
            _status: TaskStatus,
            _limit: i32,
        ) -> Result<Vec<Task>, anyhow::Error> {
            unimplemented!()
        }

        async fn find_by_team(
            &self,
            _team_id: Uuid,
            _limit: i32,
        ) -> Result<Vec<Task>, anyhow::Error> {
            unimplemented!()
        }

        async fn count_by_status(&self, _status: TaskStatus) -> Result<i64, anyhow::Error> {
            unimplemented!()
        }

        async fn update_status(&self, _id: Uuid, _status: TaskStatus) -> Result<(), anyhow::Error> {
            unimplemented!()
        }

        async fn update_status_and_schedule(
            &self,
            _id: Uuid,
            _status: TaskStatus,
            _scheduled_at: Option<chrono::DateTime<Utc>>,
        ) -> Result<(), anyhow::Error> {
            Ok(())
        }

        async fn delete(&self, _id: Uuid) -> Result<(), anyhow::Error> {
            unimplemented!()
        }

        async fn bulk_update_status(
            &self,
            _ids: &[Uuid],
            _status: TaskStatus,
        ) -> Result<u64, anyhow::Error> {
            unimplemented!()
        }
    }

    fn create_test_task() -> Task {
        Task {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "http://example.com".to_string(),
            task_type: crate::domain::models::task::TaskType::Scrape,
            status: TaskStatus::Failed,
            payload: serde_json::json!({}),
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_retry_handler_retry() {
        let updated_tasks = Arc::new(std::sync::Mutex::new(Vec::new()));
        let failed_tasks = Arc::new(std::sync::Mutex::new(Vec::new()));

        let repo = MockTaskRepository {
            updated_tasks,
            failed_tasks,
        };

        let handler = RetryHandler::with_default_policy(Arc::new(repo));
        let mut task = create_test_task();
        task.attempt_count = 0;
        task.max_retries = 3;

        let result = handler.handle_failure(&mut task).await;

        match result {
            HandleFailureResult::Retried { attempt_count, .. } => {
                assert_eq!(attempt_count, 1);
            }
            _ => panic!("Expected Retried result"),
        }
    }

    #[tokio::test]
    async fn test_retry_handler_max_exceeded() {
        let updated_tasks = Arc::new(std::sync::Mutex::new(Vec::new()));
        let failed_tasks = Arc::new(std::sync::Mutex::new(Vec::new()));

        let repo = MockTaskRepository {
            updated_tasks,
            failed_tasks,
        };

        let handler = RetryHandler::with_default_policy(Arc::new(repo));
        let mut task = create_test_task();
        task.attempt_count = 5; // Exceeds max_retries
        task.max_retries = 3;

        let result = handler.handle_failure(&mut task).await;

        match result {
            HandleFailureResult::Failed => {
                // Expected
            }
            _ => panic!("Expected Failed result"),
        }
    }
}
