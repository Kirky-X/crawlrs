// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Builder pattern constructors for domain models with many fields.
//!
//! Provides ergonomic, test-friendly construction of complex domain
//! entities without requiring callers to remember the full field list
//! or positional argument order.
//!
//! # Available builders
//!
//! - [`TaskBuilder`] — constructs [`Task`] instances with sensible defaults.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::task_domain::{TaskStatus, TaskType};
use super::task_model::Task;

/// Builder for [`Task`] domain model.
///
/// Provides sensible defaults for all fields. Required fields must be set
/// explicitly; optional fields default to `None`.
///
/// # Examples
///
/// ```rust,ignore
/// let task = TaskBuilder::new()
///     .team_id(team_id)
///     .api_key_id(api_key_id)
///     .url("https://example.com".to_string())
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct TaskBuilder {
    id: Uuid,
    task_type: TaskType,
    status: TaskStatus,
    priority: i32,
    team_id: Uuid,
    api_key_id: Uuid,
    url: String,
    payload: serde_json::Value,
    retry_count: i32,
    attempt_count: i32,
    max_retries: i32,
    scheduled_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    crawl_id: Option<Uuid>,
    updated_at: DateTime<Utc>,
    lock_token: Option<Uuid>,
    lock_expires_at: Option<DateTime<Utc>>,
}

impl TaskBuilder {
    /// Create a new builder with sensible defaults.
    ///
    /// Defaults:
    /// - `id`: new random UUID
    /// - `task_type`: `TaskType::Scrape`
    /// - `status`: `TaskStatus::Queued`
    /// - `priority`: 0
    /// - `team_id` / `api_key_id`: new random UUIDs
    /// - `url`: `"https://example.com"`
    /// - `payload`: empty JSON object
    /// - `retry_count` / `attempt_count`: 0
    /// - `max_retries`: 3
    /// - `created_at` / `updated_at`: `Utc::now()`
    /// - All optional fields: `None`
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            status: TaskStatus::Queued,
            priority: 0,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::Value::Null,
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

    pub fn id(mut self, id: Uuid) -> Self {
        self.id = id;
        self
    }

    pub fn task_type(mut self, task_type: TaskType) -> Self {
        self.task_type = task_type;
        self
    }

    pub fn status(mut self, status: TaskStatus) -> Self {
        self.status = status;
        self
    }

    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn team_id(mut self, team_id: Uuid) -> Self {
        self.team_id = team_id;
        self
    }

    pub fn api_key_id(mut self, api_key_id: Uuid) -> Self {
        self.api_key_id = api_key_id;
        self
    }

    pub fn url(mut self, url: String) -> Self {
        self.url = url;
        self
    }

    pub fn payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = payload;
        self
    }

    pub fn retry_count(mut self, retry_count: i32) -> Self {
        self.retry_count = retry_count;
        self
    }

    pub fn attempt_count(mut self, attempt_count: i32) -> Self {
        self.attempt_count = attempt_count;
        self
    }

    pub fn max_retries(mut self, max_retries: i32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn scheduled_at(mut self, scheduled_at: Option<DateTime<Utc>>) -> Self {
        self.scheduled_at = scheduled_at;
        self
    }

    pub fn expires_at(mut self, expires_at: Option<DateTime<Utc>>) -> Self {
        self.expires_at = expires_at;
        self
    }

    pub fn created_at(mut self, created_at: DateTime<Utc>) -> Self {
        self.created_at = created_at;
        self
    }

    pub fn started_at(mut self, started_at: Option<DateTime<Utc>>) -> Self {
        self.started_at = started_at;
        self
    }

    pub fn completed_at(mut self, completed_at: Option<DateTime<Utc>>) -> Self {
        self.completed_at = completed_at;
        self
    }

    pub fn crawl_id(mut self, crawl_id: Option<Uuid>) -> Self {
        self.crawl_id = crawl_id;
        self
    }

    pub fn updated_at(mut self, updated_at: DateTime<Utc>) -> Self {
        self.updated_at = updated_at;
        self
    }

    pub fn lock_token(mut self, lock_token: Option<Uuid>) -> Self {
        self.lock_token = lock_token;
        self
    }

    pub fn lock_expires_at(mut self, lock_expires_at: Option<DateTime<Utc>>) -> Self {
        self.lock_expires_at = lock_expires_at;
        self
    }

    /// Build the [`Task`] from the current builder state.
    pub fn build(self) -> Task {
        Task {
            id: self.id,
            task_type: self.task_type,
            status: self.status,
            priority: self.priority,
            team_id: self.team_id,
            api_key_id: self.api_key_id,
            url: self.url,
            payload: self.payload,
            retry_count: self.retry_count,
            attempt_count: self.attempt_count,
            max_retries: self.max_retries,
            scheduled_at: self.scheduled_at,
            expires_at: self.expires_at,
            created_at: self.created_at,
            started_at: self.started_at,
            completed_at: self.completed_at,
            crawl_id: self.crawl_id,
            updated_at: self.updated_at,
            lock_token: self.lock_token,
            lock_expires_at: self.lock_expires_at,
        }
    }
}

