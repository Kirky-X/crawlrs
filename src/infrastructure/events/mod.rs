// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 事件总线模块
//!
//! 提供进程内事件发布/订阅机制，解耦模块间通信。
//! 基于 `tokio::sync::broadcast` 实现，零新外部依赖。

pub mod broadcast;
pub mod event_bus;

pub use event_bus::{DomainEvent, EventBus, EventBusError};
pub use broadcast::BroadcastEventBus;
