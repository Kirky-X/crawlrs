// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::webhook::WebhookEvent;
use crate::domain::services::webhook_service::WebhookService;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::Duration;

type HmacSha256 = Hmac<Sha256>;

/// 脱敏 webhook 响应消息
///
/// 截断并清理 webhook 响应中的敏感信息
fn sanitize_webhook_response(body: &str) -> String {
    const MAX_LENGTH: usize = 200;

    if body.is_empty() {
        return String::new();
    }

    let truncated = if body.len() > MAX_LENGTH {
        format!("{}... (truncated)", &body[..MAX_LENGTH])
    } else {
        body.to_string()
    };

    // 移除可能的敏感信息（API 密钥、令牌等）
    let sanitized = truncated.replace(
        |c: char| !c.is_ascii_graphic() && !c.is_ascii_whitespace(),
        "?",
    );

    sanitized
}

/// Webhook服务实现
pub struct WebhookServiceImpl {
    /// HTTP 客户端
    client: reqwest::Client,
    /// 签名密钥
    secret: String,
}

impl WebhookServiceImpl {
    /// 创建新的 Webhook 服务实现
    pub fn new(secret: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        Self { client, secret }
    }

    /// 为负载生成签名
    fn generate_signature(&self, payload: &str, timestamp: i64) -> String {
        let message = format!("{}.{}", timestamp, payload);
        let mut mac = match HmacSha256::new_from_slice(self.secret.as_bytes()) {
            Ok(mac) => mac,
            Err(e) => {
                tracing::error!("Failed to initialize HMAC: {}", e);
                return String::new();
            }
        };
        mac.update(message.as_bytes());
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }
}

#[async_trait]
impl WebhookService for WebhookServiceImpl {
    async fn send_webhook(&self, event: &WebhookEvent) -> Result<()> {
        let timestamp = chrono::Utc::now().timestamp();
        let payload_str = serde_json::to_string(&event.payload)?;
        let signature = self.generate_signature(&payload_str, timestamp);

        let response = self
            .client
            .post(&event.webhook_url)
            .header("Content-Type", "application/json")
            .header("X-Crawlrs-Signature", signature)
            .header("X-Crawlrs-Timestamp", timestamp.to_string())
            .header("X-Crawlrs-Event-ID", event.id.to_string())
            .json(&event.payload)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let sanitized_body = sanitize_webhook_response(&body);
            tracing::warn!(
                "Webhook delivery failed with status {}: {}",
                status,
                sanitized_body
            );
            Err(anyhow!(
                "Webhook delivery failed with status {}: {}",
                status,
                sanitized_body
            ))
        }
    }
}
