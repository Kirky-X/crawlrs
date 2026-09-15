// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook Management Service Implementation
//!
//! Manages webhook endpoint registration and lifecycle.

use crate::domain::models::{Webhook, WebhookEventType};
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::domain::services::webhook_event_builder::WebhookEventBuilder;
use crate::domain::services::webhook_service::{WebhookManagementService, WebhookService};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::{error, info};
use std::sync::Arc;
use uuid::Uuid;

/// 通过组合 `WebhookService` 复用已有的签名生成与发送逻辑，
/// 避免代码重复。DI 注册在统一处理。
/// webhook feature 关闭时不编译此类型
#[cfg(feature = "webhook")]
pub struct WebhookManagementServiceImpl {
    /// Webhook 仓库（端点 CRUD）
    webhook_repository: Arc<dyn WebhookRepository>,
    /// Webhook 事件仓库（事件持久化）
    event_repository: Arc<dyn WebhookEventRepository>,
    /// Webhook 发送服务（复用现有签名+发送逻辑）
    webhook_service: Arc<dyn WebhookService>,
}

/// webhook feature 关闭时不编译此 impl
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

/// webhook feature 关闭时不编译此 impl
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

        let mut event = WebhookEventBuilder::build_triggered_event(
            webhook.team_id,
            webhook.id,
            event_type,
            payload,
            webhook.url.clone(),
        );

        // 与 send_task_webhook 一致：内联发送前置 processing，
        // 防止 webhook-worker 在发送期间认领造成重复投递
        event.status = crate::domain::models::WebhookStatus::Processing;

        self.event_repository
            .create(&event)
            .await
            .map_err(|e| anyhow!("Failed to create webhook event: {}", e))?;

        if let Err(e) = self.webhook_service.send_webhook(&event).await {
            error!("Failed to send webhook event {}: {}", event.id, e);
            // 交还 worker 按重试策略投递
            event.status = crate::domain::models::WebhookStatus::Failed;
            event.next_retry_at = Some(chrono::Utc::now() + chrono::Duration::seconds(5));
            if let Err(update_err) = self.event_repository.update(&event).await {
                error!(
                    "Failed to schedule retry for webhook event {}: {}",
                    event.id, update_err
                );
            }
            return Err(e);
        }

        // 内联发送成功：终结事件（写回 Delivered）。
        // 外发已成功但状态回写失败会造成"外部已投递、DB 仍停留 Processing"，
        // 后续 retry_failed 经 claim_pending 可能重复认领并再次外发（客户可见的
        // 重复投递事故）。故回写失败时重试 1 次，仍失败则显式返回 Err
        // （语义：已发送但状态未知）；重复外发由 event msg_id 幂等兜底
        // （R-data-integrity-010）。
        event.status = crate::domain::models::WebhookStatus::Delivered;
        event.delivered_at = Some(chrono::Utc::now());
        if let Err(update_err) = self.event_repository.update(&event).await {
            error!(
                "Failed to mark webhook event {} delivered (attempt 1/2), retrying: {}",
                event.id, update_err
            );
            if let Err(retry_err) = self.event_repository.update(&event).await {
                error!(
                    "CRITICAL: webhook event {} was delivered externally but status \
                     write-back failed after retry: {}; returning Err \
                     (sent-but-status-unknown, dedup relies on event msg_id idempotency)",
                    event.id, retry_err
                );
                return Err(anyhow!(
                    "webhook event {} delivered but status write-back failed: {}",
                    event.id,
                    retry_err
                ));
            }
        }

        info!("Triggered webhook {} for event {}", webhook_id, event.id);
        Ok(())
    }

    async fn retry_failed(&self, limit: u64) -> Result<u64> {
        // 原子认领，避免与 webhook-worker 并发重复投递同一事件
        let pending = self
            .event_repository
            .claim_pending(limit)
            .await
            .map_err(|e| anyhow!("Failed to claim pending webhook events: {}", e))?;

        let mut success_count: u64 = 0;
        for mut event in pending {
            if !event.can_retry() {
                continue;
            }

            match self.webhook_service.send_webhook(&event).await {
                Ok(status) => {
                    event.record_attempt(true, Some(status as i32), None);
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
