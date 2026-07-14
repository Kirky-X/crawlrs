// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 队列模块接口定义
//!
//! 本模块只放置 trait / struct / enum 接口定义，实现放独立文件。
//! 现有 `TaskQueue` trait（基于 Task 域模型）保持不变，新增 `MessageQueue` trait
//! 基于 `TaskMessage`，面向消息队列场景（ack/nack/dead_letter 语义）。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod client;
/// 队列模块
///
/// 提供统一的任务队列客户端和调度功能
/// 负责任务的排队、调度和执行管理
///
/// # 统一客户端使用示例
///
/// ```ignore
/// use crawlrs::queue::{
///     QueueClient,
///     QueueClientBuilder,
///     EnqueueRequest,
///     DequeueRequest,
/// };
///
/// // 创建客户端
/// let client = QueueClientBuilder::new()
///     .with_default_priority(5)
///     .build(queue);
///
/// // 入队
/// let task = client.enqueue(
///     EnqueueRequest::new("scrape", "https://example.com", payload, team_id)
/// ).await?;
///
/// // 出队
/// let task = client.dequeue(
///     DequeueRequest::new(worker_id)
/// ).await?;
/// ```
pub mod message_queue;
pub mod scheduler;
pub mod task_queue;

pub use self::client::{
    BatchDequeueResult, BatchEnqueueResult, DequeueRequest, EnqueueRequest, QueueClient,
    QueueClientBuilder, QueueClientConfig, QueueClientError as Error, QueueMetrics,
    QueueMetricsData, QueueOperation, StatusUpdateRequest, StatusUpdateType,
};

pub use self::message_queue::InMemoryQueue;
pub use self::task_queue::{PostgresTaskQueue, QueueError, TaskQueue};

// =========================================================================
// Phase 5: 消息队列接口（MessageQueue）— 面向 ack/nack/dead_letter 语义
// =========================================================================

/// 消息队列优先级
///
/// 派生 `Ord` 后，声明顺序决定大小关系：`Low < Normal < High < Critical`。
/// `Critical` 为最高优先级，出队时优先返回。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// 低优先级
    Low,
    /// 普通优先级（默认）
    #[default]
    Normal,
    /// 高优先级
    High,
    /// 关键优先级（最高）
    Critical,
}

/// 消息任务类型（队列层）
///
/// 与领域层 `crate::domain::models::TaskType`（Scrape/Crawl/Extract）不同，
/// 此枚举面向消息队列场景，支持 Webhook、导出、清理和自定义类型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueTaskType {
    /// 网页抓取任务
    Scrape,
    /// Webhook 通知任务
    Webhook,
    /// 数据导出任务
    Export,
    /// 清理任务
    Cleanup,
    /// 自定义任务类型（携带类型名称）
    Custom(String),
}

impl QueueTaskType {
    /// 返回任务类型的字符串表示
    pub fn as_str(&self) -> &str {
        match self {
            Self::Scrape => "scrape",
            Self::Webhook => "webhook",
            Self::Export => "export",
            Self::Cleanup => "cleanup",
            Self::Custom(name) => name.as_str(),
        }
    }
}

/// 消息任务
///
/// 队列层消息载体，与领域层 `Task` 通过 `From` 适配器互转。
/// 字段面向消息队列语义：优先级排序、重试计数、死信流转。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMessage {
    /// 消息唯一标识
    pub id: Uuid,
    /// 任务类型
    pub task_type: QueueTaskType,
    /// 任务负载（JSON）
    pub payload: serde_json::Value,
    /// 优先级
    pub priority: Priority,
    /// 已重试次数
    pub retry_count: u32,
    /// 最大重试次数（超过后进入死信队列）
    pub max_retries: u32,
    /// 计划执行时间
    pub scheduled_at: chrono::DateTime<chrono::Utc>,
}

impl TaskMessage {
    /// 创建新的任务消息（retry_count=0，scheduled_at=now）
    pub fn new(
        id: Uuid,
        task_type: QueueTaskType,
        payload: serde_json::Value,
        priority: Priority,
        max_retries: u32,
    ) -> Self {
        Self {
            id,
            task_type,
            payload,
            priority,
            retry_count: 0,
            max_retries,
            scheduled_at: chrono::Utc::now(),
        }
    }
}

/// 消息队列特质
///
/// 面向消息队列场景的抽象接口，支持：
/// - `enqueue` / `dequeue` 基本入出队
/// - `ack` 确认处理完成（消息移除）
/// - `nack` 处理失败（retry_count 递增，超过 max_retries 触发死信）
/// - `dead_letter` 直接转入死信队列
///
/// 与现有 `TaskQueue` trait 互补，不替换。
#[async_trait]
pub trait MessageQueue: Send + Sync {
    /// 入队消息，返回入队后的消息（可能携带服务端分配的字段）
    async fn enqueue(&self, msg: TaskMessage) -> Result<TaskMessage, QueueError>;

    /// 出队消息（按优先级降序、scheduled_at 升序）。
    /// 返回 `Ok(None)` 表示队列无可用消息。
    async fn dequeue(&self, worker_id: Uuid) -> Result<Option<TaskMessage>, QueueError>;

    /// 确认消息已处理完成，从队列移除。
    async fn ack(&self, task_id: Uuid) -> Result<(), QueueError>;

    /// 拒绝消息（处理失败），retry_count 递增；
    /// 若超过 max_retries 则自动转入死信队列。
    async fn nack(&self, task_id: Uuid, reason: String) -> Result<(), QueueError>;

    /// 直接将消息转入死信队列（不再可被 dequeue）。
    async fn dead_letter(&self, task_id: Uuid, reason: String) -> Result<(), QueueError>;
}
