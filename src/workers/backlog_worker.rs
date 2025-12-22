// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio::time::interval;
use tracing::{error, info, warn};

use crate::domain::repositories::{
    task_repository::TaskRepository, tasks_backlog_repository::TasksBacklogRepository,
};
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::utils::errors::WorkerError;

/// 积压任务处理Worker
///
/// 该Worker负责定期处理积压的任务，当团队并发限制释放时
/// 将积压任务重新加入执行队列
pub struct BacklogWorker {
    tasks_backlog_repository: Arc<dyn TasksBacklogRepository>,
    task_repository: Arc<dyn TaskRepository>,
    rate_limiting_service: Arc<dyn RateLimitingService>,
    process_interval: Duration,
    batch_size: usize,
}

impl BacklogWorker {
    pub fn new(
        tasks_backlog_repository: Arc<dyn TasksBacklogRepository>,
        task_repository: Arc<dyn TaskRepository>,
        rate_limiting_service: Arc<dyn RateLimitingService>,
        process_interval: Duration,
        batch_size: usize,
    ) -> Self {
        Self {
            tasks_backlog_repository,
            task_repository,
            rate_limiting_service,
            process_interval,
            batch_size,
        }
    }

    /// 处理积压任务
    async fn process_backlog(&self) -> Result<(), WorkerError> {
        info!("开始处理积压任务");

        // 1. 获取所有待处理的积压任务
        let pending_backlogs = self
            .tasks_backlog_repository
            .get_pending_tasks(None, Some(self.batch_size as u64))
            .await
            .map_err(|e| WorkerError::RepositoryError(e.to_string()))?;

        if pending_backlogs.is_empty() {
            info!("没有待处理的积压任务");
            return Ok(());
        }

        info!("发现 {} 个待处理的积压任务", pending_backlogs.len());

        let mut processed_count = 0;
        let mut failed_count = 0;
        let mut expired_count = 0;

        // 2. 按团队分组处理任务
        let mut backlogs_by_team: std::collections::HashMap<uuid::Uuid, Vec<_>> =
            std::collections::HashMap::new();
        for backlog in pending_backlogs {
            backlogs_by_team
                .entry(backlog.team_id)
                .or_default()
                .push(backlog);
        }

        // 3. 处理每个团队的积压任务
        for (team_id, team_backlogs) in backlogs_by_team {
            info!("处理团队 {} 的 {} 个积压任务", team_id, team_backlogs.len());

            for backlog in team_backlogs {
                match self.process_single_backlog(backlog).await {
                    Ok(true) => processed_count += 1,
                    Ok(false) => {
                        // 任务已过期
                        expired_count += 1;
                    }
                    Err(e) => {
                        error!("处理积压任务失败: {}", e);
                        failed_count += 1;
                    }
                }
            }
        }

        info!(
            "积压任务处理完成: 成功={}, 失败={}, 过期={}",
            processed_count, failed_count, expired_count
        );

        Ok(())
    }

    /// 处理单个积压任务
    async fn process_single_backlog(
        &self,
        backlog: crate::domain::repositories::tasks_backlog_repository::TasksBacklog,
    ) -> Result<bool, WorkerError> {
        // 1. 检查任务是否已过期
        if backlog.is_expired() {
            info!("积压任务 {} 已过期，标记为过期状态", backlog.id);

            let mut expired_backlog = backlog.clone();
            expired_backlog
                .mark_expired()
                .map_err(WorkerError::DomainError)?;

            self.tasks_backlog_repository
                .update(&expired_backlog)
                .await
                .map_err(|e| WorkerError::RepositoryError(e.to_string()))?;

            return Ok(false);
        }

        // 2. 检查是否超过重试次数
        if !backlog.can_retry() {
            warn!("积压任务 {} 重试次数已达上限，标记为失败", backlog.id);

            let mut failed_backlog = backlog.clone();
            failed_backlog
                .mark_failed()
                .map_err(WorkerError::DomainError)?;

            self.tasks_backlog_repository
                .update(&failed_backlog)
                .await
                .map_err(|e| WorkerError::RepositoryError(e.to_string()))?;

            return Ok(false);
        }

        // 3. 检查团队的并发限制
        match self
            .rate_limiting_service
            .check_team_concurrency(backlog.team_id, backlog.task_id)
            .await
        {
            Ok(crate::domain::services::rate_limiting_service::ConcurrencyResult::Allowed) => {
                info!(
                    "团队 {} 并发槽位可用，处理积压任务 {}",
                    backlog.team_id, backlog.id
                );

                // 4. 重新激活任务
                match self.reactivate_task(backlog.clone()).await {
                    Ok(_) => {
                        info!("积压任务 {} 重新激活成功", backlog.id);
                        Ok(true)
                    }
                    Err(e) => {
                        error!("重新激活任务失败: {}", e);

                        // 增加重试次数
                        let mut retry_backlog = backlog.clone();
                        retry_backlog.increment_retry_count();

                        self.tasks_backlog_repository
                            .update(&retry_backlog)
                            .await
                            .map_err(|e| WorkerError::RepositoryError(e.to_string()))?;

                        Err(e)
                    }
                }
            }
            Ok(crate::domain::services::rate_limiting_service::ConcurrencyResult::Denied {
                reason,
            }) => {
                info!(
                    "团队 {} 并发限制未释放: {}，积压任务 {} 继续保持积压状态",
                    backlog.team_id, reason, backlog.id
                );
                Ok(false)
            }
            Ok(crate::domain::services::rate_limiting_service::ConcurrencyResult::Queued {
                ..
            }) => {
                // 这种情况不应该发生，因为我们正在处理积压任务
                warn!("积压任务 {} 被重新排队，这是意外的行为", backlog.id);
                Ok(false)
            }
            Err(e) => {
                error!("检查团队并发限制失败: {}", e);
                Err(WorkerError::ServiceError(e.to_string()))
            }
        }
    }

