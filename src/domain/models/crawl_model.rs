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
    #[allow(clippy::too_many_arguments)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    // ========== Crawl::new tests ==========

    #[test]
    fn test_crawl_new_sets_defaults() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let config = serde_json::json!({"max_depth": 3});

        let before = Utc::now();
        let crawl = Crawl::new(
            id,
            team_id,
            "Test Crawl".to_string(),
            "https://example.com".to_string(),
            "https://example.com/page".to_string(),
            config.clone(),
        );
        let after = Utc::now();

        assert_eq!(crawl.id, id);
        assert_eq!(crawl.team_id, team_id);
        assert_eq!(crawl.name, "Test Crawl");
        assert_eq!(crawl.root_url, "https://example.com");
        assert_eq!(crawl.url, "https://example.com/page");
        assert_eq!(
            crawl.status,
            CrawlStatus::Queued,
            "new crawl should be Queued"
        );
        assert_eq!(crawl.config(), &config);
        assert_eq!(crawl.total_tasks(), 0, "new crawl should have 0 tasks");
        assert_eq!(crawl.completed_tasks(), 0);
        assert_eq!(crawl.failed_tasks(), 0);
        assert!(crawl.completed_at.is_none());
        assert!(
            crawl.created_at >= before && crawl.created_at <= after,
            "created_at should be now"
        );
        assert_eq!(crawl.created_at, crawl.updated_at);
    }

    // ========== Crawl::with_all_fields tests ==========

    #[test]
    fn test_crawl_with_all_fields_sets_everything() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let config = serde_json::json!({"k": "v"});
        let created = Utc::now();
        let updated = created + chrono::Duration::seconds(10);
        let completed = updated + chrono::Duration::seconds(5);

        let crawl = Crawl::with_all_fields(
            id,
            team_id,
            "Full Crawl".to_string(),
            "https://root.com".to_string(),
            "https://root.com/x".to_string(),
            CrawlStatus::Completed,
            config.clone(),
            10,
            8,
            2,
            created,
            updated,
            Some(completed),
        );

        assert_eq!(crawl.id, id);
        assert_eq!(crawl.team_id, team_id);
        assert_eq!(crawl.name, "Full Crawl");
        assert_eq!(crawl.root_url, "https://root.com");
        assert_eq!(crawl.url, "https://root.com/x");
        assert_eq!(crawl.status, CrawlStatus::Completed);
        assert_eq!(crawl.config(), &config);
        assert_eq!(crawl.total_tasks(), 10);
        assert_eq!(crawl.completed_tasks(), 8);
        assert_eq!(crawl.failed_tasks(), 2);
        assert_eq!(crawl.created_at, created);
        assert_eq!(crawl.updated_at, updated);
        assert_eq!(crawl.completed_at, Some(completed));
    }

    // ========== progress_percentage tests ==========

    #[test]
    fn test_progress_percentage_zero_tasks_returns_100() {
        let crawl = make_crawl();
        assert_eq!(
            crawl.progress_percentage(),
            100.0,
            "0 total tasks should return 100%"
        );
    }

    #[test]
    fn test_progress_percentage_partial() {
        let mut crawl = make_crawl();
        crawl.set_total_tasks(10);
        for _ in 0..5 {
            crawl.increment_completed_tasks();
        }
        let pct = crawl.progress_percentage();
        assert!(
            (pct - 50.0).abs() < f64::EPSILON,
            "5/10 completed should be 50%, got {}",
            pct
        );
    }

    #[test]
    fn test_progress_percentage_all_completed() {
        let mut crawl = make_crawl();
        crawl.set_total_tasks(4);
        for _ in 0..4 {
            crawl.increment_completed_tasks();
        }
        let pct = crawl.progress_percentage();
        assert!(
            (pct - 100.0).abs() < f64::EPSILON,
            "4/4 completed should be 100%, got {}",
            pct
        );
    }

    // ========== is_finished tests ==========

    #[test]
    fn test_is_finished_true_for_terminal_statuses() {
        let mut crawl = make_crawl();
        for status in [
            CrawlStatus::Completed,
            CrawlStatus::Failed,
            CrawlStatus::Cancelled,
        ] {
            crawl.status = status;
            assert!(
                crawl.is_finished(),
                "{:?} should be a finished status",
                status
            );
        }
    }

    #[test]
    fn test_is_finished_false_for_non_terminal_statuses() {
        let mut crawl = make_crawl();
        for status in [CrawlStatus::Queued, CrawlStatus::Processing] {
            crawl.status = status;
            assert!(
                !crawl.is_finished(),
                "{:?} should not be a finished status",
                status
            );
        }
    }

    // ========== start / complete / fail / cancel tests ==========

    #[test]
    fn test_start_sets_processing() {
        let mut crawl = make_crawl();
        let before = Utc::now();
        crawl.start();

        assert_eq!(crawl.status, CrawlStatus::Processing);
        assert!(crawl.updated_at >= before);
    }

    #[test]
    fn test_complete_sets_completed_and_completed_at() {
        let mut crawl = make_crawl();
        crawl.status = CrawlStatus::Processing;
        let before = Utc::now();
        crawl.complete();

        assert_eq!(crawl.status, CrawlStatus::Completed);
        assert!(crawl.completed_at.is_some());
        assert!(crawl.completed_at.expect("completed_at set") >= before);
        assert!(crawl.updated_at >= before);
    }

    #[test]
    fn test_fail_sets_failed_and_completed_at() {
        let mut crawl = make_crawl();
        let before = Utc::now();
        crawl.fail();

        assert_eq!(crawl.status, CrawlStatus::Failed);
        assert!(crawl.completed_at.is_some());
        assert!(crawl.updated_at >= before);
    }

    #[test]
    fn test_cancel_sets_cancelled_and_completed_at() {
        let mut crawl = make_crawl();
        let before = Utc::now();
        crawl.cancel();

        assert_eq!(crawl.status, CrawlStatus::Cancelled);
        assert!(crawl.completed_at.is_some());
        assert!(crawl.updated_at >= before);
    }

    // ========== increment / set task counters ==========

    #[test]
    fn test_increment_total_tasks() {
        let mut crawl = make_crawl();
        assert_eq!(crawl.total_tasks(), 0);

        crawl.increment_total_tasks();
        crawl.increment_total_tasks();
        assert_eq!(crawl.total_tasks(), 2, "should increment by 1 each call");
    }

    #[test]
    fn test_increment_completed_tasks() {
        let mut crawl = make_crawl();
        crawl.increment_completed_tasks();
        assert_eq!(crawl.completed_tasks(), 1);
    }

    #[test]
    fn test_increment_failed_tasks() {
        let mut crawl = make_crawl();
        crawl.increment_failed_tasks();
        crawl.increment_failed_tasks();
        assert_eq!(crawl.failed_tasks(), 2);
    }

    #[test]
    fn test_set_total_tasks_overrides_count() {
        let mut crawl = make_crawl();
        crawl.increment_total_tasks();
        crawl.increment_total_tasks();
        crawl.set_total_tasks(50);
        assert_eq!(crawl.total_tasks(), 50, "set_total_tasks should override");
    }

    #[test]
    fn test_set_total_tasks_zero() {
        let mut crawl = make_crawl();
        crawl.set_total_tasks(0);
        assert_eq!(crawl.total_tasks(), 0);
        assert_eq!(crawl.progress_percentage(), 100.0, "0 tasks still 100%");
    }

    // ========== CrawlStatus Display / FromStr tests ==========

    #[test]
    fn test_crawl_status_display_all_variants() {
        assert_eq!(CrawlStatus::Queued.to_string(), "queued");
        assert_eq!(CrawlStatus::Processing.to_string(), "processing");
        assert_eq!(CrawlStatus::Completed.to_string(), "completed");
        assert_eq!(CrawlStatus::Failed.to_string(), "failed");
        assert_eq!(CrawlStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn test_crawl_status_from_str_valid() {
        assert_eq!(
            CrawlStatus::from_str("queued").expect("valid"),
            CrawlStatus::Queued
        );
        assert_eq!(
            CrawlStatus::from_str("processing").expect("valid"),
            CrawlStatus::Processing
        );
        assert_eq!(
            CrawlStatus::from_str("completed").expect("valid"),
            CrawlStatus::Completed
        );
        assert_eq!(
            CrawlStatus::from_str("failed").expect("valid"),
            CrawlStatus::Failed
        );
        assert_eq!(
            CrawlStatus::from_str("cancelled").expect("valid"),
            CrawlStatus::Cancelled
        );
    }

    #[test]
    fn test_crawl_status_from_str_invalid_returns_error() {
        let err = CrawlStatus::from_str("unknown").expect_err("invalid status should error");
        assert!(
            err.contains("Invalid crawl status"),
            "error should describe invalid status: {}",
            err
        );
        assert!(
            err.contains("unknown"),
            "error should include the bad value"
        );
    }

    #[test]
    fn test_crawl_status_default_is_queued() {
        assert_eq!(CrawlStatus::default(), CrawlStatus::Queued);
    }

    #[test]
    fn test_crawl_status_serde_roundtrip() {
        for status in [
            CrawlStatus::Queued,
            CrawlStatus::Processing,
            CrawlStatus::Completed,
            CrawlStatus::Failed,
            CrawlStatus::Cancelled,
        ] {
            let json = serde_json::to_string(&status).expect("serialize");
            let back: CrawlStatus = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(status, back, "roundtrip should preserve: {}", json);
        }
    }

    // ========== Crawl serde roundtrip ==========

    #[test]
    fn test_crawl_serde_roundtrip() {
        let mut crawl = make_crawl();
        crawl.set_total_tasks(10);
        crawl.increment_completed_tasks();

        let json = serde_json::to_string(&crawl).expect("serialize");
        let back: Crawl = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(crawl, back, "serde roundtrip should preserve crawl");
    }

    // ========== Helper ==========

    fn make_crawl() -> Crawl {
        Crawl::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "Test".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        )
    }
}
