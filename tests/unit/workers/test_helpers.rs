// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Test helper functions for workers tests

use chrono::Utc;
use uuid::Uuid;

use crate::domain::models::task::{Task, TaskStatus, TaskType};

/// Create a test task with specified status
pub fn create_test_task(status: TaskStatus) -> Task {
    Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status,
        priority: 0,
        team_id: Uuid::new_v4(),
        api_key_id: Uuid::new_v4(),
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
