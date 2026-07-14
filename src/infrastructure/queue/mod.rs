// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 队列模块
//!
//! 提供统一的任务队列客户端和调度功能
//!
//! # 统一客户端使用示例
//!
//! ```ignore
//! use crawlrs::queue::{
//!     QueueClient,
//!     QueueClientBuilder,
//!     EnqueueRequest,
//!     DequeueRequest,
//! };
//!
//! // 创建客户端
//! let client = QueueClientBuilder::new()
//!     .with_default_priority(5)
//!     .build(queue);
//!
//! // 入队
//! let task = client.enqueue(
//!     EnqueueRequest::new("scrape", "https://example.com", payload, team_id)
//! ).await?;
//!
//! // 出队
//! let task = client.dequeue(
//!     DequeueRequest::new(worker_id)
//! ).await?;
//! ```

// 重新导出 queue 模块以保持向后兼容性
pub use crate::queue::*;

pub mod apalis_queue;
