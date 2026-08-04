// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 队列模块接口定义
//!
//! 本模块只放置 trait / struct / enum 接口定义，实现放独立文件。
//!
//! - `TaskQueue` trait 基于 Task 域模型，面向任务生命周期管理。
//! - `MessageQueue` trait 基于 pgmq 设计思想，面向通用 JSON 消息队列
//!   （visibility timeout、归档、批量操作）。存储操作通过 `MessageQueueRepository`
//!   trait 抽象，不同数据库后端各自实现。
//!
//! 两者互补，不互相替代。

/// 任务队列模块（基于 Task 域模型）
pub mod task_queue;

/// pgmq 风格消息队列：trait 定义 + 通用实现（DbMessageQueue）
pub mod message_queue;

/// PostgreSQL 消息队列存储实现（MessageQueueRepository for PostgreSQL）
pub mod postgres_message_queue;

pub use self::message_queue::{
    DbMessageQueue, Message, MessageQueue, MessageQueueError, MessageQueueRepository,
};
pub use self::postgres_message_queue::PostgresMessageQueueRepository;
pub use self::task_queue::{PostgresTaskQueue, QueueError, TaskQueue};
