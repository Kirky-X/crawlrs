// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Test helper functions for workers tests

use chrono::Utc;

use crate::domain::models::task::{Task, TaskStatus, TaskType};

/// Create a test task with specified status
pub fn create_test_task(status: TaskStatus) -> Task {
    Task {
        id: crawlrs::domain::id::Id::generate().into(),
        task_type: TaskType::Scrape,
        status,
        priority: 0,
        team_id: crawlrs::domain::id::Id::generate().into(),
        api_key_id: crawlrs::domain::id::Id::generate().into(),
        url: "https://example.com".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: Utc::now(),
        lock_token: None,
        lock_expires_at: None,
    }
}
