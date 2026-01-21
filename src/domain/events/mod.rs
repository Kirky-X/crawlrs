// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 领域事件模块
//!
//! 该模块提供事件驱动架构的基础设施，支持：
//! - 领域事件定义：系统中的重要业务事件
//! - 事件发布：发布领域事件
//! - 事件订阅：监听和处理领域事件
//!
//! 事件用于解耦业务逻辑，当重要业务状态改变时发布事件，
//! 订阅者可以异步处理这些事件而不影响主业务流程。

pub mod in_memory;
pub mod models;
pub mod traits;

pub use models::*;
pub use traits::*;
