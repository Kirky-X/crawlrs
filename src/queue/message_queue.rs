// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! pgmq 风格消息队列 — 基于 dbnexus 数据库层实现
//!
//! 设计灵感来自 [pgmq](https://github.com/pgmq/pgmq)，通过 trait 抽象存储层，
//! 在现有 dbnexus 数据库层实现轻量级消息队列。
//!
//! ## 架构
//!
//! - `MessageQueueRepository` trait：抽象所有数据库操作，不同后端各自实现
//! - `MessageQueue` trait：pgmq 风格公开 API（send / read / delete / archive）
//! - `DbMessageQueue`：通用实现，仅通过 `MessageQueueRepository` 操作存储，
//!   不直接接触 SQL，可搭配任意后端（PostgreSQL、SQLite 等）
//!
//! ## 核心语义
//!
//! - **命名队列**：通过 `queue_name` 区分不同队列，共享同一张物理表
//! - **Visibility Timeout**：消息被 `read` 后在指定时间内对其他消费者不可见，
//!   超时未 `delete`/`archive` 的消息自动重新可用（至少一次投递保证）
//! - **原子读取**：由 repository 实现保证并发安全（如 PG 使用 `FOR UPDATE SKIP LOCKED`）
//! - **归档**：`archive` 将消息移入归档表而非删除，支持回放和审计
//!
//! ## 与现有 TaskQueue 的关系
//!
//! `TaskQueue` 面向 Task 域模型（enqueue/dequeue/complete/fail/cancel），
//! 与 Task 生命周期强耦合。`MessageQueue` 是通用消息队列抽象，
//! 面向 JSON 消息，提供 visibility timeout、batch 操作、归档等 pgmq 语义。
//! 两者互补，不互相替代。

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::debug;
use std::sync::Arc;
use thiserror::Error;

/// 消息队列错误类型
#[derive(Error, Debug)]
pub enum MessageQueueError {
    /// 数据库错误
    #[error("Database error: {0}")]
    Database(String),

    /// 队列不存在
    #[error("Queue not found: {0}")]
    QueueNotFound(String),

    /// 消息不存在
    #[error("Message not found: msg_id={0}")]
    MessageNotFound(i64),

    /// 序列化/反序列化错误
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// 从队列读取的消息
///
/// 包含消息内容和元数据。`msg_id` 用于后续的 `delete`/`archive` 操作。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Message {
    /// 消息唯一 ID（队列内自增）
    pub msg_id: i64,
    /// 队列名称
    pub queue_name: String,
    /// 消息负载（JSON）
    pub message: serde_json::Value,
    /// 消息被读取的次数
    pub read_ct: i32,
    /// 消息入队时间
    pub enqueued_at: DateTime<Utc>,
}

/// 消息队列存储层 trait — 抽象所有数据库操作
///
/// 不同的数据库后端（PostgreSQL、SQLite 等）各自实现此 trait，
/// 提供各自的 SQL 方言和并发策略。
///
/// `DbMessageQueue` 仅通过此 trait 操作存储，不直接接触 SQL。
#[async_trait]
pub trait MessageQueueRepository: Send + Sync {
    /// 插入一条消息，返回分配的 msg_id
    async fn send(
        &self,
        queue_name: &str,
        message: &serde_json::Value,
    ) -> Result<i64, MessageQueueError>;

    /// 批量插入消息，返回每条消息的 msg_id（顺序与输入一致）
    async fn send_batch(
        &self,
        queue_name: &str,
        messages: &[serde_json::Value],
    ) -> Result<Vec<i64>, MessageQueueError>;

    /// 原子读取并锁定最多 `batch_size` 条消息
    ///
    /// 实现须保证：
    /// 1. 只返回 vt 已过期（或为 NULL）且未归档的消息
    /// 2. 按 msg_id 升序（FIFO）
    /// 3. 并发安全——跳过已被其他消费者锁定的行
    /// 4. 返回的消息 vt 已更新为 `NOW() + vt seconds`，`read_ct` 已 +1
    async fn read_batch(
        &self,
        queue_name: &str,
        vt: i32,
        batch_size: i32,
    ) -> Result<Vec<Message>, MessageQueueError>;

    /// 按 msg_id 删除消息（永久移除），返回是否实际删除
    async fn delete(&self, queue_name: &str, msg_id: i64) -> Result<bool, MessageQueueError>;

