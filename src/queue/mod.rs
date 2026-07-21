// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 队列模块接口定义
//!
//! 本模块只放置 trait / struct / enum 接口定义，实现放独立文件。
//! `TaskQueue` trait 基于 Task 域模型，是生产环境唯一的队列抽象。

/// 队列模块
///
/// 提供统一的任务队列接口，负责任务的排队、调度和执行管理。
pub mod task_queue;

pub use self::task_queue::{PostgresTaskQueue, QueueError, TaskQueue};
