// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 等待辅助函数
///
/// 提供任务等待和轮询功能
use crawlrs::domain::models::task::Task;
use crawlrs::domain::repositories::task_repository::TaskRepository;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

/// 等待辅助函数
pub struct WaitHelpers;

impl WaitHelpers {
    /// 等待任务完成（成功或失败）
    pub async fn wait_for_task_completion<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        loop {
            if start.elapsed() > timeout {
                return Err(format!(
                    "Task {} did not complete within {} seconds",
                    task_id, timeout_secs
                ));
            }

            if let Ok(Some(task)) = task_repo.find_by_id(task_id).await {
                if task.status == crawlrs::domain::models::task::TaskStatus::Completed
                    || task.status == crawlrs::domain::models::task::TaskStatus::Failed
                {
                    return Ok(task);
                }
            }

            sleep(Duration::from_millis(100)).await;
        }
    }

    /// 等待任务达到特定状态
    pub async fn wait_for_task_status<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        expected_status: crawlrs::domain::models::task::TaskStatus,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        loop {
            if start.elapsed() > timeout {
                return Err(format!(
                    "Task {} did not reach expected status within {} seconds",
                    task_id, timeout_secs
                ));
            }

            if let Ok(Some(task)) = task_repo.find_by_id(task_id).await {
                if task.status == expected_status {
                    return Ok(task);
                }
            }

            sleep(Duration::from_millis(100)).await;
        }
    }

    /// 等待任务状态变为 Queued
    pub async fn wait_for_queued<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        Self::wait_for_task_status(
            task_repo,
            task_id,
            crawlrs::domain::models::task::TaskStatus::Queued,
            timeout_secs,
        )
        .await
    }

    /// 等待任务状态变为 Active
    pub async fn wait_for_active<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        Self::wait_for_task_status(
            task_repo,
            task_id,
            crawlrs::domain::models::task::TaskStatus::Active,
            timeout_secs,
        )
        .await
    }

    /// 等待任务状态变为 Completed
    pub async fn wait_for_completed<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        Self::wait_for_task_status(
            task_repo,
            task_id,
            crawlrs::domain::models::task::TaskStatus::Completed,
            timeout_secs,
        )
        .await
    }

    /// 等待任务状态变为 Failed
    pub async fn wait_for_failed<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        Self::wait_for_task_status(
            task_repo,
            task_id,
            crawlrs::domain::models::task::TaskStatus::Failed,
            timeout_secs,
        )
        .await
    }

    /// 等待任务状态变为 Cancelled
    pub async fn wait_for_cancelled<T: TaskRepository>(
        task_repo: &T,
        task_id: Uuid,
        timeout_secs: u64,
    ) -> Result<Task, String> {
        Self::wait_for_task_status(
            task_repo,
            task_id,
            crawlrs::domain::models::task::TaskStatus::Cancelled,
            timeout_secs,
        )
        .await
    }

    /// 轮询直到条件满足
    pub async fn poll_until<R, T, F>(
        mut condition: F,
        timeout_secs: u64,
        interval_ms: u64,
    ) -> Result<T, String>
    where
        F: FnMut() -> Option<T>,
    {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_secs);
        let interval = Duration::from_millis(interval_ms);

        loop {
            if start.elapsed() > timeout {
                return Err("Timeout waiting for condition".to_string());
            }

            if let Some(result) = condition() {
                return Ok(result);
            }

            sleep(interval).await;
        }
    }
}
