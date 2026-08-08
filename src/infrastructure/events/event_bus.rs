// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 事件总线 trait 与领域事件定义
//!
//! 提供进程内事件发布/订阅机制，解耦模块间通信。
//! 基于 `tokio::sync::broadcast` 实现，零新外部依赖。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::broadcast;
use uuid::Uuid;

/// 事件总线错误
#[derive(Error, Debug)]
pub enum EventBusError {
    /// 发布失败（无订阅者或 channel 已关闭）
    #[error("Event publish failed: {0}")]
    PublishFailed(String),
}

/// 领域事件枚举
///
/// 覆盖核心领域事件，所有变体包含 `team_id` 以支持多租户隔离。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainEvent {
    /// 任务完成
    TaskCompleted { task_id: Uuid, team_id: Uuid },
    /// 任务失败
    TaskFailed {
        task_id: Uuid,
        team_id: Uuid,
        error: String,
    },
    /// 爬取完成
    CrawlCompleted { crawl_id: Uuid, team_id: Uuid },
    /// 爬取失败
    CrawlFailed {
        crawl_id: Uuid,
        team_id: Uuid,
        error: String,
    },
    /// 抓取完成
    ScrapeCompleted { scrape_id: Uuid, team_id: Uuid },
    /// 抓取失败
    ScrapeFailed {
        scrape_id: Uuid,
        team_id: Uuid,
        error: String,
    },
}

/// 事件总线 trait
///
/// 提供 publish/subscribe 接口，支持进程内事件驱动架构。
/// 实现基于 `tokio::sync::broadcast`，多订阅者并发接收事件。
#[async_trait]
pub trait EventBus: Send + Sync {
    /// 发布事件到所有订阅者
    ///
    /// # 参数
    ///
    /// * `event` - 要发布的领域事件
    ///
    /// # 返回
    ///
    /// * `Ok(())` - 发布成功（即使无订阅者）
    /// * `Err(EventBusError)` - 发布失败（channel 已关闭）
    fn publish(&self, event: DomainEvent) -> Result<(), EventBusError>;

    /// 订阅事件流
    ///
    /// # 返回
    ///
    /// 返回 `broadcast::Receiver<DomainEvent>`，用于接收后续发布的事件。
    /// 新订阅者仅接收订阅后发布的事件，不接收历史事件。
    fn subscribe(&self) -> broadcast::Receiver<DomainEvent>;
}
