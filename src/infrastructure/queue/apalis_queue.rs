// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Apalis 队列实现（Phase 5 工业化集成）
//!
//! ## 设计说明
//!
//! apalis 0.5 的 `PostgresStorage` 是面向 worker 框架的 `Backend` trait 实现，
//! 不是通用的消息队列（无 ack/nack/dead_letter 语义）。其 API 设计围绕
//! `WorkerBuilder` + `Monitor` + `Service` 的工作流，且需要 `sqlx::PgPool`。
//!
//! 本项目使用 dbnexus 管理数据库连接池（基于 Sea-ORM），不直接持有 sqlx PgPool。
//! 将 apalis PostgresStorage 直接适配为 MessageQueue 需要：
//! 1. 从 dbnexus 提取底层 sqlx PgPool（当前未暴露）
//! 2. 运行 `PostgresStorage::setup()` 创建 apalis 专用的 job 表
//! 3. 为 TaskMessage 实现 apalis 的 `Job` trait 和 `Storage` trait 适配
//!
//! 上述集成复杂度高且需要运行时 PostgreSQL 验证，延后到 Phase 11（DI 注册）后处理。
//! 当前 ApalisQueue 封装 `InMemoryQueue` 作为 fallback，保证 MessageQueue trait 可用。
//!
//! 后续集成路径：
//! - 方案 A：从 dbnexus 连接池提取 sqlx::PgPool，直接使用 PostgresStorage
//! - 方案 B：实现自定义 Storage 适配器，基于 dbnexus 的 Sea-ORM 连接

use async_trait::async_trait;
use log::debug;
use uuid::Uuid;

use crate::queue::{InMemoryQueue, MessageQueue, QueueError, TaskMessage};

/// Apalis 队列（封装 InMemoryQueue 作为 fallback）
///
/// 实现 `MessageQueue` trait，当前后端为 `InMemoryQueue`。
/// 后续 apalis PostgresStorage 集成完成后，可切换后端实现。
#[derive(Debug, Default)]
pub struct ApalisQueue {
    /// 内部队列后端（当前为 InMemoryQueue）
    inner: InMemoryQueue,
}

impl ApalisQueue {
    /// 创建新的 ApalisQueue 实例
    pub fn new() -> Self {
        Self::default()
    }

    /// 从已有的 InMemoryQueue 构造（用于测试或复用）
    pub fn with_backend(inner: InMemoryQueue) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl MessageQueue for ApalisQueue {
    async fn enqueue(&self, msg: TaskMessage) -> Result<TaskMessage, QueueError> {
        debug!("ApalisQueue::enqueue task_id={}", msg.id);
        self.inner.enqueue(msg).await
    }

    async fn dequeue(&self, worker_id: Uuid) -> Result<Option<TaskMessage>, QueueError> {
        debug!("ApalisQueue::dequeue worker_id={}", worker_id);
        self.inner.dequeue(worker_id).await
    }

    async fn ack(&self, task_id: Uuid) -> Result<(), QueueError> {
        debug!("ApalisQueue::ack task_id={}", task_id);
        self.inner.ack(task_id).await
    }

    async fn nack(&self, task_id: Uuid, reason: String) -> Result<(), QueueError> {
        debug!("ApalisQueue::nack task_id={} reason={}", task_id, reason);
        self.inner.nack(task_id, reason).await
    }

    async fn dead_letter(&self, task_id: Uuid, reason: String) -> Result<(), QueueError> {
        debug!(
            "ApalisQueue::dead_letter task_id={} reason={}",
            task_id, reason
        );
        self.inner.dead_letter(task_id, reason).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::{Priority, QueueTaskType};

    /// 构造测试消息
    fn make_msg(id: Uuid, priority: Priority, max_retries: u32) -> TaskMessage {
        TaskMessage::new(
            id,
            QueueTaskType::Scrape,
            serde_json::json!({"url": "https://example.com"}),
            priority,
            max_retries,
        )
    }

    #[tokio::test]
    async fn test_apalis_queue_enqueue_dequeue() {
        let queue = ApalisQueue::new();
        let id = Uuid::new_v4();

        queue
            .enqueue(make_msg(id, Priority::Normal, 3))
            .await
            .unwrap();

        let dequeued = queue.dequeue(Uuid::new_v4()).await.unwrap();
        assert!(dequeued.is_some());
        assert_eq!(dequeued.unwrap().id, id);
    }

    #[tokio::test]
    async fn test_apalis_queue_priority_order() {
        let queue = ApalisQueue::new();
        let low_id = Uuid::new_v4();
        let high_id = Uuid::new_v4();

        queue
            .enqueue(make_msg(low_id, Priority::Low, 3))
            .await
            .unwrap();
        queue
            .enqueue(make_msg(high_id, Priority::High, 3))
            .await
            .unwrap();

        let d1 = queue.dequeue(Uuid::new_v4()).await.unwrap().unwrap();
        assert_eq!(d1.id, high_id, "High priority should dequeue first");

        let d2 = queue.dequeue(Uuid::new_v4()).await.unwrap().unwrap();
        assert_eq!(d2.id, low_id, "Low priority should dequeue second");
    }

    #[tokio::test]
    async fn test_apalis_queue_ack_removes_message() {
        let queue = ApalisQueue::new();
        let id = Uuid::new_v4();

        queue
            .enqueue(make_msg(id, Priority::Normal, 3))
            .await
            .unwrap();
        queue.ack(id).await.unwrap();

        let result = queue.dequeue(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none(), "after ack, dequeue should return None");
    }

    #[tokio::test]
    async fn test_apalis_queue_nack_increments_retry() {
        let queue = ApalisQueue::new();
        let id = Uuid::new_v4();

        queue
            .enqueue(make_msg(id, Priority::Normal, 3))
            .await
            .unwrap();
        queue.dequeue(Uuid::new_v4()).await.unwrap().unwrap();
        queue.nack(id, "fail".to_string()).await.unwrap();

        let d = queue.dequeue(Uuid::new_v4()).await.unwrap().unwrap();
        assert_eq!(d.retry_count, 1, "retry_count should be 1 after nack");
    }

    #[tokio::test]
    async fn test_apalis_queue_dead_letter() {
        let queue = ApalisQueue::new();
        let id = Uuid::new_v4();

        // max_retries=0: nack 一次后 retry_count=1 > 0 → 死信
        queue
            .enqueue(make_msg(id, Priority::Normal, 0))
            .await
            .unwrap();
        queue.dequeue(Uuid::new_v4()).await.unwrap().unwrap();
        queue.nack(id, "exceeded".to_string()).await.unwrap();

        let result = queue.dequeue(Uuid::new_v4()).await.unwrap();
        assert!(
            result.is_none(),
            "dead-lettered message should not be dequeued"
        );
    }
}
