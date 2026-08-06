// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook Management Service Implementation
//!
//! Manages webhook endpoint registration and lifecycle.

use crate::domain::services::webhook_service::{WebhookService, WebhookManagementService};
use crate::domain::services::webhook_event_builder::WebhookEventBuilder;
use crate::domain::models::{Webhook, WebhookEventType};
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use async_trait::async_trait;
use log::{error, info};
use std::sync::Arc;
use uuid::Uuid;
use anyhow::{anyhow, Result};

/// 通过组合 `WebhookService` 复用已有的签名生成与发送逻辑，
/// 避免代码重复。DI 注册在 Phase 11 统一处理。
/// R-wh-001 / T023：webhook feature 关闭时不编译此类型
#[cfg(feature = "webhook")]
pub struct WebhookManagementServiceImpl {
    /// Webhook 仓库（端点 CRUD）
    webhook_repository: Arc<dyn WebhookRepository>,
    /// Webhook 事件仓库（事件持久化）
    event_repository: Arc<dyn WebhookEventRepository>,
    /// Webhook 发送服务（复用现有签名+发送逻辑）
    webhook_service: Arc<dyn WebhookService>,
}

/// R-wh-001 / T023：webhook feature 关闭时不编译此 impl
#[cfg(feature = "webhook")]
impl WebhookManagementServiceImpl {
    /// 创建新的 Webhook 管理服务实现（测试与手动构造用）
    pub fn new(
        webhook_repository: Arc<dyn WebhookRepository>,
        event_repository: Arc<dyn WebhookEventRepository>,
        webhook_service: Arc<dyn WebhookService>,
    ) -> Self {
        Self {
            webhook_repository,
            event_repository,
            webhook_service,
        }
    }
}

/// R-wh-001 / T023：webhook feature 关闭时不编译此 impl
#[cfg(feature = "webhook")]
#[async_trait]
impl WebhookManagementService for WebhookManagementServiceImpl {
    async fn register_webhook(&self, team_id: Uuid, url: String) -> Result<Webhook> {
        let webhook = Webhook::new(Uuid::new_v4(), team_id, url);
        webhook
            .validate_url()
            .map_err(|e| anyhow!("Invalid webhook URL: {}", e))?;

        let created = self
            .webhook_repository
            .create(&webhook)
            .await
            .map_err(|e| anyhow!("Failed to create webhook: {}", e))?;

        info!(
            "Registered webhook {} for team {}",
            created.id, created.team_id
        );
        Ok(created)
    }

    async fn trigger_webhook(
        &self,
        webhook_id: Uuid,
        event_type: WebhookEventType,
        payload: serde_json::Value,
    ) -> Result<()> {
        let webhook = self
            .webhook_repository
            .find_by_id(webhook_id)
            .await
            .map_err(|e| anyhow!("Failed to find webhook {}: {}", webhook_id, e))?
            .ok_or_else(|| anyhow!("Webhook not found: {}", webhook_id))?;

        let event = WebhookEventBuilder::build_triggered_event(
            webhook.team_id,
            webhook.id,
            event_type,
            payload,
            webhook.url.clone(),
        );

        self.event_repository
            .create(&event)
            .await
            .map_err(|e| anyhow!("Failed to create webhook event: {}", e))?;

        if let Err(e) = self.webhook_service.send_webhook(&event).await {
            error!("Failed to send webhook event {}: {}", event.id, e);
            return Err(e);
        }

        info!("Triggered webhook {} for event {}", webhook_id, event.id);
        Ok(())
    }

    async fn retry_failed(&self, limit: u64) -> Result<u64> {
        let pending = self
            .event_repository
            .find_pending(limit)
            .await
            .map_err(|e| anyhow!("Failed to find pending webhook events: {}", e))?;

        let mut success_count: u64 = 0;
        for mut event in pending {
            if !event.can_retry() {
                continue;
            }

            match self.webhook_service.send_webhook(&event).await {
                Ok(()) => {
                    event.record_attempt(true, Some(200), None);
                    success_count += 1;
                }
                Err(e) => {
                    event.record_attempt(false, None, Some(e.to_string()));
                }
            }

            if let Err(e) = self.event_repository.update(&event).await {
                error!("Failed to update webhook event {}: {}", event.id, e);
            }
        }

        info!(
            "Retried pending webhook events (limit {}), {} succeeded",
            limit, success_count
        );
        Ok(success_count)
    }

    async fn list_webhooks(&self, team_id: Uuid) -> Result<Vec<Webhook>> {
        let webhooks = self
            .webhook_repository
            .find_by_team_id(team_id)
            .await
            .map_err(|e| anyhow!("Failed to list webhooks for team {}: {}", team_id, e))?;

        Ok(webhooks)
    }
}
