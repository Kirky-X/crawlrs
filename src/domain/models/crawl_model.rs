// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl domain model - pure domain entity without ORM annotations
//!
//! This module contains the pure domain model for Crawl,
//! following Domain-Driven Design principles.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// Crawl domain model
///
/// Represents a website crawl task with multiple sub-tasks.
/// This is a pure domain model without any ORM annotations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Crawl {
    /// Unique identifier
    pub id: Uuid,
    /// Team ID for multi-tenancy
    pub team_id: Uuid,
    /// Human-readable name
    pub name: String,
    /// Root URL for the crawl
    pub root_url: String,
    /// Current target URL
    pub url: String,
    /// Current status
    pub status: CrawlStatus,
    /// Crawl configuration as JSON
    config: serde_json::Value,
    /// Total number of sub-tasks
    total_tasks: i32,
    /// Number of completed sub-tasks
    completed_tasks: i32,
    /// Number of failed sub-tasks
    failed_tasks: i32,
    /// When the crawl was created
    pub created_at: DateTime<Utc>,
    /// When the crawl was last updated
    pub updated_at: DateTime<Utc>,
    /// When the crawl completed
    pub completed_at: Option<DateTime<Utc>>,
}

impl Crawl {
    /// Create a new crawl
    pub fn new(
        id: Uuid,
        team_id: Uuid,
        name: String,
        root_url: String,
        url: String,
        config: serde_json::Value,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            team_id,
            name,
            root_url,
            url,
            status: CrawlStatus::Queued,
            config,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }

    /// Create a crawl with all fields (for mappers)
    pub fn with_all_fields(
        id: Uuid,
        team_id: Uuid,
        name: String,
        root_url: String,
        url: String,
        status: CrawlStatus,
        config: serde_json::Value,
        total_tasks: i32,
        completed_tasks: i32,
        failed_tasks: i32,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        completed_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            id,
            team_id,
            name,
            root_url,
            url,
            status,
            config,
            total_tasks,
            completed_tasks,
            failed_tasks,
            created_at,
            updated_at,
            completed_at,
        }
    }

    /// Get the crawl configuration
    pub fn config(&self) -> &serde_json::Value {
        &self.config
    }

    /// Get total tasks count
    pub fn total_tasks(&self) -> i32 {
        self.total_tasks
    }

    /// Get completed tasks count
    pub fn completed_tasks(&self) -> i32 {
        self.completed_tasks
    }

    /// Get failed tasks count
    pub fn failed_tasks(&self) -> i32 {
        self.failed_tasks
    }

    /// Calculate progress percentage
    pub fn progress_percentage(&self) -> f64 {
        if self.total_tasks == 0 {
            return 100.0;
        }
        (self.completed_tasks as f64 / self.total_tasks as f64) * 100.0
    }

    /// Check if the crawl is finished
    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            CrawlStatus::Completed | CrawlStatus::Failed | CrawlStatus::Cancelled
        )
    }

    /// Start the crawl
    pub fn start(&mut self) {
        self.status = CrawlStatus::Processing;
        self.updated_at = Utc::now();
    }

    /// Complete the crawl
    pub fn complete(&mut self) {
        self.status = CrawlStatus::Completed;
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Fail the crawl
    pub fn fail(&mut self) {
        self.status = CrawlStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Cancel the crawl
    pub fn cancel(&mut self) {
        self.status = CrawlStatus::Cancelled;
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Increment total tasks
    pub fn increment_total_tasks(&mut self) {
        self.total_tasks += 1;
        self.updated_at = Utc::now();
    }

    /// Increment completed tasks
    pub fn increment_completed_tasks(&mut self) {
        self.completed_tasks += 1;
        self.updated_at = Utc::now();
    }

    /// Increment failed tasks
    pub fn increment_failed_tasks(&mut self) {
        self.failed_tasks += 1;
        self.updated_at = Utc::now();
    }

    /// Set total tasks
    pub fn set_total_tasks(&mut self, count: i32) {
        self.total_tasks = count;
        self.updated_at = Utc::now();
    }
}

/// Crawl status enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CrawlStatus {
    /// Queued for processing
    #[default]
    Queued,
    /// Currently processing
    Processing,
    /// Completed successfully
    Completed,
    /// Failed
    Failed,
    /// Cancelled
    Cancelled,
}

impl fmt::Display for CrawlStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrawlStatus::Queued => write!(f, "queued"),
            CrawlStatus::Processing => write!(f, "processing"),
            CrawlStatus::Completed => write!(f, "completed"),
            CrawlStatus::Failed => write!(f, "failed"),
            CrawlStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl FromStr for CrawlStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(CrawlStatus::Queued),
            "processing" => Ok(CrawlStatus::Processing),
            "completed" => Ok(CrawlStatus::Completed),
            "failed" => Ok(CrawlStatus::Failed),
            "cancelled" => Ok(CrawlStatus::Cancelled),
            _ => Err(format!("Invalid crawl status: {}", s)),
        }
    }
}