    /// 批量删除消息，返回实际删除数量
    async fn delete_batch(
        &self,
        queue_name: &str,
        msg_ids: &[i64],
    ) -> Result<u64, MessageQueueError>;

    /// 归档单条消息（移入归档表），返回是否实际归档
    async fn archive(&self, queue_name: &str, msg_id: i64) -> Result<bool, MessageQueueError>;

    /// 批量归档消息，返回实际归档数量
    async fn archive_batch(
        &self,
        queue_name: &str,
        msg_ids: &[i64],
    ) -> Result<u64, MessageQueueError>;
}

/// 消息队列 trait — pgmq 风格 API
///
/// 提供命名队列的消息发送、读取、删除、归档操作。
/// 具体存储操作委托给 `MessageQueueRepository`。
#[async_trait]
pub trait MessageQueue: Send + Sync {
    /// 发送消息到指定队列
    ///
    /// 队列不存在时自动创建（幂等）。返回消息的 `msg_id`。
    async fn send(
        &self,
        queue_name: &str,
        message: &serde_json::Value,
    ) -> Result<i64, MessageQueueError>;

    /// 批量发送消息
    ///
    /// 返回每条消息的 `msg_id` 列表（顺序与输入一致）。
    async fn send_batch(
        &self,
        queue_name: &str,
        messages: &[serde_json::Value],
    ) -> Result<Vec<i64>, MessageQueueError>;

    /// 读取一条消息（带 visibility timeout）
    ///
    /// 消息被读取后在 `vt` 秒内对其他消费者不可见。
    /// 如果消费者在 `vt` 秒内未调用 `delete`/`archive`，
    /// 消息将自动重新可用（至少一次投递）。
    ///
    /// 返回 `None` 表示队列为空或所有消息均被锁定。
    async fn read(&self, queue_name: &str, vt: i32) -> Result<Option<Message>, MessageQueueError>;

    /// 批量读取消息
    ///
    /// 最多读取 `batch_size` 条消息，每条消息独立设置 visibility timeout。
    async fn read_batch(
        &self,
        queue_name: &str,
        vt: i32,
        batch_size: i32,
    ) -> Result<Vec<Message>, MessageQueueError>;

    /// 删除消息（永久移除）
    ///
    /// 返回 `true` 表示删除成功，`false` 表示消息不存在。
    async fn delete(&self, queue_name: &str, msg_id: i64) -> Result<bool, MessageQueueError>;

    /// 批量删除消息
    ///
    /// 返回实际删除的消息数量。
    async fn delete_batch(
        &self,
        queue_name: &str,
        msg_ids: &[i64],
    ) -> Result<u64, MessageQueueError>;

    /// 归档消息（移入归档表，支持回放）
    ///
    /// 返回 `true` 表示归档成功，`false` 表示消息不存在。
    async fn archive(&self, queue_name: &str, msg_id: i64) -> Result<bool, MessageQueueError>;

    /// 批量归档消息
    ///
    /// 返回实际归档的消息数量。
    async fn archive_batch(
        &self,
        queue_name: &str,
        msg_ids: &[i64],
    ) -> Result<u64, MessageQueueError>;
}

/// 基于 `MessageQueueRepository` 的通用消息队列实现
///
/// 不直接接触 SQL，所有存储操作委托给注入的 repository。
/// 搭配不同 repository 实现即可支持不同数据库后端。
///
/// # 线程安全
///
/// 内部持有 `Arc<dyn MessageQueueRepository>`，可安全在多个 task/线程间共享。
pub struct DbMessageQueue {
    repo: Arc<dyn MessageQueueRepository>,
}

