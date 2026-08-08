// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 基于 tokio::sync::broadcast 的事件总线实现

use async_trait::async_trait;
use tokio::sync::broadcast;

use super::event_bus::{DomainEvent, EventBus, EventBusError};

/// 基于 broadcast channel 的事件总线实现
///
/// 使用 `tokio::sync::broadcast` 实现进程内事件发布/订阅。
/// 多订阅者并发接收事件，channel 满时丢弃最旧消息（尽力投递语义）。
pub struct BroadcastEventBus {
    sender: broadcast::Sender<DomainEvent>,
}

impl BroadcastEventBus {
    /// 创建新的事件总线实例
    ///
    /// # 参数
    ///
    /// * `capacity` - broadcast channel 缓冲区大小。默认推荐 1024。
    ///   满时最旧消息被丢弃，订阅者可能漏事件。
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }
}

#[async_trait]
impl EventBus for BroadcastEventBus {
    fn publish(&self, event: DomainEvent) -> Result<(), EventBusError> {
        // send 返回 Err 仅当无活跃 receiver（所有 receiver 已 drop）。
        // 无订阅者时仍返回 Ok（不报错），符合"尽力投递"语义。
        let _ = self.sender.send(event);
        Ok(())
    }

    fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.sender.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_publish_with_no_subscribers_returns_ok() {
        let bus = BroadcastEventBus::new(16);
        let event = DomainEvent::TaskCompleted {
            task_id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
        };
        let result = bus.publish(event);
        assert!(
            result.is_ok(),
            "publish with no subscribers should return Ok"
        );
    }

    #[tokio::test]
    async fn test_subscriber_receives_published_event() {
        let bus = BroadcastEventBus::new(16);
        let mut receiver = bus.subscribe();

        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let event = DomainEvent::TaskCompleted { task_id, team_id };

        bus.publish(event).unwrap();

        let received = receiver.recv().await.unwrap();
        match received {
            DomainEvent::TaskCompleted {
                task_id: tid,
                team_id: tid2,
            } => {
                assert_eq!(tid, task_id);
                assert_eq!(tid2, team_id);
            }
            _ => panic!("Expected TaskCompleted event"),
        }
    }

    #[tokio::test]
    async fn test_multiple_subscribers_all_receive_event() {
        let bus = BroadcastEventBus::new(16);
        let mut receiver1 = bus.subscribe();
        let mut receiver2 = bus.subscribe();

        let scrape_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let event = DomainEvent::ScrapeCompleted { scrape_id, team_id };

        bus.publish(event).unwrap();

        let received1 = receiver1.recv().await.unwrap();
        let received2 = receiver2.recv().await.unwrap();

        match received1 {
            DomainEvent::ScrapeCompleted {
                scrape_id: sid,
                team_id: tid,
            } => {
                assert_eq!(sid, scrape_id);
                assert_eq!(tid, team_id);
            }
            _ => panic!("Receiver 1: Expected ScrapeCompleted event"),
        }

        match received2 {
            DomainEvent::ScrapeCompleted {
                scrape_id: sid,
                team_id: tid,
            } => {
                assert_eq!(sid, scrape_id);
                assert_eq!(tid, team_id);
            }
            _ => panic!("Receiver 2: Expected ScrapeCompleted event"),
        }
    }

    #[tokio::test]
    async fn test_publish_multiple_events_in_order() {
        let bus = BroadcastEventBus::new(16);
        let mut receiver = bus.subscribe();

        let task_id1 = Uuid::new_v4();
        let task_id2 = Uuid::new_v4();
        let team_id = Uuid::new_v4();

        bus.publish(DomainEvent::TaskCompleted {
            task_id: task_id1,
            team_id,
        })
        .unwrap();
        bus.publish(DomainEvent::TaskFailed {
            task_id: task_id2,
            team_id,
            error: "test error".to_string(),
        })
        .unwrap();

        let received1 = receiver.recv().await.unwrap();
        let received2 = receiver.recv().await.unwrap();

        match received1 {
            DomainEvent::TaskCompleted { task_id, .. } => assert_eq!(task_id, task_id1),
            _ => panic!("Expected TaskCompleted"),
        }

        match received2 {
            DomainEvent::TaskFailed { task_id, error, .. } => {
                assert_eq!(task_id, task_id2);
                assert_eq!(error, "test error");
            }
            _ => panic!("Expected TaskFailed"),
        }
    }
}
