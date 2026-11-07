// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! webhook 投递能力装配（webhook-on / webhook-off 双形态）。

use std::sync::Arc;

use crate::bootstrap::infrastructure::Repositories;
use crate::config::settings::Settings;
#[cfg(not(feature = "webhook"))]
use crate::domain::services::noop_webhook_service::NoopWebhookService;
use crate::domain::services::webhook_service::WebhookService;
#[cfg(feature = "webhook")]
use crate::domain::services::webhook_service::WebhookServiceImpl;
#[cfg(feature = "webhook")]
use crate::infrastructure::services::webhook_sender_impl::WebhookSenderImpl;

/// 装配 webhook 服务：webhook-on → `WebhookServiceImpl`；webhook-off → `NoopWebhookService`
///（所有方法返回 Ok(())，业务逻辑放行）。
pub(super) fn init_webhook_service(
    #[cfg_attr(not(feature = "webhook"), allow(unused_variables))] repositories: &Repositories,
    #[cfg_attr(not(feature = "webhook"), allow(unused_variables))] settings: &Settings,
    http_client: &Arc<reqwest::Client>,
) -> Arc<dyn WebhookService> {
    // webhook 相关服务在 webhook-off 时不初始化
    // webhook-on：装配 WebhookServiceImpl（含 WebhookSenderImpl + 签名密钥 + 事件仓库）
    // webhook-off：装配 NoopWebhookService（所有方法返回 Ok(())，业务逻辑放行）
    #[cfg(feature = "webhook")]
    let webhook_sender: Arc<WebhookSenderImpl> = Arc::new(WebhookSenderImpl::new(
        http_client.clone(),
        std::time::Duration::from_secs(10),
    ));
    #[cfg(feature = "webhook")]
    let webhook_service: Arc<WebhookServiceImpl> = Arc::new(WebhookServiceImpl::new(
        webhook_sender.clone(),
        settings.webhook.secret().to_string(),
        repositories.webhook_event_repo.clone(),
    ));
    #[cfg(not(feature = "webhook"))]
    let webhook_service: Arc<NoopWebhookService> = {
        // webhook-off 时输出 info 日志，告知运维 webhook 投递已禁用。
        // 严重程度低于 auth/rate-limit 警告（webhook 关闭不直接引入安全风险，仅影响通知能力）。
        log::info!(
            "webhook feature disabled, using NoopWebhookService — \
             trigger_completion/trigger_failure are no-ops"
        );
        Arc::new(NoopWebhookService::new())
    };
    webhook_service
}
