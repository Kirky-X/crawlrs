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
        self.expires_at.map_or(false, |expires_at| Utc::now() > expires_at)
    }

    /// Check if the task is locked
    pub fn is_locked(&self) -> bool {
        self.lock_token.is_some() && 
            self.lock_expires_at.map_or(false, |expires_at| Utc::now() < expires_at)
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
