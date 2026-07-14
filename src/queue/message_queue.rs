// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 内存消息队列实现（测试用 + ApalisQueue fallback）
//!
//! 不依赖 PostgreSQL，使用 `tokio::sync::Mutex` + `HashMap` 实现。
//! 用于验证 `MessageQueue` trait 行为，也作为 `ApalisQueue` 的 fallback 后端。
//!
//! 三区模型：
//! - `ready`: 可出队的消息
//! - `in_flight`: 已出队但未 ack/nack 的消息
//! - `dead_letter`: 死信消息（不可再出队）

use std::collections::HashMap;

use async_trait::async_trait;
use log::{debug, warn};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::{MessageQueue, QueueError, TaskMessage};

/// 将"未找到"包装为 QueueError::Repository
fn not_found(task_id: Uuid) -> QueueError {
    QueueError::Repository(
        crate::domain::repositories::task_repository::RepositoryError::Database(anyhow::anyhow!(
            "task not found: {}",
            task_id
        )),
    )
}

/// 队列内部状态（三区）
#[derive(Debug, Default)]
struct QueueState {
    /// 可出队的消息
    ready: HashMap<Uuid, TaskMessage>,
    /// 已出队但未确认的消息
    in_flight: HashMap<Uuid, TaskMessage>,
    /// 死信消息
    dead_letter: HashMap<Uuid, TaskMessage>,
}

/// 内存消息队列
///
/// 使用单一 Mutex 保护三区状态，避免多锁死锁。
/// 出队按优先级降序、scheduled_at 升序选取。
#[derive(Debug, Default)]
pub struct InMemoryQueue {
    state: Mutex<QueueState>,
}

impl InMemoryQueue {
    /// 创建空队列
    pub fn new() -> Self {
        Self::default()
    }

    /// 返回死信队列中的消息数（测试辅助）
    pub async fn dead_letter_count(&self) -> usize {
        self.state.lock().await.dead_letter.len()
    }
}

#[async_trait]
impl MessageQueue for InMemoryQueue {
    async fn enqueue(&self, msg: TaskMessage) -> Result<TaskMessage, QueueError> {
        let id = msg.id;
        debug!("enqueue task_id={} priority={:?}", id, msg.priority);
        self.state.lock().await.ready.insert(id, msg.clone());
        Ok(msg)
    }

    async fn dequeue(&self, worker_id: Uuid) -> Result<Option<TaskMessage>, QueueError> {
        debug!("dequeue worker_id={}", worker_id);
        let mut state = self.state.lock().await;
        let now = chrono::Utc::now();

        // 筛选可执行消息（scheduled_at <= now），按优先级降序、时间升序选取
        let best_id = state
            .ready
            .iter()
            .filter(|(_, m)| m.scheduled_at <= now)
            .max_by(|a, b| {
                a.1.priority
                    .cmp(&b.1.priority)
                    .then_with(|| b.1.scheduled_at.cmp(&a.1.scheduled_at))
            })
            .map(|(id, _)| *id);

        if let Some(id) = best_id {
            if let Some(msg) = state.ready.remove(&id) {
                state.in_flight.insert(id, msg.clone());
                return Ok(Some(msg));
            }
        }
        Ok(None)
    }

    async fn ack(&self, task_id: Uuid) -> Result<(), QueueError> {
        debug!("ack task_id={}", task_id);
        let mut state = self.state.lock().await;
        // 优先从 in_flight 移除（正常流程：dequeue → ack）
        if state.in_flight.remove(&task_id).is_some() {
            return Ok(());
        }
        // 也支持从 ready 直接 ack（未 dequeue 就确认）
        if state.ready.remove(&task_id).is_some() {
            return Ok(());
        }
        Err(not_found(task_id))
    }

    async fn nack(&self, task_id: Uuid, reason: String) -> Result<(), QueueError> {
        debug!("nack task_id={} reason={}", task_id, reason);
        let mut state = self.state.lock().await;

        // 从 in_flight 或 ready 取出消息
        let mut msg = state.in_flight.remove(&task_id);
        if msg.is_none() {
            msg = state.ready.remove(&task_id);
        }
        let mut msg = msg.ok_or_else(|| not_found(task_id))?;

        msg.retry_count += 1;

        // 超过最大重试次数（retry_count > max_retries）→ 死信队列
        if msg.retry_count > msg.max_retries {
            warn!(
                "task {} exceeded max_retries ({}/{}), moving to dead letter: {}",
                task_id, msg.retry_count, msg.max_retries, reason
            );
            state.dead_letter.insert(task_id, msg);
        } else {
            // 放回 ready 队列等待重试
            state.ready.insert(task_id, msg);
        }
        Ok(())
    }

