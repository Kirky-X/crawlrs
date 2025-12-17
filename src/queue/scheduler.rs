// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::task::{Task, TaskStatus};
use crate::domain::repositories::task_repository::TaskRepository;
use crate::queue::task_queue::QueueError;
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration as TokioDuration};
use tracing::{error, info};

/// 任务调度器
pub struct TaskScheduler<R: TaskRepository + Send + Sync + 'static> {
    /// 任务仓库
    repository: Arc<R>,
}

impl<R: TaskRepository + Send + Sync + 'static> TaskScheduler<R> {
    /// 创建新的任务调度器实例
    ///
    /// # 参数
    ///
    /// * `repository` - 任务仓库
    ///
    /// # 返回值
    ///
    /// 返回新的任务调度器实例
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    /// 启动调度器后台任务
    ///
    /// 这里的调度器目前主要负责清理过期任务或处理卡住的任务等维护工作
    /// 实际的任务调度（获取任务）由Worker通过acquire_next主动拉取
    ///
    /// # 返回值
    ///
    /// 返回后台任务的句柄
    pub fn start(&self) -> JoinHandle<()> {
        let repository = self.repository.clone();

        tokio::spawn(async move {
            let mut interval = interval(TokioDuration::from_secs(60)); // 每分钟检查一次

            loop {
                interval.tick().await;

                // 这里可以添加定期维护逻辑
                // 例如：重置长时间处于Active状态但未更新心跳的任务
                // 目前TaskRepository接口还没有暴露相关方法，这里作为预留扩展点
                match repository.reset_stuck_tasks(Duration::minutes(30)).await {
                    Ok(count) => {
                        if count > 0 {
                            info!("Reset {} stuck tasks", count);
                        }
                    }
                    Err(e) => {
                        error!("Failed to reset stuck tasks: {}", e);
                    }
                }

                match repository.expire_tasks().await {
                    Ok(count) => {
                        if count > 0 {
                            info!("Expired {} tasks", count);
                        }
                    }
                    Err(e) => {
                        error!("Failed to expire tasks: {}", e);
                    }
                }

                info!("Scheduler maintenance tick");
            }
        })
    }

    /// 在特定时间调度任务执行
    ///
    /// # 参数
    ///
    /// * `task` - 要调度的任务
    /// * `time` - 执行时间
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 调度成功的任务
    /// * `Err(QueueError)` - 调度失败
    pub async fn schedule_at(
        &self,
        mut task: Task,
        time: DateTime<Utc>,
    ) -> Result<Task, QueueError> {
        task.scheduled_at = Some(time.into());
        task.status = TaskStatus::Queued;

        // Ensure created_at is set if not already (Task::new sets it, but good to be safe)
        if task.created_at.timestamp() == 0 {
            task.created_at = Utc::now().into();
        }

        let created = self.repository.create(&task).await?;
        Ok(created)
    }

    /// 在一段时间后调度任务执行
    ///
    /// # 参数
    ///
    /// * `task` - 要调度的任务
    /// * `duration` - 延迟时间
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 调度成功的任务
    /// * `Err(QueueError)` - 调度失败
    pub async fn schedule_in(&self, task: Task, duration: Duration) -> Result<Task, QueueError> {
        let time = Utc::now() + duration;
        self.schedule_at(task, time).await
    }

    /// 重新调度失败的任务进行重试，支持指数退避或固定延迟
    ///
    /// # 参数
    ///
    /// * `task` - 需要重试的任务
    /// * `delay` - 重试延迟时间
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 重调度后的任务
    /// * `Err(QueueError)` - 重调度失败
    pub async fn reschedule_retry(
        &self,
        mut task: Task,
        delay: Duration,
    ) -> Result<Task, QueueError> {
        if !task.can_retry() {
            // If cannot retry, mark as failed permanently
            task.status = TaskStatus::Failed;
            task.completed_at = Some(Utc::now().into());
            let updated = self.repository.update(&task).await?;
            return Ok(updated);
        }

        task.status = TaskStatus::Queued;
        task.attempt_count += 1;
        task.scheduled_at = Some((Utc::now() + delay).into());
        task.started_at = None; // Reset started_at as it's queued again
        task.completed_at = None;

        let updated = self.repository.update(&task).await?;
        Ok(updated)
    }

    /// 以高优先级调度任务
    ///
    /// # 参数
    ///
    /// * `task` - 需要调度的任务
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 调度后的任务
    /// * `Err(QueueError)` - 调度失败
    ///
    /// # 说明
    ///
    /// 将任务优先级设置为100（高优先级），并立即调度执行
    pub async fn schedule_urgent(&self, mut task: Task) -> Result<Task, QueueError> {
        task.priority = 100; // Assuming 100 is high priority
        task.scheduled_at = Some(Utc::now().into()); // Immediate
        task.status = TaskStatus::Queued;

        let created = self.repository.create(&task).await?;
        Ok(created)
    }
}
