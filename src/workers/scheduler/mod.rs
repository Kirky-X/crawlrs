// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 内存感知调度器模块
//!
//! 提供基于内存使用率的任务准入决策与公平优先级队列。
//! 详见 `design.md` §5（crawler-capability-absorption Stage 1）。
//!
//! [`memory_scheduler`] 依赖 `SystemMonitorTrait`（位于 `metrics` 特性门控的
//! `infrastructure::observability::metrics` 模块），故仅在 `metrics` 启用时编译；
//! [`priority_queue`] 是纯数据结构，无外部依赖，始终可用。

pub mod priority_queue;
#[cfg(feature = "metrics")]
pub mod memory_scheduler;

pub use priority_queue::{PriorityQueue, ScheduledTask};
#[cfg(feature = "metrics")]
pub use memory_scheduler::{Admission, MemoryScheduler, MemoryState};
