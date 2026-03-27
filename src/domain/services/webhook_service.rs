// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook Service
//!
//! Unified webhook service for task completion and failure notifications.
//! Supports dependency injection via Shaku.

use crate::application::dto::scrape_request::ScrapeRequestDto;
use crate::domain::models::Task;
use crate::domain::models::{WebhookEvent, WebhookEventType};
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::services::webhook_sender::WebhookSender;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;
use shaku::{Component, Interface};
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// Webhook服务接口（支持 DI）
#[async_trait]
pub trait WebhookService: Interface + Send + Sync {
    /// 发送Webhook事件
    async fn send_webhook(&self, event: &WebhookEvent) -> Result<()>;

    /// 触发任务完成 webhook
    async fn trigger_completion(&self, task: &Task) -> Result<()>;

    /// 触发任务失败 webhook
    async fn trigger_failure(&self, task: &Task, error_msg: String) -> Result<()>;
}

/// Webhook服务实现
#[derive(Component)]
#[shaku(interface = WebhookService)]
pub struct WebhookServiceImpl {
    /// Webhook 发送器
    #[shaku(inject)]
    webhook_sender: Arc<dyn WebhookSender>,
    /// Webhook 签名密钥
    #[shaku(default = String::new())]
    secret: String,
    /// Webhook 事件仓库
    #[shaku(inject)]
    repository: Arc<dyn WebhookEventRepository>,
}

impl WebhookServiceImpl {
    /// 创建新的 Webhook 服务实现
    pub fn new(
        webhook_sender: Arc<dyn WebhookSender>,
        secret: String,
        repository: Arc<dyn WebhookEventRepository>,
    ) -> Self {
        Self {
            webhook_sender,
            secret,
            repository,
        }
    }

    /// 为负载生成签名（包含时间戳以防止重放攻击）
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

    /// 提取 webhook URL 从任务
    fn extract_webhook_url(&self, task: &Task) -> Option<String> {
        // Try to parse as ScrapeRequestDto first
        if let Ok(req) = serde_json::from_value::<ScrapeRequestDto>(task.payload.clone()) {
            return req.webhook;
        }

        // Fall back to extracting from payload directly
        task.payload
            .get("webhook")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// 获取事件类型
    fn get_event_type(&self, task: &Task) -> WebhookEventType {
        match task.task_type.as_str() {
            "scrape" => WebhookEventType::ScrapeCompleted,
            "crawl" => WebhookEventType::CrawlCompleted,
            _ => WebhookEventType::Custom("extract.completed".to_string()),
        }
    }

    /// 获取失败事件类型
    fn get_failed_event_type(&self, task: &Task) -> WebhookEventType {
        match task.task_type.as_str() {
            "scrape" => WebhookEventType::ScrapeFailed,
            "crawl" => WebhookEventType::CrawlFailed,
            _ => WebhookEventType::Custom("extract.failed".to_string()),
        }
    }
}

#[async_trait]
impl WebhookService for WebhookServiceImpl {
    async fn send_webhook(&self, event: &WebhookEvent) -> Result<()> {
        let timestamp = chrono::Utc::now().timestamp();
        let payload_str = serde_json::to_string(&event.payload)?;
        let signature = self.generate_signature(&payload_str, timestamp);

        let mut headers = std::collections::HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert("X-Crawlrs-Signature".to_string(), signature);
        headers.insert("X-Crawlrs-Timestamp".to_string(), timestamp.to_string());
        headers.insert("X-Crawlrs-Event-ID".to_string(), event.id.to_string());

        let payload = serde_json::from_str(&payload_str)?;

        self.webhook_sender
            .send(&event.webhook_url, &payload, Some(&headers))
            .await?;

        info!("Webhook sent successfully for event {}", event.id);
        Ok(())
    }

    async fn trigger_completion(&self, task: &Task) -> Result<()> {
        let webhook_url = match self.extract_webhook_url(task) {
            Some(url) => url,
            None => {
                info!("No webhook URL found for task {}", task.id);
                return Ok(());
            }
        };

        let event_type = self.get_event_type(task);
        self.send_task_webhook(task, webhook_url, event_type, None)
            .await
    }

