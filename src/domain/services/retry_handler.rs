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
use tracing::{info, warn};

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
            task.status = "failed".to_string();
            task.completed_at = Some(Utc::now().naive_utc());

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
        task.scheduled_at = Some(next_retry.naive_utc());
        task.status = "queued".to_string();
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
