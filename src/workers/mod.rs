// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 工作器模块
///
/// 提供后台任务处理和工作器管理功能
/// 包括任务执行、工作器生命周期管理和并发控制
pub mod backlog_worker;
pub mod errors;
pub mod expiration_worker;
pub mod manager;
pub mod scheduler;
pub mod scrape_worker;
pub mod task_state_machine;
/// R-wh-001 / T026：webhook feature 关闭时不编译此模块
/// （webhook_worker spawn 也会门控，见 main.rs）
#[cfg(feature = "webhook")]
pub mod webhook_worker;
pub mod worker;

pub use errors::ScrapeWorkerError;
pub use worker::{AbstractWorker, ProcessResult, Worker, WorkerProcess};
