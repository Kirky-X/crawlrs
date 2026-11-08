// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 后台 workers 装配：webhook（仅 webhook-on）/ backlog / expiration。

use std::sync::Arc;

use crate::bootstrap::infrastructure::Repositories;
use crate::config::settings::Settings;
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::webhook_service::WebhookService;

/// workers 装配产物
pub(super) struct WorkerServices {
    #[cfg(feature = "webhook")]
    pub(super) webhook_worker: Arc<crate::workers::webhook_worker::WebhookWorker>,
    pub(super) backlog_worker: Arc<crate::workers::backlog_worker::BacklogWorker>,
    pub(super) expiration_worker: Arc<crate::workers::expiration_worker::ExpirationWorker>,
}

/// 装配后台 workers：webhook（仅 webhook-on）/ backlog / expiration。
pub(super) fn init_workers(
    repositories: &Repositories,
    settings: &Settings,
    #[cfg_attr(not(feature = "webhook"), allow(unused_variables))] webhook_service: &Arc<
        dyn WebhookService,
    >,
    rate_limiting_service: &Arc<dyn RateLimitingService>,
) -> WorkerServices {
    // Initialize WebhookWorker（仅 webhook-on 时构造）
    // webhook-off：不构造 WebhookWorker，ServicesComponents.webhook_worker 字段不编译
    #[cfg(feature = "webhook")]
    let webhook_worker = Arc::new(crate::workers::webhook_worker::WebhookWorker::new(
        repositories.webhook_event_repo.clone(),
        webhook_service.clone(),
        // D10 修复：重试上限此前固定 `RetryPolicy::default()`（=5），
        // `[webhook] max_retries` 配置无消费方（E2E F1：配置 3 实际 5）。
        crate::utils::retry_policy::RetryPolicy {
            max_retries: settings.webhook.max_retries,
            ..Default::default()
        },
    ));

    let backlog_worker = Arc::new(crate::workers::backlog_worker::BacklogWorker::new(
        repositories.tasks_backlog_repo.clone(),
        repositories.task_repo.clone(),
        rate_limiting_service.clone(),
        Arc::new(settings.clone()),
    ));

    let expiration_worker = Arc::new(crate::workers::expiration_worker::ExpirationWorker::new(
        repositories.task_repo.clone(),
        repositories.result_repo.clone(),
        settings.retention.scrape_results_days,
    ));

    WorkerServices {
        #[cfg(feature = "webhook")]
        webhook_worker,
        backlog_worker,
        expiration_worker,
    }
}