    async fn trigger_failure(&self, task: &Task, error_msg: String) -> Result<()> {
        let webhook_url = match self.extract_webhook_url(task) {
            Some(url) => url,
            None => {
                info!("No webhook URL found for task {}", task.id);
                return Ok(());
            }
        };

        let event_type = self.get_failed_event_type(task);
        self.send_task_webhook(task, webhook_url, event_type, Some(error_msg))
            .await
    }
}

impl WebhookServiceImpl {
    /// 发送任务 webhook 事件
    async fn send_task_webhook(
        &self,
        task: &Task,
        webhook_url: String,
        event_type: WebhookEventType,
        error_msg: Option<String>,
    ) -> Result<()> {
        info!(
            "Triggering webhook {:?} for task {} (url: {})",
            event_type, task.id, webhook_url
        );

        let mut payload = json!({
            "task_id": task.id,
            "status": if error_msg.is_some() { "failed" } else { "completed" },
            "url": task.url,
            "timestamp": Utc::now().timestamp()
        });

        if let Some(msg) = error_msg {
            payload["error"] = json!(msg);
        }

        let event = WebhookEvent::new(
            Uuid::new_v4(),
            task.team_id,
            Uuid::nil(),
            event_type,
            payload,
            webhook_url,
        );

        // Save event to repository
        if let Err(e) = self.repository.create(&event).await {
            error!("Failed to create webhook event for task {}: {}", task.id, e);
            return Err(anyhow!("Failed to create webhook event: {}", e));
        }

        // Send webhook
        if let Err(e) = self.send_webhook(&event).await {
            error!("Failed to send webhook for task {}: {}", task.id, e);
            return Err(e);
        }

        Ok(())
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::TaskStatus;
    use crate::domain::repositories::task_repository::RepositoryError;
    use async_trait::async_trait;
    use chrono::FixedOffset;
    use serde_json::Value;
    use std::collections::HashMap;

    struct MockWebhookEventRepository;

    #[async_trait]
    impl WebhookEventRepository for MockWebhookEventRepository {
        async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
            Ok(event.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<WebhookEvent>, RepositoryError> {
            Ok(None)
        }

        async fn find_pending(&self, _limit: u64) -> Result<Vec<WebhookEvent>, RepositoryError> {
            Ok(vec![])
        }

        async fn find_by_team_id_paginated(
            &self,
            _team_id: Uuid,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<WebhookEvent>, RepositoryError> {
            Ok(vec![])
        }

        async fn count_by_team_id(&self, _team_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn update(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
            Ok(event.clone())
        }
    }

    struct MockWebhookSender;

    #[async_trait]
    impl WebhookSender for MockWebhookSender {
        async fn send(
            &self,
            _url: &str,
            _payload: &Value,
            _headers: Option<&HashMap<String, String>>,
        ) -> Result<()> {
            Ok(())
        }

        async fn send_with_status(
            &self,
            _url: &str,
            _payload: &Value,
            _headers: Option<&HashMap<String, String>>,
        ) -> Result<u16> {
            Ok(200)
        }
    }

    fn create_test_task() -> Task {
        let now = Utc::now();
        Task {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "http://example.com".to_string(),
            task_type: crate::domain::models::TaskType::Scrape,
            status: crate::domain::models::TaskStatus::Completed,
            payload: serde_json::json!({
                "url": "http://example.com",
                "webhook": "https://example.com/webhook"
            }),
            attempt_count: 1,
            max_retries: 3,
            scheduled_at: None,
            created_at: now,
            updated_at: now,
            priority: 0,
            retry_count: 0,
            expires_at: None,
            started_at: None,
            completed_at: None,
            crawl_id: None,
            lock_token: None,
            lock_expires_at: None,
        }
    }

    #[tokio::test]
    async fn test_extract_webhook_url() {
        let webhook_sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender);
        let repo = Arc::new(MockWebhookEventRepository);
        let service = WebhookServiceImpl::new(webhook_sender, "secret".to_string(), repo);

        let task = create_test_task();
        let url = service.extract_webhook_url(&task);
        assert_eq!(url, Some("https://example.com/webhook".to_string()));
    }

    #[tokio::test]
    async fn test_no_webhook_no_trigger() {
        let webhook_sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender);
        let repo = Arc::new(MockWebhookEventRepository);
        let service = WebhookServiceImpl::new(webhook_sender, "secret".to_string(), repo);

        let mut task = create_test_task();
        task.payload = serde_json::json!({"url": "http://example.com"}); // No webhook

        let result = service.trigger_completion(&task).await;
        assert!(result.is_ok());
    }
}
