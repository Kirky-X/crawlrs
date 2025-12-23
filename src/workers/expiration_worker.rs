use crate::domain::repositories::task_repository::TaskRepository;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{error, info};
use std::time::Duration;

/// 任务过期清理工作器
///
/// 负责定期扫描并清理过期的任务
pub struct ExpirationWorker<R>
where
    R: TaskRepository + Send + Sync + 'static,
{
    repository: Arc<R>,
    interval: Duration,
}

impl<R> ExpirationWorker<R>
where
    R: TaskRepository + Send + Sync + 'static,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self {
            repository,
            interval: Duration::from_secs(60 * 60), // 每小时运行一次
        }
    }

    /// 运行工作器
    pub async fn run(&self) {
        info!("Task expiration worker started");

        let mut interval = tokio::time::interval(self.interval);

        loop {
            interval.tick().await;

            match self.cleanup_expired_tasks().await {
                Ok(count) => {
                    if count > 0 {
                        info!("Cleaned up {} expired tasks", count);
                    }
                }
                Err(e) => {
                    error!("Failed to cleanup expired tasks: {}", e);
                }
            }
        }
    }

    /// 启动后台运行
    pub fn start(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    async fn cleanup_expired_tasks(&self) -> Result<u64, String> {
        self.repository
            .expire_tasks()
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
#[path = "expiration_worker_test.rs"]
mod tests;
