// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 工作器模块
///
/// 提供后台任务处理和工作器管理功能
/// 包括任务执行、工作器生命周期管理和并发控制
pub mod backlog_worker;
pub mod manager;
pub mod scrape_worker;
pub mod webhook_worker;
pub mod worker;

pub use worker::Worker;
