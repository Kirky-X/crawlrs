// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::domain::models::webhook::{WebhookEvent, WebhookStatus};
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use chrono::Utc;
use futures::StreamExt;
use hmac::{Hmac, Mac};
use metrics::{counter, histogram};
use rand::Rng;
use reqwest::{header, Client};

use sha2::Sha256;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info};

use std::sync::Arc;

/// Webhook工作器
#[derive(Clone)]
pub struct WebhookWorker<R: WebhookEventRepository> {
    /// 仓库
    repo: Arc<R>,
    /// Webhook 密钥
    secret: String,
    /// HTTP客户端
    client: Client,
}

impl<R: WebhookEventRepository> WebhookWorker<R> {
    /// 创建新的Webhook工作器实例
    ///
    /// # 参数
    ///
    /// * `repo` - 仓库
    /// * `secret` - Webhook 密钥
    ///
    /// # 返回值
    ///
    /// 返回新的Webhook工作器实例
    pub fn new(repo: Arc<R>, secret: String) -> Self {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("Crawlrs-Webhook/0.1.0"),
        );
        Self {
            repo,
            secret,
            client: Client::builder().default_headers(headers).build().unwrap(),
        }
    }

    /// 运行Webhook工作器
    ///
    /// 启动Webhook处理循环，定期处理待处理的Webhook事件
    pub async fn run(&self) {
        info!("Webhook worker started");
        loop {
            if let Err(e) = self.process_pending_webhooks().await {
                error!("Error processing webhooks: {}", e);
            }
            sleep(Duration::from_secs(5)).await;
        }
    }

    /// 处理待处理的Webhook事件
    ///
    /// 从数据库中获取待处理的Webhook事件并发送
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 处理成功
    /// * `Err(anyhow::Error)` - 处理失败
    pub async fn process_pending_webhooks(&self) -> anyhow::Result<()> {
        // Batch size
        let batch_size = 50;

        let events = self.repo.find_pending(batch_size).await?;

        if events.is_empty() {
            return Ok(());
        }

        info!("Processing {} pending webhooks", events.len());

        // Process in parallel with bounded concurrency
        let worker = self;
        futures::stream::iter(events)
            .for_each_concurrent(10, |event| {
                let w = worker;
                async move {
                    if let Err(e) = w.deliver_webhook(event).await {
                        error!("Failed to deliver webhook: {}", e);
                    }
                }
            })
            .await;

        Ok(())
    }

    async fn deliver_webhook(&self, mut event: WebhookEvent) -> anyhow::Result<()> {
        info!("Delivering webhook {} to {}", event.id, event.webhook_url);
        counter!("webhook_delivery_attempts_total").increment(1);

        let start = std::time::Instant::now();

        // Create signature
        let secret = self.secret.as_bytes();
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
        mac.update(event.payload.to_string().as_bytes());
        let signature = mac.finalize().into_bytes();
        let signature_hex = hex::encode(signature);

        let response = self
            .client
            .post(&event.webhook_url)
            .header("X-Crawlrs-Signature", signature_hex)
            .header("X-Crawlrs-Event", event.event_type.to_string())
            .json(&event.payload)
            .timeout(Duration::from_secs(10))
            .send()
            .await;

        let duration = start.elapsed();
        histogram!("webhook_delivery_duration_seconds").record(duration.as_secs_f64());

        match response {
            Ok(resp) => {
                // Record response status
                event.response_status = Some(resp.status().as_u16() as i32);

                if resp.status().is_success() {
                    event.status = WebhookStatus::Delivered;
                    event.delivered_at = Some(Utc::now());

                    info!("Webhook {} delivered successfully", event.id);
                    self.repo.update(&event).await?;
                    counter!("webhook_delivery_success_total").increment(1);
                } else {
                    // Non-success status code
                    error!(
                        "Webhook {} delivery failed with status: {}",
                        event.id,
                        resp.status()
                    );
                    self.handle_failure(event).await?;
                    counter!("webhook_delivery_failed_total", "reason" => "http_error")
                        .increment(1);
                }
            }
            Err(e) => {
                // Network or other error
                error!("Webhook {} delivery failed with error: {}", event.id, e);
                event.error_message = Some(e.to_string());
                self.handle_failure(event).await?;
                counter!("webhook_delivery_failed_total", "reason" => "network_error").increment(1);
            }
        }

        Ok(())
    }

    async fn handle_failure(&self, mut event: WebhookEvent) -> anyhow::Result<()> {
        let new_attempt_count = event.attempt_count + 1;

        if new_attempt_count >= event.max_retries {
            event.status = WebhookStatus::Dead; // Dead Letter Queue equivalent
            info!(
                "Webhook moved to dead letter state after {} retries",
                event.max_retries
            );
            counter!("webhook_dead_letter_total").increment(1);
        } else {
            event.status = WebhookStatus::Failed;
            event.attempt_count = new_attempt_count;

            // Exponential backoff with jitter
            let base_backoff = 2u64.pow(new_attempt_count as u32);
            let jitter = rand::thread_rng().gen_range(0..base_backoff / 2);
            let backoff = base_backoff + jitter;

            event.next_retry_at = Some(Utc::now() + chrono::Duration::seconds(backoff as i64));
        }

        self.repo.update(&event).await?;
        Ok(())
    }
}
