// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Worker 启动去重 — 提取 `start_api_service` 和 `start_worker_service` 中
//! 重复的 worker spawn 代码。

use crate::config::settings::Settings;
use crate::di::CrawlRsState;
use crate::di::CrawlRsStateExt;
use crate::workers::retention_worker::RetentionWorker;
use crate::workers::{AbstractWorker, Worker};
use std::sync::Arc;

/// 启动所有通用 worker（webhook / backlog / expiration，按需含 retention）。
///
/// `start_api_service()` 和 `start_worker_service()` 共享的 worker spawn 逻辑
/// 统一由此函数管理，避免重复代码。
///
/// `include_retention`：T014（converge 补强）。RetentionWorker 仅在
/// `WorkerManager::start_workers`（`command: ["worker"]`）注册，默认
/// `SERVICE_TYPE=api` 的单容器部署不会执行保留期清理。api 形态传 `true`
/// 由此处启动；worker 形态传 `false`（由 WorkerManager 启动，避免同进程双调度）。
///
/// T024 修复：返回所有 worker 的 `JoinHandle`，调用方应在关闭时 await
/// 并记录 panic。
pub async fn spawn_common_workers(
    app_state: &CrawlRsState,
    settings: &Arc<Settings>,
    include_retention: bool,
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

    // T014（converge 补强）：RetentionWorker 调度。仅在 include_retention=true
    // 时启动（api 形态）；worker 形态由 WorkerManager::start_workers 注册，避免双调度。
    if include_retention {
        let retention_processor = Arc::new(RetentionWorker::new(
            app_state.result_repo(),
            app_state.geo_restriction_repo(),
            app_state.webhook_event_repo(),
            app_state.audit_service(),
            settings.retention.scrape_results_days,
            settings.retention.geo_logs_days,
            settings.retention.webhook_events_days,
            settings.retention.audit_logs_days,
            crate::domain::retention_policy::RetentionBatchPolicy::from_settings(
                &settings.retention,
            ),
            settings.retention.category_timeout_seconds,
            std::sync::Arc::new(crate::workers::retention_worker::PgRetentionLock::new(
                app_state.db_pool(),
            )),
        ));
        let retention_worker = AbstractWorker::new(
            retention_processor,
            std::time::Duration::from_secs(settings.retention.interval_seconds),
        );
        handles.push(tokio::spawn(async move {
            retention_worker.run().await;
        }));
    }

    handles
}