    /// 重新激活任务
    async fn reactivate_task(
        &self,
        backlog: crate::domain::repositories::tasks_backlog_repository::TasksBacklog,
    ) -> Result<(), WorkerError> {
        // 1. 查找原始任务
        let task = self
            .task_repository
            .find_by_id(backlog.task_id)
            .await
            .map_err(|e| WorkerError::RepositoryError(e.to_string()))?
            .ok_or_else(|| WorkerError::NotFound(format!("任务 {} 不存在", backlog.task_id)))?;

        // 2. 检查任务状态
        if task.status != crate::domain::models::task::TaskStatus::Queued {
            info!("任务 {} 状态为 {}，不需要重新激活", task.id, task.status);

            // 标记积压任务为已完成
            let mut completed_backlog = backlog.clone();
            completed_backlog
                .mark_completed()
                .map_err(|e| WorkerError::DomainError(e.to_string()))?;

            self.tasks_backlog_repository
                .update(&completed_backlog)
                .await
                .map_err(|e| WorkerError::RepositoryError(e.to_string()))?;

            return Ok(());
        }

        // 3. 更新任务状态为queued，准备重新执行
        let mut updated_task = task.clone();
        updated_task.status = crate::domain::models::task::TaskStatus::Queued;
        updated_task.updated_at = Utc::now().into();

        self.task_repository
            .update(&updated_task)
            .await
            .map_err(|e| WorkerError::RepositoryError(e.to_string()))?;

        // 4. 标记积压任务为已完成
        let mut completed_backlog = backlog.clone();
        completed_backlog
            .mark_completed()
            .map_err(WorkerError::DomainError)?;

        self.tasks_backlog_repository
            .update(&completed_backlog)
            .await
            .map_err(|e| WorkerError::RepositoryError(e.to_string()))?;

        info!("任务 {} 重新激活成功", task.id);
        Ok(())
    }

    /// 清理过期任务
    async fn cleanup_expired_tasks(&self) -> Result<(), WorkerError> {
        info!("开始清理过期积压任务");

        let expired_backlogs = self
            .tasks_backlog_repository
            .get_expired_tasks(Some(100))
            .await
            .map_err(|e| WorkerError::RepositoryError(e.to_string()))?;

        if expired_backlogs.is_empty() {
            info!("没有过期的积压任务");
            return Ok(());
        }

        let mut cleaned_count = 0;

        for backlog in expired_backlogs {
            match self.process_expired_backlog(backlog).await {
                Ok(_) => cleaned_count += 1,
                Err(e) => {
                    error!("清理过期积压任务失败: {}", e);
                }
            }
        }

        info!("清理过期积压任务完成，共清理 {} 个任务", cleaned_count);
        Ok(())
    }

    /// 处理过期积压任务
    async fn process_expired_backlog(
        &self,
        backlog: crate::domain::repositories::tasks_backlog_repository::TasksBacklog,
    ) -> Result<(), WorkerError> {
        info!("处理过期积压任务 {}", backlog.id);

        // 1. 标记积压任务为过期
        let mut expired_backlog = backlog.clone();
        expired_backlog
            .mark_expired()
            .map_err(WorkerError::DomainError)?;

        self.tasks_backlog_repository
            .update(&expired_backlog)
            .await
            .map_err(|e| WorkerError::RepositoryError(e.to_string()))?;

        // 2. 查找对应的任务
        let task = self
            .task_repository
            .find_by_id(backlog.task_id)
            .await
            .map_err(|e| WorkerError::RepositoryError(e.to_string()))?;

        if let Some(task) = task {
            // 3. 如果任务还在pending状态，标记为失败
            if task.status == crate::domain::models::task::TaskStatus::Queued {
                let mut failed_task = task.clone();
                failed_task.status = crate::domain::models::task::TaskStatus::Failed;
                failed_task.updated_at = Utc::now().into();

                self.task_repository
                    .update(&failed_task)
                    .await
                    .map_err(|e| WorkerError::RepositoryError(e.to_string()))?;

                info!("任务 {} 因积压过期被标记为失败", task.id);
            }
        }

        Ok(())
    }
}

#[async_trait]
impl crate::workers::Worker for BacklogWorker {
    async fn run(&self) -> Result<(), WorkerError> {
        info!("积压任务处理Worker启动");

        let mut interval = interval(self.process_interval);

        loop {
            interval.tick().await;

            // 处理积压任务
            if let Err(e) = self.process_backlog().await {
                error!("处理积压任务时发生错误: {}", e);
            }

            // 定期清理过期任务（每10个周期清理一次）
            static CLEANUP_COUNTER: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let counter = CLEANUP_COUNTER.fetch_add(1, Ordering::SeqCst);
            if counter.is_multiple_of(10) {
                if let Err(e) = self.cleanup_expired_tasks().await {
                    error!("清理过期积压任务时发生错误: {}", e);
                }
            }
        }
    }

    fn name(&self) -> &str {
        "backlog-worker"
    }
}