    async fn dead_letter(&self, task_id: Uuid, reason: String) -> Result<(), QueueError> {
        debug!("dead_letter task_id={} reason={}", task_id, reason);
        let mut state = self.state.lock().await;

        // 从 in_flight 或 ready 取出消息
        let msg = state.in_flight.remove(&task_id);
        let msg = if msg.is_some() {
            msg
        } else {
            state.ready.remove(&task_id)
        };

        if let Some(msg) = msg {
            state.dead_letter.insert(task_id, msg);
            Ok(())
        } else {
            Err(not_found(task_id))
        }
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

    // 测试 1: enqueue 后能 dequeue 到相同消息
    #[tokio::test]
    async fn test_enqueue_then_dequeue_returns_same_message() {
        let queue = InMemoryQueue::new();
        let id = Uuid::new_v4();
        let msg = make_msg(id, Priority::Normal, 3);

        let enqueued = queue.enqueue(msg).await.unwrap();
        assert_eq!(enqueued.id, id);

        let dequeued = queue.dequeue(Uuid::new_v4()).await.unwrap();
        assert!(dequeued.is_some(), "should dequeue a message");
        let dequeued = dequeued.unwrap();
        assert_eq!(dequeued.id, id);
        assert_eq!(dequeued.task_type, QueueTaskType::Scrape);
        assert_eq!(dequeued.priority, Priority::Normal);
    }

    // 测试 2: 优先级排序（Critical > High > Normal > Low）
    #[tokio::test]
    async fn test_dequeue_priority_order() {
        let queue = InMemoryQueue::new();
        let low_id = Uuid::new_v4();
        let normal_id = Uuid::new_v4();
        let high_id = Uuid::new_v4();
        let critical_id = Uuid::new_v4();

        // 按非优先级顺序入队
        queue
            .enqueue(make_msg(low_id, Priority::Low, 3))
            .await
            .unwrap();
        queue
            .enqueue(make_msg(critical_id, Priority::Critical, 3))
            .await
            .unwrap();
        queue
            .enqueue(make_msg(normal_id, Priority::Normal, 3))
            .await
            .unwrap();
        queue
            .enqueue(make_msg(high_id, Priority::High, 3))
            .await
            .unwrap();

        // 出队顺序应为 Critical > High > Normal > Low
        let d1 = queue.dequeue(Uuid::new_v4()).await.unwrap().unwrap();
        assert_eq!(d1.id, critical_id, "first dequeue should be Critical");

        let d2 = queue.dequeue(Uuid::new_v4()).await.unwrap().unwrap();
        assert_eq!(d2.id, high_id, "second dequeue should be High");

        let d3 = queue.dequeue(Uuid::new_v4()).await.unwrap().unwrap();
        assert_eq!(d3.id, normal_id, "third dequeue should be Normal");

        let d4 = queue.dequeue(Uuid::new_v4()).await.unwrap().unwrap();
        assert_eq!(d4.id, low_id, "fourth dequeue should be Low");

        let d5 = queue.dequeue(Uuid::new_v4()).await.unwrap();
        assert!(d5.is_none(), "fifth dequeue should be None");
    }

    // 测试 3: ack 后消息不再可 dequeue
    #[tokio::test]
    async fn test_ack_removes_message_from_dequeue() {
        let queue = InMemoryQueue::new();
        let id = Uuid::new_v4();

        queue
            .enqueue(make_msg(id, Priority::Normal, 3))
            .await
            .unwrap();

        // ack 后消息不再可出队
        queue.ack(id).await.unwrap();

        let result = queue.dequeue(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none(), "after ack, dequeue should return None");
    }

    // 测试 4: nack 后 retry_count 递增
    #[tokio::test]
    async fn test_nack_increments_retry_count() {
        let queue = InMemoryQueue::new();
        let id = Uuid::new_v4();

        // max_retries=3 足够，nack 一次不会进死信
        queue
            .enqueue(make_msg(id, Priority::Normal, 3))
            .await
            .unwrap();

        // 出队
        let d1 = queue.dequeue(Uuid::new_v4()).await.unwrap().unwrap();
        assert_eq!(d1.retry_count, 0, "initial retry_count should be 0");

        // nack 后 retry_count 递增
        queue.nack(id, "test failure".to_string()).await.unwrap();

        // 再次出队，retry_count 应为 1
        let d2 = queue.dequeue(Uuid::new_v4()).await.unwrap().unwrap();
        assert_eq!(d2.id, id);
        assert_eq!(d2.retry_count, 1, "retry_count should be 1 after nack");
    }

    // 测试 5: retry_count 超过 max_retries 触发 dead_letter
    #[tokio::test]
    async fn test_nack_exceeding_max_retries_triggers_dead_letter() {
        let queue = InMemoryQueue::new();
        let id = Uuid::new_v4();

        // max_retries=0：不允许重试，nack 一次后 retry_count=1 > 0 → 死信
        queue
            .enqueue(make_msg(id, Priority::Normal, 0))
            .await
            .unwrap();

        // 出队
        let d1 = queue.dequeue(Uuid::new_v4()).await.unwrap().unwrap();
        assert_eq!(d1.retry_count, 0);

        // nack：retry_count → 1 > max_retries=0 → 死信
        queue
            .nack(id, "exceeded retries".to_string())
            .await
            .unwrap();

        // 死信队列应有一条消息
        assert_eq!(
            queue.dead_letter_count().await,
            1,
            "should have 1 dead-lettered message"
        );

        // 再次出队应返回 None（消息在死信队列中）
        let d2 = queue.dequeue(Uuid::new_v4()).await.unwrap();
        assert!(d2.is_none(), "dead-lettered message should not be dequeued");
    }

    // 测试 6: 死信队列消息不可再 dequeue
    #[tokio::test]
    async fn test_dead_lettered_message_not_available_for_dequeue() {
        let queue = InMemoryQueue::new();
        let id = Uuid::new_v4();

        queue
            .enqueue(make_msg(id, Priority::Normal, 3))
            .await
            .unwrap();

        // 直接转入死信队列
        queue
            .dead_letter(id, "manual dead letter".to_string())
            .await
            .unwrap();

        // 出队应返回 None
        let result = queue.dequeue(Uuid::new_v4()).await.unwrap();
        assert!(
            result.is_none(),
            "dead-lettered message should not be dequeued"
        );

        // 死信队列应有 1 条
        assert_eq!(queue.dead_letter_count().await, 1);
    }
}
