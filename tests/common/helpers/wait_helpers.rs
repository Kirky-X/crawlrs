// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(dead_code)]

use crawlrs::domain::models::task::{Task, TaskStatus};
use crawlrs::domain::repositories::task_repository::TaskRepository;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

pub struct WaitHelpers;

impl WaitHelpers {
    async fn poll_with_timeout<T, F>(
        mut condition: F,
        timeout: Duration,
        interval: Duration,
        error_message: String,
    ) -> Result<T, String>
    where
        F: FnMut() -> Option<T>,
    {
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > timeout {
                return Err(error_message);
            }

            if let Some(result) = condition() {
                return Ok(result);
            }

            sleep(interval).await;
        }
    }

    pub async fn wait_for_task_completion<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        Self::poll_with_timeout(
            || {
                task_repo.find_by_id(task_id).ok().flatten().filter(|task| {
                    task.status == TaskStatus::Completed || task.status == TaskStatus::Failed
                })
            },
            Duration::from_secs(timeout_secs),
            Duration::from_millis(100),
            format!(
                "Task {} did not complete within {} seconds",
                task_id, timeout_secs
            ),
        )
        .await
    }

    pub async fn wait_for_task_status<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        expected_status: TaskStatus,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        Self::poll_with_timeout(
            || {
                task_repo
                    .find_by_id(task_id)
                    .ok()
                    .flatten()
                    .filter(|task| task.status == expected_status)
            },
            Duration::from_secs(timeout_secs),
            Duration::from_millis(100),
            format!(
                "Task {} did not reach expected status within {} seconds",
                task_id, timeout_secs
            ),
        )
        .await
    }

    pub async fn wait_for_queued<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        Self::wait_for_task_status(task_repo, task_id, TaskStatus::Queued, timeout_secs).await
    }

    pub async fn wait_for_active<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        Self::wait_for_task_status(task_repo, task_id, TaskStatus::Active, timeout_secs).await
    }

    pub async fn wait_for_completed<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        Self::wait_for_task_status(task_repo, task_id, TaskStatus::Completed, timeout_secs).await
    }

    pub async fn wait_for_failed<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        Self::wait_for_task_status(task_repo, task_id, TaskStatus::Failed, timeout_secs).await
    }

    pub async fn wait_for_cancelled<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        Self::wait_for_task_status(task_repo, task_id, TaskStatus::Cancelled, timeout_secs).await
    }

    pub async fn poll_until<T, F>(
        condition: F,
        timeout_secs: u64,
        interval_ms: u64,
    ) -> Result<T, String>
    where
        F: FnMut() -> Option<T>,
    {
        Self::poll_with_timeout(
            condition,
            Duration::from_secs(timeout_secs),
            Duration::from_millis(interval_ms),
            "Timeout waiting for condition".to_string(),
        )
        .await
    }
}
