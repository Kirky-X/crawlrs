// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 队列模块接口定义
//!
//! 本模块只放置 trait / struct / enum 接口定义，实现放独立文件。
//! `TaskQueue` trait 基于 Task 域模型，是生产环境唯一的队列抽象。

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
pub mod scheduler;
pub mod task_queue;

pub use self::client::{
    BatchDequeueResult, BatchEnqueueResult, DequeueRequest, EnqueueRequest, QueueClient,
    QueueClientBuilder, QueueClientConfig, QueueClientError as Error, QueueMetrics,
    QueueMetricsData, QueueOperation, StatusUpdateRequest, StatusUpdateType,
};

pub use self::task_queue::{PostgresTaskQueue, QueueError, TaskQueue};