impl DbMessageQueue {
    /// 创建新的消息队列实例
    pub fn new(repo: Arc<dyn MessageQueueRepository>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl MessageQueue for DbMessageQueue {
    async fn send(
        &self,
        queue_name: &str,
        message: &serde_json::Value,
    ) -> Result<i64, MessageQueueError> {
        let msg_id = self.repo.send(queue_name, message).await?;
        debug!(
            "message_queue: sent msg_id={} to queue={}",
            msg_id, queue_name
        );
        Ok(msg_id)
    }

    async fn send_batch(
        &self,
        queue_name: &str,
        messages: &[serde_json::Value],
    ) -> Result<Vec<i64>, MessageQueueError> {
        let ids = self.repo.send_batch(queue_name, messages).await?;
        debug!(
            "message_queue: sent {} messages to queue={}",
            ids.len(),
            queue_name
        );
        Ok(ids)
    }

    async fn read(&self, queue_name: &str, vt: i32) -> Result<Option<Message>, MessageQueueError> {
        let messages = self.read_batch(queue_name, vt, 1).await?;
        Ok(messages.into_iter().next())
    }

    async fn read_batch(
        &self,
        queue_name: &str,
        vt: i32,
        batch_size: i32,
    ) -> Result<Vec<Message>, MessageQueueError> {
        let messages = self.repo.read_batch(queue_name, vt, batch_size).await?;
        if !messages.is_empty() {
            debug!(
                "message_queue: read {} messages from queue={}",
                messages.len(),
                queue_name
            );
        }
        Ok(messages)
    }

    async fn delete(&self, queue_name: &str, msg_id: i64) -> Result<bool, MessageQueueError> {
        let deleted = self.repo.delete(queue_name, msg_id).await?;
        if deleted {
            debug!(
                "message_queue: deleted msg_id={} from queue={}",
                msg_id, queue_name
            );
        }
        Ok(deleted)
    }

    async fn delete_batch(
        &self,
        queue_name: &str,
        msg_ids: &[i64],
    ) -> Result<u64, MessageQueueError> {
        let count = self.repo.delete_batch(queue_name, msg_ids).await?;
        if count > 0 {
            debug!(
                "message_queue: deleted {} messages from queue={}",
                count, queue_name
            );
        }
        Ok(count)
    }

    async fn archive(&self, queue_name: &str, msg_id: i64) -> Result<bool, MessageQueueError> {
        let archived = self.repo.archive(queue_name, msg_id).await?;
        if archived {
            debug!(
                "message_queue: archived msg_id={} from queue={}",
                msg_id, queue_name
            );
        }
        Ok(archived)
    }

    async fn archive_batch(
        &self,
        queue_name: &str,
        msg_ids: &[i64],
    ) -> Result<u64, MessageQueueError> {
        let count = self.repo.archive_batch(queue_name, msg_ids).await?;
        if count > 0 {
            debug!(
                "message_queue: archived {} messages from queue={}",
                count, queue_name
            );
        }
        Ok(count)
    }
}

/// Arc<T> 的 MessageQueue 委托实现
#[async_trait]
impl<T: MessageQueue + ?Sized> MessageQueue for Arc<T> {
    async fn send(
        &self,
        queue_name: &str,
        message: &serde_json::Value,
    ) -> Result<i64, MessageQueueError> {
        (**self).send(queue_name, message).await
    }

    async fn send_batch(
        &self,
        queue_name: &str,
        messages: &[serde_json::Value],
    ) -> Result<Vec<i64>, MessageQueueError> {
        (**self).send_batch(queue_name, messages).await
    }

    async fn read(&self, queue_name: &str, vt: i32) -> Result<Option<Message>, MessageQueueError> {
        (**self).read(queue_name, vt).await
    }

    async fn read_batch(
        &self,
        queue_name: &str,
        vt: i32,
        batch_size: i32,
    ) -> Result<Vec<Message>, MessageQueueError> {
        (**self).read_batch(queue_name, vt, batch_size).await
    }

    async fn delete(&self, queue_name: &str, msg_id: i64) -> Result<bool, MessageQueueError> {
        (**self).delete(queue_name, msg_id).await
    }

    async fn delete_batch(
        &self,
        queue_name: &str,
        msg_ids: &[i64],
    ) -> Result<u64, MessageQueueError> {
        (**self).delete_batch(queue_name, msg_ids).await
    }

    async fn archive(&self, queue_name: &str, msg_id: i64) -> Result<bool, MessageQueueError> {
        (**self).archive(queue_name, msg_id).await
    }

    async fn archive_batch(
        &self,
        queue_name: &str,
        msg_ids: &[i64],
    ) -> Result<u64, MessageQueueError> {
        (**self).archive_batch(queue_name, msg_ids).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

    /// Mock MessageQueueRepository 用于单元测试
    struct MockRepository {
        send_calls: AtomicU64,
        read_calls: AtomicU64,
        delete_calls: AtomicU64,
        archive_calls: AtomicU64,
        next_msg_id: AtomicI64,
    }

    impl MockRepository {
        fn new() -> Self {
            Self {
                send_calls: AtomicU64::new(0),
                read_calls: AtomicU64::new(0),
                delete_calls: AtomicU64::new(0),
                archive_calls: AtomicU64::new(0),
                next_msg_id: AtomicI64::new(1),
            }
        }
    }

    #[async_trait]
    impl MessageQueueRepository for MockRepository {
        async fn send(
            &self,
            _queue_name: &str,
            _message: &serde_json::Value,
        ) -> Result<i64, MessageQueueError> {
            self.send_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.next_msg_id.fetch_add(1, Ordering::SeqCst))
        }

        async fn send_batch(
            &self,
            queue_name: &str,
            messages: &[serde_json::Value],
        ) -> Result<Vec<i64>, MessageQueueError> {
            let mut ids = Vec::with_capacity(messages.len());
            for msg in messages {
                ids.push(self.send(queue_name, msg).await?);
            }
            Ok(ids)
        }

        async fn read_batch(
            &self,
            _queue_name: &str,
            _vt: i32,
            _batch_size: i32,
        ) -> Result<Vec<Message>, MessageQueueError> {
            self.read_calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![])
        }

        async fn delete(&self, _queue_name: &str, _msg_id: i64) -> Result<bool, MessageQueueError> {
            self.delete_calls.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }

        async fn delete_batch(
            &self,
            _queue_name: &str,
            msg_ids: &[i64],
        ) -> Result<u64, MessageQueueError> {
            self.delete_calls.fetch_add(1, Ordering::SeqCst);
            Ok(msg_ids.len() as u64)
        }

        async fn archive(
            &self,
            _queue_name: &str,
            _msg_id: i64,
        ) -> Result<bool, MessageQueueError> {
            self.archive_calls.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }

        async fn archive_batch(
            &self,
            _queue_name: &str,
            msg_ids: &[i64],
        ) -> Result<u64, MessageQueueError> {
            self.archive_calls.fetch_add(1, Ordering::SeqCst);
            Ok(msg_ids.len() as u64)
        }
    }

    // ========== Message struct tests ==========

    #[test]
    fn test_message_serialization() {
        let msg = Message {
            msg_id: 42,
            queue_name: "test_queue".to_string(),
            message: serde_json::json!({"key": "value"}),
            read_ct: 3,
            enqueued_at: Utc::now(),
        };

        let json = serde_json::to_string(&msg).expect("serialize should succeed");
        let deserialized: Message =
            serde_json::from_str(&json).expect("deserialize should succeed");

        assert_eq!(deserialized.msg_id, 42);
        assert_eq!(deserialized.queue_name, "test_queue");
        assert_eq!(deserialized.read_ct, 3);
        assert_eq!(deserialized.message, serde_json::json!({"key": "value"}));
    }

    // ========== MessageQueueError tests ==========

    #[test]
    fn test_error_display() {
        let err = MessageQueueError::QueueNotFound("my_queue".to_string());
        assert_eq!(err.to_string(), "Queue not found: my_queue");

        let err = MessageQueueError::MessageNotFound(123);
        assert_eq!(err.to_string(), "Message not found: msg_id=123");

        let err = MessageQueueError::Database("connection lost".to_string());
        assert_eq!(err.to_string(), "Database error: connection lost");
    }

    // ========== DbMessageQueue via MockRepository tests ==========

    #[tokio::test]
    async fn test_send_increments_counter() {
        let repo = Arc::new(MockRepository::new());
        let queue = DbMessageQueue::new(repo.clone());
        let msg_id = queue
            .send("test", &serde_json::json!({"data": 1}))
            .await
            .unwrap();
        assert_eq!(msg_id, 1);
        assert_eq!(repo.send_calls.load(Ordering::SeqCst), 1);

        let msg_id2 = queue
            .send("test", &serde_json::json!({"data": 2}))
            .await
            .unwrap();
        assert_eq!(msg_id2, 2);
        assert_eq!(repo.send_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_send_batch() {
        let repo = Arc::new(MockRepository::new());
        let queue = DbMessageQueue::new(repo.clone());
        let messages = vec![
            serde_json::json!({"a": 1}),
            serde_json::json!({"b": 2}),
            serde_json::json!({"c": 3}),
        ];
        let ids = queue.send_batch("test", &messages).await.unwrap();
        assert_eq!(ids.len(), 3);
        assert_eq!(ids, vec![1, 2, 3]);
        assert_eq!(repo.send_calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_read_returns_none_on_empty() {
        let repo = Arc::new(MockRepository::new());
        let queue = DbMessageQueue::new(repo.clone());
        let result = queue.read("test", 30).await.unwrap();
        assert!(result.is_none());
        assert_eq!(repo.read_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_read_batch_returns_empty() {
        let repo = Arc::new(MockRepository::new());
        let queue = DbMessageQueue::new(repo.clone());
        let result = queue.read_batch("test", 30, 5).await.unwrap();
        assert!(result.is_empty());
        assert_eq!(repo.read_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_delete_returns_true() {
        let repo = Arc::new(MockRepository::new());
        let queue = DbMessageQueue::new(repo.clone());
        let result = queue.delete("test", 1).await.unwrap();
        assert!(result);
        assert_eq!(repo.delete_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_delete_batch() {
        let repo = Arc::new(MockRepository::new());
        let queue = DbMessageQueue::new(repo.clone());
        let count = queue.delete_batch("test", &[1, 2, 3]).await.unwrap();
        assert_eq!(count, 3);
        assert_eq!(repo.delete_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_archive_returns_true() {
        let repo = Arc::new(MockRepository::new());
        let queue = DbMessageQueue::new(repo.clone());
        let result = queue.archive("test", 1).await.unwrap();
        assert!(result);
        assert_eq!(repo.archive_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_archive_batch() {
        let repo = Arc::new(MockRepository::new());
        let queue = DbMessageQueue::new(repo.clone());
        let count = queue.archive_batch("test", &[1, 2]).await.unwrap();
        assert_eq!(count, 2);
        assert_eq!(repo.archive_calls.load(Ordering::SeqCst), 1);
    }

    // ========== Arc<T> delegation tests ==========

    #[tokio::test]
    async fn test_arc_delegation_send() {
        let repo = Arc::new(MockRepository::new());
        let queue = Arc::new(DbMessageQueue::new(repo.clone()));
        let queue: Arc<dyn MessageQueue> = queue;
        let msg_id = queue
            .send("test", &serde_json::json!({"x": 1}))
            .await
            .unwrap();
        assert_eq!(msg_id, 1);
        assert_eq!(repo.send_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_delegation_read() {
        let repo = Arc::new(MockRepository::new());
        let queue = Arc::new(DbMessageQueue::new(repo.clone()));
        let queue: Arc<dyn MessageQueue> = queue;
        let result = queue.read("test", 30).await.unwrap();
        assert!(result.is_none());
        assert_eq!(repo.read_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_delegation_delete() {
        let repo = Arc::new(MockRepository::new());
        let queue = Arc::new(DbMessageQueue::new(repo.clone()));
        let queue: Arc<dyn MessageQueue> = queue;
        let result = queue.delete("test", 1).await.unwrap();
        assert!(result);
        assert_eq!(repo.delete_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_delegation_archive() {
        let repo = Arc::new(MockRepository::new());
        let queue = Arc::new(DbMessageQueue::new(repo.clone()));
        let queue: Arc<dyn MessageQueue> = queue;
        let result = queue.archive("test", 1).await.unwrap();
        assert!(result);
        assert_eq!(repo.archive_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_delegation_send_batch() {
        let repo = Arc::new(MockRepository::new());
        let queue = Arc::new(DbMessageQueue::new(repo.clone()));
        let queue: Arc<dyn MessageQueue> = queue;
        let ids = queue
            .send_batch("test", &[serde_json::json!({"a": 1})])
            .await
            .unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(repo.send_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_delegation_read_batch() {
        let repo = Arc::new(MockRepository::new());
        let queue = Arc::new(DbMessageQueue::new(repo.clone()));
        let queue: Arc<dyn MessageQueue> = queue;
        let msgs = queue.read_batch("test", 30, 5).await.unwrap();
        assert!(msgs.is_empty());
        assert_eq!(repo.read_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_delegation_delete_batch() {
        let repo = Arc::new(MockRepository::new());
        let queue = Arc::new(DbMessageQueue::new(repo.clone()));
        let queue: Arc<dyn MessageQueue> = queue;
        let count = queue.delete_batch("test", &[1, 2]).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_arc_delegation_archive_batch() {
        let repo = Arc::new(MockRepository::new());
        let queue = Arc::new(DbMessageQueue::new(repo.clone()));
        let queue: Arc<dyn MessageQueue> = queue;
        let count = queue.archive_batch("test", &[1]).await.unwrap();
        assert_eq!(count, 1);
    }
}
