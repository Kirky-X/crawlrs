// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::task::Task;
use crate::domain::repositories::task_repository::TaskRepository;
use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

/// 队列错误类型
#[derive(Error, Debug)]
pub enum QueueError {
    /// 仓库错误
    #[error("Repository error: {0}")]
    Repository(#[from] crate::domain::repositories::task_repository::RepositoryError),

    /// 队列为空
    #[error("Queue empty")]
    Empty,
}

/// 任务队列特质
#[async_trait]
pub trait TaskQueue: Send + Sync {
    /// 入队任务
    async fn enqueue(&self, task: Task) -> Result<Task, QueueError>;

    /// 出队任务
    async fn dequeue(&self, worker_id: Uuid) -> Result<Option<Task>, QueueError>;

    /// 完成任务
    async fn complete(&self, task_id: Uuid) -> Result<(), QueueError>;
    /// 失败任务
    async fn fail(&self, task_id: Uuid) -> Result<(), QueueError>;
}

/// PostgreSQL任务队列实现
pub struct PostgresTaskQueue<R: TaskRepository> {
    /// 任务仓库
    repository: Arc<R>,
}

impl<R: TaskRepository> PostgresTaskQueue<R> {
    /// 创建新的PostgreSQL任务队列实例
    ///
    /// # 参数
    ///
    /// * `repository` - 任务仓库
    ///
    /// # 返回值
    ///
    /// 返回新的PostgreSQL任务队列实例
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<R: TaskRepository> TaskQueue for PostgresTaskQueue<R> {
    /// 入队任务
    ///
    /// # 参数
    ///
    /// * `task` - 要入队的任务
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 入队成功的任务
    /// * `Err(QueueError)` - 入队失败
    async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
        let created = self.repository.create(&task).await?;
        Ok(created)
    }

    /// 出队任务
    ///
    /// # 参数
    ///
    /// * `worker_id` - 工作者ID
    ///
    /// # 返回值
    ///
    /// * `Ok(Some(Task))` - 成功出队的任务
    /// * `Ok(None)` - 没有可出队的任务
    /// * `Err(QueueError)` - 出队失败
    async fn dequeue(&self, worker_id: Uuid) -> Result<Option<Task>, QueueError> {
        let task = self.repository.acquire_next(worker_id).await?;
        Ok(task)
    }

    /// 完成任务
    ///
    /// # 参数
    ///
    /// * `task_id` - 任务ID
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功
    /// * `Err(QueueError)` - 失败
    async fn complete(&self, task_id: Uuid) -> Result<(), QueueError> {
        self.repository.mark_completed(task_id).await?;
        Ok(())
    }

    /// 失败任务
    ///
    /// # 参数
    ///
    /// * `task_id` - 任务ID
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功
    /// * `Err(QueueError)` - 失败
    async fn fail(&self, task_id: Uuid) -> Result<(), QueueError> {
        self.repository.mark_failed(task_id).await?;
        Ok(())
    }
}

#[async_trait]
impl<T: TaskQueue + ?Sized> TaskQueue for Arc<T> {
    async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
        (**self).enqueue(task).await
    }

    async fn dequeue(&self, worker_id: Uuid) -> Result<Option<Task>, QueueError> {
        (**self).dequeue(worker_id).await
    }

    async fn complete(&self, task_id: Uuid) -> Result<(), QueueError> {
        (**self).complete(task_id).await
    }

    async fn fail(&self, task_id: Uuid) -> Result<(), QueueError> {
        (**self).fail(task_id).await
    }
}