impl Default for TaskBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_task_builder_defaults() {
        let task = TaskBuilder::new().build();
        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(task.task_type, TaskType::Scrape);
        assert_eq!(task.priority, 0);
        assert_eq!(task.retry_count, 0);
        assert_eq!(task.attempt_count, 0);
        assert_eq!(task.max_retries, 3);
        assert_eq!(task.url, "https://example.com");
        assert!(task.started_at.is_none());
        assert!(task.completed_at.is_none());
        assert!(task.crawl_id.is_none());
        assert!(task.lock_token.is_none());
        assert!(task.lock_expires_at.is_none());
        assert!(task.expires_at.is_none());
        assert!(task.scheduled_at.is_none());
    }

    #[test]
    fn test_task_builder_with_required_fields() {
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let task = TaskBuilder::new()
            .team_id(team_id)
            .api_key_id(api_key_id)
            .url("https://test.com".to_string())
            .build();
        assert_eq!(task.team_id, team_id);
        assert_eq!(task.api_key_id, api_key_id);
        assert_eq!(task.url, "https://test.com");
    }

    #[test]
    fn test_task_builder_with_all_fields() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let lock_token = Uuid::new_v4();
        let now = Utc::now();

        let task = TaskBuilder::new()
            .id(id)
            .task_type(TaskType::Crawl)
            .status(TaskStatus::Active)
            .priority(5)
            .team_id(team_id)
            .api_key_id(api_key_id)
            .url("https://custom.com".to_string())
            .payload(serde_json::json!({"key": "value"}))
            .retry_count(1)
            .attempt_count(2)
            .max_retries(5)
            .scheduled_at(Some(now))
            .expires_at(Some(now + Duration::hours(1)))
            .created_at(now)
            .started_at(Some(now))
            .completed_at(Some(now + Duration::minutes(5)))
            .crawl_id(Some(crawl_id))
            .updated_at(now)
            .lock_token(Some(lock_token))
            .lock_expires_at(Some(now + Duration::minutes(10)))
            .build();

        assert_eq!(task.id, id);
        assert_eq!(task.task_type, TaskType::Crawl);
        assert_eq!(task.status, TaskStatus::Active);
        assert_eq!(task.priority, 5);
        assert_eq!(task.team_id, team_id);
        assert_eq!(task.api_key_id, api_key_id);
        assert_eq!(task.url, "https://custom.com");
        assert_eq!(task.payload, serde_json::json!({"key": "value"}));
        assert_eq!(task.retry_count, 1);
        assert_eq!(task.attempt_count, 2);
        assert_eq!(task.max_retries, 5);
        assert_eq!(task.scheduled_at, Some(now));
        assert_eq!(task.expires_at, Some(now + Duration::hours(1)));
        assert_eq!(task.started_at, Some(now));
        assert_eq!(task.completed_at, Some(now + Duration::minutes(5)));
        assert_eq!(task.crawl_id, Some(crawl_id));
        assert_eq!(task.lock_token, Some(lock_token));
        assert_eq!(task.lock_expires_at, Some(now + Duration::minutes(10)));
    }

    #[test]
    fn test_task_builder_default_trait() {
        let builder = TaskBuilder::default();
        let task = builder.build();
        assert_eq!(task.status, TaskStatus::Queued);
    }

    #[test]
    fn test_task_builder_clone() {
        let builder = TaskBuilder::new().priority(10);
        let cloned = builder.clone();
        let task1 = builder.build();
        let task2 = cloned.build();
        assert_eq!(task1.priority, task2.priority);
        assert_eq!(task1.id, task2.id);
    }

    #[test]
    fn test_task_builder_chaining() {
        let task = TaskBuilder::new()
            .task_type(TaskType::Extract)
            .priority(3)
            .max_retries(10)
            .build();
        assert_eq!(task.task_type, TaskType::Extract);
        assert_eq!(task.priority, 3);
        assert_eq!(task.max_retries, 10);
    }
}
