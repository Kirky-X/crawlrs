// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Worker 启动去重 — 提取 `start_api_service` 和 `start_worker_service` 中
//! 重复的 worker spawn 代码。

use crate::config::settings::Settings;
use crate::di::CrawlRsState;
use crate::di::CrawlRsStateExt;
use crate::workers::{AbstractWorker, Worker};
use std::sync::Arc;

/// 启动所有通用 worker（webhook / backlog / expiration）。
///
/// `start_api_service()` 和 `start_worker_service()` 共享的 worker spawn 逻辑
/// 统一由此函数管理，避免重复代码。
///
/// T024 修复：返回所有 worker 的 `JoinHandle`，调用方应在关闭时 await
/// 并记录 panic。
pub async fn spawn_common_workers(
    app_state: &CrawlRsState,
    settings: &Arc<Settings>,
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    // R-wh-003 / T029：webhook worker 仅在 webhook feature 启用时启动
    #[cfg(feature = "webhook")]
    {
        let webhook_worker = AbstractWorker::new(
            app_state.webhook_worker(),
            std::time::Duration::from_secs(5),
        );
        handles.push(tokio::spawn(async move {
            webhook_worker.run().await;
        }));
    }

    // Start backlog worker
    let backlog_worker = AbstractWorker::new(
        app_state.backlog_worker(),
        std::time::Duration::from_secs(settings.timeouts.workers.backlog_interval_seconds),
    );
    handles.push(tokio::spawn(async move {
        backlog_worker.run().await;
    }));

    // Start expiration worker
    let expiration_worker = AbstractWorker::new(
        app_state.expiration_worker(),
        std::time::Duration::from_secs(3600), // Run every hour
    );
    handles.push(tokio::spawn(async move {
        expiration_worker.run().await;
    }));

    handles
}
