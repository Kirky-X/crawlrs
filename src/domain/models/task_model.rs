// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task domain model - pure domain entity without ORM annotations
//!
//! This module contains the pure domain model for Task,
//! following Domain-Driven Design principles.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::task_domain::{TaskStatus, TaskType};

/// Task domain model
///
/// Represents a scraping or crawling task in the system.
/// This is a pure domain model without any ORM annotations,
/// following DDD principles for clean architecture.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    /// Unique identifier for the task
    pub id: Uuid,
    /// Type of task (scrape, crawl, extract)
    pub task_type: TaskType,
    /// Current status of the task
    pub status: TaskStatus,
    /// Priority level (higher = more urgent)
    pub priority: i32,
    /// Team ID for multi-tenancy
    pub team_id: Uuid,
    /// API key ID that created this task
    pub api_key_id: Uuid,
    /// Target URL to scrape/crawl
    pub url: String,
    /// Task payload as JSON (request parameters)
    pub payload: serde_json::Value,
    /// Number of retry attempts made
    pub retry_count: i32,
    /// Total number of attempts (including initial)
    pub attempt_count: i32,
    /// Maximum number of retry attempts allowed
    pub max_retries: i32,
    /// When the task should be executed (scheduled)
    pub scheduled_at: Option<DateTime<Utc>>,
    /// When the task expires
    pub expires_at: Option<DateTime<Utc>>,
    /// When the task was created
    pub created_at: DateTime<Utc>,
    /// When the task started execution
    pub started_at: Option<DateTime<Utc>>,
    /// When the task completed
    pub completed_at: Option<DateTime<Utc>>,
    /// Parent crawl ID if this task is part of a crawl
    pub crawl_id: Option<Uuid>,
    /// When the task was last updated
    pub updated_at: DateTime<Utc>,
    /// Lock token for distributed task acquisition
    pub lock_token: Option<Uuid>,
    /// When the lock expires
    pub lock_expires_at: Option<DateTime<Utc>>,
}

impl Task {
    /// Create a new task with default values
    pub fn new(
        id: Uuid,
        task_type: TaskType,
        team_id: Uuid,
        api_key_id: Uuid,
        url: String,
        payload: serde_json::Value,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            task_type,
            status: TaskStatus::Queued,
            priority: 0,
            team_id,
            api_key_id,
            url,
            payload,
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

    /// Check if the task can be retried
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }

