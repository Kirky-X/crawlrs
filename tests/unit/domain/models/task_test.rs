// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use serde_json::json;
use uuid::Uuid;

#[test]
fn test_task_lifecycle_happy_path() {
    // Given: 新创建的任务
    let team_id = Uuid::new_v4();
    let url = "http://example.com".to_string();
    let payload = json!({});
    let mut task = Task::new(TaskType::Scrape, team_id, url, payload);
    assert_eq!(task.status, TaskStatus::Queued);

    task.status = TaskStatus::Active;
    assert_eq!(task.status, TaskStatus::Active);

    task.status = TaskStatus::Completed;
    assert_eq!(task.status, TaskStatus::Completed);
}

#[test]
fn test_task_retry_logic() {
    // Given: 失败任务
    let team_id = Uuid::new_v4();
    let url = "http://example.com".to_string();
    let payload = json!({});
    let mut task = Task::new(TaskType::Scrape, team_id, url, payload);

    task.status = TaskStatus::Failed;
    task.attempt_count = 2;
    task.max_retries = 3;

    // When: 检查是否可重试
    let can_retry = task.attempt_count < task.max_retries;
    assert!(can_retry);

    // When: 重试次数达到上限
    task.attempt_count = 3;
    let can_retry_full = task.attempt_count < task.max_retries;

    // Then: 不可重试
    assert!(!can_retry_full);
}
