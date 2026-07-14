// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use super::{Priority, QueueTaskType, TaskMessage};
use crate::domain::models::{Task, TaskType as DomainTaskType};
use crate::domain::repositories::task_repository::TaskRepository;
use async_trait::async_trait;
use chrono::Utc;
use log::debug;
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
    /// 取消任务
    async fn cancel(&self, task_id: Uuid) -> Result<(), QueueError>;
}

/// PostgreSQL任务队列实现
pub struct PostgresTaskQueue {
    /// 任务仓库
    pub repository: Arc<dyn TaskRepository>,
}

impl PostgresTaskQueue {
    /// 创建新的PostgreSQL任务队列实例
    ///
    /// # 参数
    ///
    /// * `repository` - 任务仓库
    ///
    /// # 返回值
    ///
    /// 返回新的PostgreSQL任务队列实例
    pub fn new(repository: Arc<dyn TaskRepository>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl TaskQueue for PostgresTaskQueue {
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
        debug!("worker_id={}", worker_id);
        let task = self.repository.acquire_next(worker_id).await?;
        debug!("has_task={:?}", task.is_some());
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

    /// 取消任务
    ///
    /// # 参数
    ///
    /// * `task_id` - 任务ID
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功
    /// * `Err(QueueError)` - 失败
    async fn cancel(&self, task_id: Uuid) -> Result<(), QueueError> {
        self.repository.mark_cancelled(task_id).await?;
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

    async fn cancel(&self, task_id: Uuid) -> Result<(), QueueError> {
        (**self).cancel(task_id).await
    }
}

// =========================================================================
// Phase 5 T019: Task ↔ TaskMessage 适配器（领域层与队列层互转）
// =========================================================================

/// 领域 i32 优先级（1-10）→ 队列 Priority 枚举
fn priority_from_domain(p: i32) -> Priority {
    match p {
        1..=3 => Priority::Low,
        4..=6 => Priority::Normal,
        7..=8 => Priority::High,
        _ => Priority::Critical, // 9-10 及越界值
    }
}

/// 队列 Priority 枚举 → 领域 i32 优先级
fn priority_to_domain(p: Priority) -> i32 {
    match p {
        Priority::Low => 1,
        Priority::Normal => 5,
        Priority::High => 8,
        Priority::Critical => 10,
    }
}

/// 领域 TaskType（Scrape/Crawl/Extract）→ 队列 QueueTaskType
fn task_type_to_queue(tt: DomainTaskType) -> QueueTaskType {
    match tt {
        DomainTaskType::Scrape => QueueTaskType::Scrape,
        DomainTaskType::Crawl => QueueTaskType::Custom("crawl".to_string()),
        DomainTaskType::Extract => QueueTaskType::Custom("extract".to_string()),
    }
}

/// 队列 QueueTaskType → 领域 TaskType（Webhook/Cleanup/未知 Custom 回退为 Scrape）
fn task_type_from_queue(qt: &QueueTaskType) -> DomainTaskType {
    match qt {
        QueueTaskType::Scrape => DomainTaskType::Scrape,
        QueueTaskType::Webhook => DomainTaskType::Scrape,
        QueueTaskType::Export => DomainTaskType::Extract,
        QueueTaskType::Cleanup => DomainTaskType::Scrape,
        QueueTaskType::Custom(s) => match s.as_str() {
            "crawl" => DomainTaskType::Crawl,
            "extract" => DomainTaskType::Extract,
            "scrape" => DomainTaskType::Scrape,
            _ => DomainTaskType::Scrape,
        },
    }
}

/// 领域 Task → 队列 TaskMessage
///
/// 将领域层任务转换为队列消息。scheduled_at 为 None 时使用当前时间。
/// team_id、api_key_id、url 等领域字段不携带到 TaskMessage（载荷在 payload 中）。
impl From<Task> for TaskMessage {
    fn from(task: Task) -> Self {
        TaskMessage {
            id: task.id,
            task_type: task_type_to_queue(task.task_type),
            payload: task.payload,
            priority: priority_from_domain(task.priority),
            retry_count: task.retry_count.max(0) as u32,
            max_retries: task.max_retries.max(0) as u32,
            scheduled_at: task.scheduled_at.unwrap_or_else(Utc::now),
        }
    }
}

/// 队列 TaskMessage → 领域 Task
///
/// 将队列消息转换回领域任务。team_id、api_key_id、url 为默认值（Uuid::nil / 空串），
/// 这些字段不在 TaskMessage 中，需要从外部上下文补充。
impl From<TaskMessage> for Task {
    fn from(msg: TaskMessage) -> Self {
        let task_type = task_type_from_queue(&msg.task_type);
        let priority = priority_to_domain(msg.priority);
        let retry_count = msg.retry_count as i32;
        let max_retries = msg.max_retries as i32;
        let scheduled_at = Some(msg.scheduled_at);

        let mut task = Task::new(
            msg.id,
            task_type,
            Uuid::nil(),
            Uuid::nil(),
            String::new(),
            msg.payload,
        );
        task.priority = priority;
        task.retry_count = retry_count;
        task.max_retries = max_retries;
        task.scheduled_at = scheduled_at;
        task
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::*;
    use crate::domain::models::{TaskStatus, TaskType};

    /// 构造测试 Task
    fn make_task(task_type: TaskType, priority: i32, retry_count: i32) -> Task {
        let mut task = Task::new(
            Uuid::new_v4(),
            task_type,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({"key": "value"}),
        );
        task.priority = priority;
        task.retry_count = retry_count;
        task.max_retries = 5;
        task
    }

    #[test]
    fn test_task_to_message_preserves_core_fields() {
        let id = Uuid::new_v4();
        let mut task = make_task(TaskType::Scrape, 7, 2);
        task.id = id;

        let msg: TaskMessage = task.into();

        assert_eq!(msg.id, id);
        assert_eq!(msg.task_type, QueueTaskType::Scrape);
        assert_eq!(msg.priority, Priority::High);
        assert_eq!(msg.retry_count, 2);
        assert_eq!(msg.max_retries, 5);
    }

    #[test]
    fn test_task_type_mapping_round_trip() {
        // Scrape → Scrape → Scrape
        let task = make_task(TaskType::Scrape, 5, 0);
        let msg: TaskMessage = task.into();
        assert_eq!(msg.task_type, QueueTaskType::Scrape);

        // Crawl → Custom("crawl") → Crawl
        let task = make_task(TaskType::Crawl, 5, 0);
        let msg: TaskMessage = task.into();
        assert_eq!(msg.task_type, QueueTaskType::Custom("crawl".to_string()));
        let back: Task = msg.into();
        assert_eq!(back.task_type, TaskType::Crawl);

        // Extract → Custom("extract") → Extract
        let task = make_task(TaskType::Extract, 5, 0);
        let msg: TaskMessage = task.into();
        assert_eq!(msg.task_type, QueueTaskType::Custom("extract".to_string()));
        let back: Task = msg.into();
        assert_eq!(back.task_type, TaskType::Extract);
    }

    #[test]
    fn test_priority_mapping_all_levels() {
        let cases = [
            (1, Priority::Low),
            (3, Priority::Low),
            (4, Priority::Normal),
            (6, Priority::Normal),
            (7, Priority::High),
            (8, Priority::High),
            (9, Priority::Critical),
            (10, Priority::Critical),
        ];

        for (i32_val, expected) in cases {
            let task = make_task(TaskType::Scrape, i32_val, 0);
            let msg: TaskMessage = task.into();
            assert_eq!(
                msg.priority, expected,
                "priority {} should map to {:?}",
                i32_val, expected
            );
        }
    }

    #[test]
    fn test_message_to_task_defaults() {
        let msg = TaskMessage::new(
            Uuid::new_v4(),
            QueueTaskType::Scrape,
            serde_json::json!({"data": 1}),
            Priority::Critical,
            3,
        );

        let task: Task = msg.into();

        assert_eq!(task.priority, 10, "Critical should map to 10");
        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(task.team_id, Uuid::nil());
        assert_eq!(task.api_key_id, Uuid::nil());
        assert!(task.url.is_empty());
        assert!(task.scheduled_at.is_some());
    }

    #[test]
    fn test_round_trip_preserves_retry_and_max() {
        let mut task = make_task(TaskType::Scrape, 5, 3);
        task.max_retries = 7;

        let msg: TaskMessage = task.into();
        assert_eq!(msg.retry_count, 3);
        assert_eq!(msg.max_retries, 7);

        let back: Task = msg.into();
        assert_eq!(back.retry_count, 3);
        assert_eq!(back.max_retries, 7);
    }
}
