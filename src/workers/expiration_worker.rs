// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::repositories::task_repository::TaskRepository;
use crate::workers::worker::{ProcessResult, WorkerProcess};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::info;

/// 任务过期清理工作器
///
/// 负责定期扫描并清理过期的任务
pub struct ExpirationWorker<R>
where
    R: TaskRepository + Send + Sync + 'static,
{
    repository: Arc<R>,
}

impl<R> ExpirationWorker<R>
where
    R: TaskRepository + Send + Sync + 'static,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    async fn cleanup_expired_tasks(&self) -> Result<u64, String> {
        self.repository
            .expire_tasks()
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
impl<R> WorkerProcess for ExpirationWorker<R>
where
    R: TaskRepository + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        "expiration-worker"
    }

    async fn process(&self) -> ProcessResult {
        match self.cleanup_expired_tasks().await {
            Ok(count) => {
                if count > 0 {
                    info!("Cleaned up {} expired tasks", count);
                }
                ProcessResult::Completed
            }
            Err(e) => ProcessResult::Error(format!("Failed to cleanup expired tasks: {}", e)),
        }
    }
}

#[cfg(test)]
#[path = "expiration_worker_test.rs"]
mod tests;
