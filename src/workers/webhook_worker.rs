// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::webhook::{WebhookEvent, WebhookStatus};
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::services::webhook_service::WebhookService;
use crate::utils::{errors::WorkerError, retry_policy::RetryPolicy};
use crate::workers::worker::Worker;
use anyhow::Result;
use chrono::Utc;
use metrics::counter;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

/// Webhook工作器
///
/// 负责处理webhook事件的发送和重试
pub struct WebhookWorker {
    /// Webhook事件仓库
    repo: Arc<dyn WebhookEventRepository>,
    /// Webhook服务
    webhook_service: Arc<dyn WebhookService>,
    /// 重试策略
    retry_policy: RetryPolicy,
}

impl WebhookWorker {
    /// 创建新的webhook工作器
    pub fn new(
        repo: Arc<dyn WebhookEventRepository>,
        webhook_service: Arc<dyn WebhookService>,
        retry_policy: RetryPolicy,
    ) -> Self {
        Self {
            repo,
            webhook_service,
            retry_policy,
        }
    }

    /// 使用默认重试策略创建webhook工作器
    pub fn with_default_policy(
        repo: Arc<dyn WebhookEventRepository>,
        webhook_service: Arc<dyn WebhookService>,
    ) -> Self {
        Self::new(repo, webhook_service, RetryPolicy::default())
    }

    /// 处理待处理的webhook事件
    pub async fn process_pending_webhooks(&self) -> Result<()> {
        let pending_events = self
            .repo
            .find_pending(100)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to find pending events: {}", e))?;

        if !pending_events.is_empty() {
            info!("Processing {} pending webhook events", pending_events.len());

            for event in pending_events {
                if let Err(e) = self.process_webhook_event(event).await {
                    error!("Failed to process webhook event: {}", e);
                }
            }
        }

        Ok(())
    }

    /// 处理单个webhook事件
    async fn process_webhook_event(&self, mut event: WebhookEvent) -> Result<()> {
        info!(
            "Processing webhook event {} for URL {} (attempt {})",
            event.id, event.webhook_url, event.attempt_count
        );

        // 尝试发送webhook
        match self.webhook_service.send_webhook(&event).await {
            Ok(_) => {
                info!("Successfully delivered webhook {}", event.id);
                event.status = WebhookStatus::Delivered;
                event.delivered_at = Some(Utc::now());
                event.response_status = Some(200);
                self.repo
                    .update(&event)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to update event: {}", e))?;

                counter!("webhook_delivery_success_total").increment(1);
                Ok(())
            }
            Err(e) => {
                error!("Failed to deliver webhook {}: {}", event.id, e);

                // 尝试解析错误中的 HTTP 状态码
                let error_msg = e.to_string();
                if error_msg.contains("status 500") {
                    event.response_status = Some(500);
                } else if error_msg.contains("status 400") {
                    event.response_status = Some(400);
                } else if error_msg.contains("status 404") {
                    event.response_status = Some(404);
                }

                self.handle_webhook_failure(event, e).await
            }
        }
    }

    /// 处理webhook发送失败
    async fn handle_webhook_failure(
        &self,
        mut event: WebhookEvent,
        error: anyhow::Error,
    ) -> Result<()> {
        let new_attempt_count = event.attempt_count + 1;

        // 检查是否应该重试
        if !self
            .retry_policy
            .should_retry_with_error(new_attempt_count as u32, &error)
        {
            // 达到最大重试次数或错误不可重试，移动到死信队列
            event.status = WebhookStatus::Dead;
            event.error_message = Some(error.to_string());
            event.attempt_count = new_attempt_count;

            warn!(
                "Webhook {} moved to dead letter state after {} attempts: {}",
                event.id, new_attempt_count, error
            );
            counter!("webhook_dead_letter_total", "reason" => error.to_string()).increment(1);
        } else {
            // 计算下次重试时间
            let next_retry = self
                .retry_policy
                .next_retry_time(new_attempt_count as u32, Utc::now());

            event.status = WebhookStatus::Failed;
            event.attempt_count = new_attempt_count;
            event.next_retry_at = Some(next_retry);
            event.error_message = Some(error.to_string());

            info!(
                "Webhook {} will be retried at {} (attempt {})",
                event.id, next_retry, new_attempt_count
            );
            counter!("webhook_retry_scheduled_total", "attempt" => new_attempt_count.to_string())
                .increment(1);
        }

        self.repo
            .update(&event)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to update event: {}", e))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl Worker for WebhookWorker {
    async fn run(&self) -> Result<(), WorkerError> {
        info!("Starting webhook worker");

        loop {
            if let Err(e) = self.process_pending_webhooks().await {
                error!("Error processing pending webhooks: {}", e);
            }

            sleep(Duration::from_secs(5)).await;
        }
    }

    fn name(&self) -> &str {
        "webhook_worker"
    }
}


