// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::webhook::WebhookEvent;
use crate::domain::services::webhook_service::WebhookService;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;

type HmacSha256 = Hmac<Sha256>;

/// 最大允许的时间戳偏差（秒）
/// 用于防止重放攻击
/// 接收方 webhook handler 应使用此常量验证时间戳
const MAX_TIMESTAMP_AGE: i64 = 300; // 5分钟

/// 验证 webhook 时间戳是否在有效期内
/// 用于防止重放攻击
/// 接收方 webhook handler 应调用此函数验证请求时间戳
fn validate_timestamp(timestamp: i64) -> bool {
    let now = Utc::now().timestamp();
    let diff = (now - timestamp).abs();
    diff <= MAX_TIMESTAMP_AGE
}

/// 为负载生成签名（包含时间戳以防止重放攻击）
fn generate_signature(secret: &str, payload: &str, timestamp: i64) -> String {
    let message = format!("{}.{}", timestamp, payload);
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
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

/// 验证 webhook 签名
/// 供接收方使用以验证 webhook  authenticity 和 freshness
pub fn verify_webhook_signature(
    secret: &str,
    payload: &str,
    timestamp: i64,
    signature: &str,
) -> bool {
    // 首先验证时间戳是否在有效期内
    if !validate_timestamp(timestamp) {
        tracing::warn!("Webhook timestamp is outside valid window");
        return false;
    }

    // 重新计算签名并比较
    let expected_signature = generate_signature(secret, payload, timestamp);
    constant_time_eq(signature, &expected_signature)
}

/// 常数时间字符串比较以防止时序攻击
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).all(|(x, y)| x == y)
}

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
    truncated.replace(
        |c: char| !c.is_ascii_graphic() && !c.is_ascii_whitespace(),
        "?",
    )
}

/// Webhook服务实现
pub struct WebhookServiceImpl {
    /// HTTP 客户端 (通过依赖注入的单例)
    client: Arc<reqwest::Client>,
    /// 签名密钥
    secret: String,
}

impl WebhookServiceImpl {
    /// 创建新的 Webhook 服务实现 (使用依赖注入)
    ///
    /// # Arguments
    ///
    /// * `secret` - Webhook 签名密钥
    /// * `client` - HTTP 客户端实例 (通过 Arc 共享)
    pub fn new(secret: String, client: Arc<reqwest::Client>) -> Self {
        Self { client, secret }
    }

    /// 为负载生成签名（包含时间戳以防止重放攻击）
    fn generate_signature(&self, payload: &str, timestamp: i64) -> String {
        generate_signature(&self.secret, payload, timestamp)
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