    /// Check if the task is expired
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| Utc::now() > expires_at)
    }

    /// Check if the task is locked
    pub fn is_locked(&self) -> bool {
        self.lock_token.is_some()
            && self
                .lock_expires_at
                .is_some_and(|expires_at| Utc::now() < expires_at)
    }

    /// Mark the task as started
    pub fn start(&mut self) {
        self.status = TaskStatus::Active;
        self.started_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Mark the task as completed
    pub fn complete(&mut self) {
        self.status = TaskStatus::Completed;
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Mark the task as failed
    pub fn fail(&mut self) {
        self.status = TaskStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Mark the task as cancelled
    pub fn cancel(&mut self) {
        self.status = TaskStatus::Cancelled;
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Increment retry count
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
        self.attempt_count += 1;
        self.updated_at = Utc::now();
    }

    /// Acquire lock for this task
    pub fn acquire_lock(&mut self, worker_id: Uuid, lock_duration: chrono::Duration) {
        self.lock_token = Some(worker_id);
        self.lock_expires_at = Some(Utc::now() + lock_duration);
        self.updated_at = Utc::now();
    }

    /// Release lock on this task
    pub fn release_lock(&mut self) {
        self.lock_token = None;
        self.lock_expires_at = None;
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::task_domain::{TaskStatus, TaskType};

    // ========== Task::new tests ==========

    #[test]
    fn test_task_new_sets_defaults() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let payload = serde_json::json!({"url": "https://example.com"});

        let before = Utc::now();
        let task = Task::new(
            id,
            TaskType::Scrape,
            team_id,
            api_key_id,
            "https://example.com".to_string(),
            payload.clone(),
        );
        let after = Utc::now();

        assert_eq!(task.id, id);
        assert_eq!(task.task_type, TaskType::Scrape);
        assert_eq!(task.status, TaskStatus::Queued, "new task should be Queued");
        assert_eq!(task.priority, 0, "default priority should be 0");
        assert_eq!(task.team_id, team_id);
        assert_eq!(task.api_key_id, api_key_id);
        assert_eq!(task.url, "https://example.com");
        assert_eq!(task.payload, payload);
        assert_eq!(task.retry_count, 0);
        assert_eq!(task.attempt_count, 0);
        assert_eq!(task.max_retries, 3, "default max_retries should be 3");
        assert!(task.scheduled_at.is_none());
        assert!(task.expires_at.is_none());
        assert!(task.started_at.is_none());
        assert!(task.completed_at.is_none());
        assert!(task.crawl_id.is_none());
        assert!(task.lock_token.is_none());
        assert!(task.lock_expires_at.is_none());
        assert!(
            task.created_at >= before && task.created_at <= after,
            "created_at should be now"
        );
        assert_eq!(task.created_at, task.updated_at);
    }

    // ========== can_retry tests ==========

    #[test]
    fn test_can_retry_true_when_below_max() {
        let mut task = make_task();
        task.retry_count = 2;
        task.max_retries = 3;
        assert!(task.can_retry(), "should retry when retry_count < max_retries");
    }

    #[test]
    fn test_can_retry_false_when_at_max() {
        let mut task = make_task();
        task.retry_count = 3;
        task.max_retries = 3;
        assert!(!task.can_retry(), "should not retry when retry_count == max_retries");
    }

    #[test]
    fn test_can_retry_false_when_above_max() {
        let mut task = make_task();
        task.retry_count = 5;
        task.max_retries = 3;
        assert!(!task.can_retry(), "should not retry when retry_count > max_retries");
    }

    // ========== is_expired tests ==========

    #[test]
    fn test_is_expired_false_when_no_expiry() {
        let task = make_task();
        assert!(!task.is_expired(), "task without expires_at should not be expired");
    }

    #[test]
    fn test_is_expired_true_when_past_expiry() {
        let mut task = make_task();
        task.expires_at = Some(Utc::now() - chrono::Duration::seconds(10));
        assert!(task.is_expired(), "task past expires_at should be expired");
    }

    #[test]
    fn test_is_expired_false_when_future_expiry() {
        let mut task = make_task();
        task.expires_at = Some(Utc::now() + chrono::Duration::seconds(3600));
        assert!(!task.is_expired(), "task with future expires_at should not be expired");
    }

    // ========== is_locked tests ==========

    #[test]
    fn test_is_locked_false_when_no_lock_token() {
        let task = make_task();
        assert!(!task.is_locked(), "task without lock_token should not be locked");
    }

    #[test]
    fn test_is_locked_true_when_lock_in_future() {
        let mut task = make_task();
        let worker = Uuid::new_v4();
        task.acquire_lock(worker, chrono::Duration::seconds(60));
        assert!(task.is_locked(), "task with future lock expiry should be locked");
    }

    #[test]
    fn test_is_locked_false_when_lock_expired() {
        let mut task = make_task();
        task.lock_token = Some(Uuid::new_v4());
        task.lock_expires_at = Some(Utc::now() - chrono::Duration::seconds(10));
        assert!(
            !task.is_locked(),
            "task with past lock expiry should not be locked"
        );
    }

    // ========== start / complete / fail / cancel tests ==========

    #[test]
    fn test_start_sets_active_and_started_at() {
        let mut task = make_task();
        let before = Utc::now();
        task.start();

        assert_eq!(task.status, TaskStatus::Active);
        assert!(task.started_at.is_some(), "started_at should be set");
        assert!(task.started_at.expect("started_at set") >= before);
        assert!(task.updated_at >= before);
    }

    #[test]
    fn test_complete_sets_completed_and_completed_at() {
        let mut task = make_task();
        task.status = TaskStatus::Active;
        let before = Utc::now();
        task.complete();

        assert_eq!(task.status, TaskStatus::Completed);
        assert!(task.completed_at.is_some(), "completed_at should be set");
        assert!(task.completed_at.expect("completed_at set") >= before);
        assert!(task.updated_at >= before);
    }

    #[test]
    fn test_fail_sets_failed_and_completed_at() {
        let mut task = make_task();
        task.status = TaskStatus::Active;
        let before = Utc::now();
        task.fail();

        assert_eq!(task.status, TaskStatus::Failed);
        assert!(task.completed_at.is_some(), "failed task should set completed_at");
        assert!(task.updated_at >= before);
    }

    #[test]
    fn test_cancel_sets_cancelled_and_completed_at() {
        let mut task = make_task();
        let before = Utc::now();
        task.cancel();

        assert_eq!(task.status, TaskStatus::Cancelled);
        assert!(task.completed_at.is_some(), "cancelled task should set completed_at");
        assert!(task.updated_at >= before);
    }

    // ========== increment_retry tests ==========

    #[test]
    fn test_increment_retry_increments_both_counters() {
        let mut task = make_task();
        assert_eq!(task.retry_count, 0);
        assert_eq!(task.attempt_count, 0);

        let before = Utc::now();
        task.increment_retry();

        assert_eq!(task.retry_count, 1, "retry_count should increment");
        assert_eq!(task.attempt_count, 1, "attempt_count should increment");
        assert!(task.updated_at >= before);
    }

    #[test]
    fn test_increment_retry_multiple_times() {
        let mut task = make_task();
        task.increment_retry();
        task.increment_retry();
        task.increment_retry();
        assert_eq!(task.retry_count, 3);
        assert_eq!(task.attempt_count, 3);
    }

    // ========== acquire_lock / release_lock tests ==========

    #[test]
    fn test_acquire_lock_sets_token_and_expiry() {
        let mut task = make_task();
        let worker = Uuid::new_v4();
        let duration = chrono::Duration::seconds(30);
        let before = Utc::now();

        task.acquire_lock(worker, duration);

        assert_eq!(task.lock_token, Some(worker), "lock_token should be worker id");
        let expiry = task
            .lock_expires_at
            .expect("lock_expires_at should be set");
        assert!(
            expiry >= before + duration,
            "lock_expires_at should be ~duration in the future"
        );
        assert!(task.updated_at >= before);
    }

    #[test]
    fn test_release_lock_clears_token_and_expiry() {
        let mut task = make_task();
        task.acquire_lock(Uuid::new_v4(), chrono::Duration::seconds(30));
        assert!(task.lock_token.is_some());

        let before = Utc::now();
        task.release_lock();

        assert!(task.lock_token.is_none(), "lock_token should be cleared");
        assert!(task.lock_expires_at.is_none(), "lock_expires_at should be cleared");
        assert!(task.updated_at >= before);
    }

    // ========== serde roundtrip ==========

    #[test]
    fn test_task_serde_roundtrip() {
        let mut task = make_task();
        task.priority = 5;
        task.retry_count = 2;
        task.crawl_id = Some(Uuid::new_v4());

        let json = serde_json::to_string(&task).expect("serialize");
        let back: Task = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(task, back, "serde roundtrip should preserve task");
    }

    // ========== Helper ==========

    fn make_task() -> Task {
        Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        )
    }
}
