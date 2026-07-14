// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook Service
//!
//! Unified webhook service for task completion and failure notifications.
//! Supports dependency injection via Shaku.

use crate::application::dto::scrape_request::ScrapeRequestDto;
use crate::domain::models::{Task, Webhook};
use crate::domain::models::{WebhookEvent, WebhookEventType};
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::domain::services::webhook_sender::WebhookSender;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Utc;
use hmac::{Hmac, Mac};
use log::{error, info};
use serde_json::json;
use sha2::Sha256;
use std::sync::Arc;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// Webhook服务接口（支持 DI）
#[async_trait]
pub trait WebhookService: Send + Sync {
    /// 发送Webhook事件
    async fn send_webhook(&self, event: &WebhookEvent) -> Result<()>;

    /// 触发任务完成 webhook
    async fn trigger_completion(&self, task: &Task) -> Result<()>;

    /// 触发任务失败 webhook
    async fn trigger_failure(&self, task: &Task, error_msg: String) -> Result<()>;
}

/// Webhook服务实现
pub struct WebhookServiceImpl {
    /// Webhook 发送器
    webhook_sender: Arc<dyn WebhookSender>,
    /// Webhook 签名密钥
    secret: String,
    /// Webhook 事件仓库
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
                log::error!("Failed to initialize HMAC: {}", e);
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

// === Section: WebhookManagementService (扩展接口) ===

/// Webhook 管理服务接口（扩展）
///
/// 提供 webhook 的注册、触发、重试和列表功能。
/// 与 `WebhookService` 互补——后者专注于发送通知，
/// 本接口专注于 webhook 端点的生命周期管理与批量重试。
#[async_trait]
pub trait WebhookManagementService: Send + Sync {
    /// 注册新的 webhook 端点
    ///
    /// # 参数
    /// * `team_id` - 团队 ID
    /// * `url` - webhook 端点 URL（必须是 http:// 或 https://）
    ///
    /// # 返回值
    /// * `Ok(Webhook)` - 注册成功
    /// * `Err` - URL 无效或持久化失败
    async fn register_webhook(&self, team_id: Uuid, url: String) -> Result<Webhook>;

    /// 触发指定 webhook 发送事件
    ///
    /// # 参数
    /// * `webhook_id` - 目标 webhook ID
    /// * `event_type` - 事件类型
    /// * `payload` - 事件负载（JSON）
    ///
    /// # 返回值
    /// * `Ok(())` - 触发并发送成功
    /// * `Err` - webhook 不存在、事件持久化失败或发送失败
    async fn trigger_webhook(
        &self,
        webhook_id: Uuid,
        event_type: WebhookEventType,
        payload: serde_json::Value,
    ) -> Result<()>;

    /// 重试失败的 webhook 事件
    ///
    /// 从事件仓库中取出待处理事件，逐个尝试重新发送，
    /// 根据发送结果更新事件状态（成功/失败/死亡）。
    ///
    /// # 参数
    /// * `limit` - 最多处理的事件数量
    ///
    /// # 返回值
    /// * `Ok(u64)` - 成功重试的事件数量
    /// * `Err` - 查询或更新失败
    async fn retry_failed(&self, limit: u64) -> Result<u64>;

    /// 列出团队的所有 webhook
    ///
    /// # 参数
    /// * `team_id` - 团队 ID
    ///
    /// # 返回值
    /// * `Ok(Vec<Webhook>)` - webhook 列表
    /// * `Err` - 查询失败
    async fn list_webhooks(&self, team_id: Uuid) -> Result<Vec<Webhook>>;
}

/// Webhook 管理服务实现
///
/// 通过组合 `WebhookService` 复用已有的签名生成与发送逻辑，
/// 避免代码重复。DI 注册在 Phase 11 统一处理。
pub struct WebhookManagementServiceImpl {
    /// Webhook 仓库（端点 CRUD）
    webhook_repository: Arc<dyn WebhookRepository>,
    /// Webhook 事件仓库（事件持久化）
    event_repository: Arc<dyn WebhookEventRepository>,
    /// Webhook 发送服务（复用现有签名+发送逻辑）
    webhook_service: Arc<dyn WebhookService>,
}

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

        let event = WebhookEvent::new(
            Uuid::new_v4(),
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
            log::error!("Failed to initialize HMAC: {}", e);
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
        log::warn!("Webhook timestamp is outside valid window");
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
    use crate::domain::repositories::task_repository::RepositoryError;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    /// Repository mock that always succeeds and tracks created events
    #[derive(Default)]
    struct MockWebhookEventRepository {
        created_count: AtomicU32,
    }

    #[async_trait]
    impl WebhookEventRepository for MockWebhookEventRepository {
        async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
            self.created_count.fetch_add(1, Ordering::SeqCst);
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

    /// Repository mock that always fails on create
    struct FailingWebhookEventRepository;

    #[async_trait]
    impl WebhookEventRepository for FailingWebhookEventRepository {
        async fn create(&self, _event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
            Err(RepositoryError::Database(anyhow::anyhow!("repo down")))
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

    /// Sender mock that always succeeds
    #[derive(Default)]
    struct MockWebhookSender {
        sent_count: AtomicU32,
    }

    #[async_trait]
    impl WebhookSender for MockWebhookSender {
        async fn send(
            &self,
            _url: &str,
            _payload: &Value,
            _headers: Option<&HashMap<String, String>>,
        ) -> Result<()> {
            self.sent_count.fetch_add(1, Ordering::SeqCst);
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

    /// Sender mock that always fails
    struct FailingWebhookSender;

    #[async_trait]
    impl WebhookSender for FailingWebhookSender {
        async fn send(
            &self,
            _url: &str,
            _payload: &Value,
            _headers: Option<&HashMap<String, String>>,
        ) -> Result<()> {
            Err(anyhow!("send failed"))
        }

        async fn send_with_status(
            &self,
            _url: &str,
            _payload: &Value,
            _headers: Option<&HashMap<String, String>>,
        ) -> Result<u16> {
            Err(anyhow!("send_with_status failed"))
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

    fn create_test_event() -> WebhookEvent {
        WebhookEvent::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::nil(),
            WebhookEventType::ScrapeCompleted,
            serde_json::json!({"task_id": "abc"}),
            "https://example.com/webhook".to_string(),
        )
    }

    fn make_service(
        sender: Arc<dyn WebhookSender>,
        repo: Arc<dyn WebhookEventRepository>,
        secret: &str,
    ) -> WebhookServiceImpl {
        WebhookServiceImpl::new(sender, secret.to_string(), repo)
    }

    // ---- extract_webhook_url ----

    #[tokio::test]
    async fn test_extract_webhook_url() {
        let webhook_sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = WebhookServiceImpl::new(webhook_sender, "secret".to_string(), repo);

        let task = create_test_task();
        let url = service.extract_webhook_url(&task);
        assert_eq!(url, Some("https://example.com/webhook".to_string()));
    }

    #[test]
    fn test_extract_webhook_url_from_raw_payload_when_dto_parse_fails() {
        let webhook_sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = WebhookServiceImpl::new(webhook_sender, "secret".to_string(), repo);

        // Payload that won't deserialize as ScrapeRequestDto (missing required `url`)
        // but contains a `webhook` string field -> fallback path
        let mut task = create_test_task();
        task.payload = serde_json::json!({"webhook": "https://fallback.example.com/hook"});
        let url = service.extract_webhook_url(&task);
        assert_eq!(url, Some("https://fallback.example.com/hook".to_string()));
    }

    #[test]
    fn test_extract_webhook_url_returns_none_when_no_webhook() {
        let webhook_sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = WebhookServiceImpl::new(webhook_sender, "secret".to_string(), repo);

        let mut task = create_test_task();
        task.payload = serde_json::json!({"url": "http://example.com"});
        assert!(service.extract_webhook_url(&task).is_none());
    }

    #[test]
    fn test_extract_webhook_url_returns_none_when_webhook_not_string() {
        let webhook_sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = WebhookServiceImpl::new(webhook_sender, "secret".to_string(), repo);

        let mut task = create_test_task();
        // webhook field is a number, not a string -> as_str() returns None
        task.payload = serde_json::json!({"webhook": 123});
        assert!(service.extract_webhook_url(&task).is_none());
    }

    // ---- get_event_type / get_failed_event_type ----

    #[test]
    fn test_get_event_type_scrape() {
        let service = make_service(
            Arc::new(MockWebhookSender::default()),
            Arc::new(MockWebhookEventRepository::default()),
            "secret",
        );
        let mut task = create_test_task();
        task.task_type = crate::domain::models::TaskType::Scrape;
        assert_eq!(
            service.get_event_type(&task),
            WebhookEventType::ScrapeCompleted
        );
    }

    #[test]
    fn test_get_event_type_crawl() {
        let service = make_service(
            Arc::new(MockWebhookSender::default()),
            Arc::new(MockWebhookEventRepository::default()),
            "secret",
        );
        let mut task = create_test_task();
        task.task_type = crate::domain::models::TaskType::Crawl;
        assert_eq!(
            service.get_event_type(&task),
            WebhookEventType::CrawlCompleted
        );
    }

    #[test]
    fn test_get_event_type_extract_returns_custom() {
        let service = make_service(
            Arc::new(MockWebhookSender::default()),
            Arc::new(MockWebhookEventRepository::default()),
            "secret",
        );
        let mut task = create_test_task();
        task.task_type = crate::domain::models::TaskType::Extract;
        assert_eq!(
            service.get_event_type(&task),
            WebhookEventType::Custom("extract.completed".to_string())
        );
    }

    #[test]
    fn test_get_failed_event_type_scrape() {
        let service = make_service(
            Arc::new(MockWebhookSender::default()),
            Arc::new(MockWebhookEventRepository::default()),
            "secret",
        );
        let mut task = create_test_task();
        task.task_type = crate::domain::models::TaskType::Scrape;
        assert_eq!(
            service.get_failed_event_type(&task),
            WebhookEventType::ScrapeFailed
        );
    }

    #[test]
    fn test_get_failed_event_type_crawl() {
        let service = make_service(
            Arc::new(MockWebhookSender::default()),
            Arc::new(MockWebhookEventRepository::default()),
            "secret",
        );
        let mut task = create_test_task();
        task.task_type = crate::domain::models::TaskType::Crawl;
        assert_eq!(
            service.get_failed_event_type(&task),
            WebhookEventType::CrawlFailed
        );
    }

    #[test]
    fn test_get_failed_event_type_extract_returns_custom() {
        let service = make_service(
            Arc::new(MockWebhookSender::default()),
            Arc::new(MockWebhookEventRepository::default()),
            "secret",
        );
        let mut task = create_test_task();
        task.task_type = crate::domain::models::TaskType::Extract;
        assert_eq!(
            service.get_failed_event_type(&task),
            WebhookEventType::Custom("extract.failed".to_string())
        );
    }

    // ---- generate_signature (method) ----

    #[test]
    fn test_generate_signature_method_returns_hex() {
        let service = make_service(
            Arc::new(MockWebhookSender::default()),
            Arc::new(MockWebhookEventRepository::default()),
            "supersecret",
        );
        let sig = service.generate_signature(r#"{"a":1}"#, 1_700_000_000);
        // HMAC-SHA256 produces 32 bytes -> 64 hex chars
        assert_eq!(sig.len(), 64);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_signature_method_is_deterministic() {
        let service = make_service(
            Arc::new(MockWebhookSender::default()),
            Arc::new(MockWebhookEventRepository::default()),
            "supersecret",
        );
        let sig1 = service.generate_signature("payload", 1_234);
        let sig2 = service.generate_signature("payload", 1_234);
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_generate_signature_method_changes_with_timestamp() {
        let service = make_service(
            Arc::new(MockWebhookSender::default()),
            Arc::new(MockWebhookEventRepository::default()),
            "supersecret",
        );
        let sig1 = service.generate_signature("payload", 1_234);
        let sig2 = service.generate_signature("payload", 1_235);
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_generate_signature_method_changes_with_payload() {
        let service = make_service(
            Arc::new(MockWebhookSender::default()),
            Arc::new(MockWebhookEventRepository::default()),
            "supersecret",
        );
        let sig1 = service.generate_signature("payload1", 1_234);
        let sig2 = service.generate_signature("payload2", 1_234);
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_generate_signature_method_with_empty_secret_returns_nonempty() {
        // HMAC accepts any key length including empty; should still produce a signature
        let service = make_service(
            Arc::new(MockWebhookSender::default()),
            Arc::new(MockWebhookEventRepository::default()),
            "",
        );
        let sig = service.generate_signature("payload", 1);
        // HMAC-SHA256 accepts empty key, so we get a valid 64-char hex
        assert_eq!(sig.len(), 64);
    }

    // ---- send_webhook ----

    #[tokio::test]
    async fn test_send_webhook_success() {
        let sender = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender.clone(), repo, "secret");
        let event = create_test_event();
        let result = service.send_webhook(&event).await;
        assert!(result.is_ok());
        assert_eq!(sender.sent_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_send_webhook_sender_failure_propagates() {
        let sender: Arc<dyn WebhookSender> = Arc::new(FailingWebhookSender);
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender, repo, "secret");
        let event = create_test_event();
        let result = service.send_webhook(&event).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("send failed"),
            "error should propagate sender msg"
        );
    }

    #[tokio::test]
    async fn test_send_webhook_includes_signature_and_timestamp_headers() {
        // Use a sender that captures headers
        use std::sync::Mutex;

        struct HeaderCapturingSender {
            captured: Mutex<Option<HashMap<String, String>>>,
        }

        #[async_trait]
        impl WebhookSender for HeaderCapturingSender {
            async fn send(
                &self,
                _url: &str,
                _payload: &Value,
                headers: Option<&HashMap<String, String>>,
            ) -> Result<()> {
                *self.captured.lock().unwrap() = headers.cloned();
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

        let sender = Arc::new(HeaderCapturingSender {
            captured: Mutex::new(None),
        });
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender.clone(), repo, "mysecret");
        let event = create_test_event();

        service.send_webhook(&event).await.expect("send ok");

        let captured = sender
            .captured
            .lock()
            .unwrap()
            .clone()
            .expect("headers captured");
        assert_eq!(
            captured.get("Content-Type").map(|s| s.as_str()),
            Some("application/json")
        );
        assert!(captured.contains_key("X-Crawlrs-Signature"));
        assert!(captured.contains_key("X-Crawlrs-Timestamp"));
        assert_eq!(
            captured.get("X-Crawlrs-Event-ID").map(|s| s.as_str()),
            Some(event.id.to_string().as_str())
        );
    }

    // ---- trigger_completion ----

    #[tokio::test]
    async fn test_no_webhook_no_trigger() {
        let webhook_sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = WebhookServiceImpl::new(webhook_sender, "secret".to_string(), repo);

        let mut task = create_test_task();
        task.payload = serde_json::json!({"url": "http://example.com"}); // No webhook

        let result = service.trigger_completion(&task).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_trigger_completion_with_webhook_succeeds() {
        let sender = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender.clone(), repo.clone(), "secret");

        let task = create_test_task(); // has webhook URL
        let result = service.trigger_completion(&task).await;
        assert!(result.is_ok());
        assert_eq!(sender.sent_count.load(Ordering::SeqCst), 1);
        assert_eq!(repo.created_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_trigger_completion_for_crawl_task() {
        let sender = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender.clone(), repo, "secret");

        let mut task = create_test_task();
        task.task_type = crate::domain::models::TaskType::Crawl;
        let result = service.trigger_completion(&task).await;
        assert!(result.is_ok());
        assert_eq!(sender.sent_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_trigger_completion_for_extract_task_uses_custom_event() {
        let sender = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender.clone(), repo, "secret");

        let mut task = create_test_task();
        task.task_type = crate::domain::models::TaskType::Extract;
        let result = service.trigger_completion(&task).await;
        assert!(result.is_ok());
        assert_eq!(sender.sent_count.load(Ordering::SeqCst), 1);
    }

    // ---- trigger_failure ----

    #[tokio::test]
    async fn test_trigger_failure_no_webhook_returns_ok() {
        let sender = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender.clone(), repo, "secret");

        let mut task = create_test_task();
        task.payload = serde_json::json!({"url": "http://example.com"}); // no webhook

        let result = service.trigger_failure(&task, "boom".to_string()).await;
        assert!(result.is_ok());
        assert_eq!(sender.sent_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_trigger_failure_with_webhook_succeeds() {
        let sender = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender.clone(), repo.clone(), "secret");

        let task = create_test_task();
        let result = service
            .trigger_failure(&task, "task failed".to_string())
            .await;
        assert!(result.is_ok());
        assert_eq!(sender.sent_count.load(Ordering::SeqCst), 1);
        assert_eq!(repo.created_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_trigger_failure_for_crawl_task() {
        let sender = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender.clone(), repo, "secret");

        let mut task = create_test_task();
        task.task_type = crate::domain::models::TaskType::Crawl;
        let result = service
            .trigger_failure(&task, "crawl error".to_string())
            .await;
        assert!(result.is_ok());
        assert_eq!(sender.sent_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_trigger_failure_for_extract_task_uses_custom_event() {
        let sender = Arc::new(MockWebhookSender::default());
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender.clone(), repo, "secret");

        let mut task = create_test_task();
        task.task_type = crate::domain::models::TaskType::Extract;
        let result = service
            .trigger_failure(&task, "extract error".to_string())
            .await;
        assert!(result.is_ok());
        assert_eq!(sender.sent_count.load(Ordering::SeqCst), 1);
    }

    // ---- send_task_webhook failure paths ----

    #[tokio::test]
    async fn test_send_task_webhook_repo_failure_propagates() {
        let sender = Arc::new(MockWebhookSender::default());
        let repo: Arc<dyn WebhookEventRepository> = Arc::new(FailingWebhookEventRepository);
        let service = make_service(sender.clone(), repo, "secret");

        let task = create_test_task();
        let result = service.trigger_completion(&task).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to create webhook event"),
            "should report repo failure, got: {}",
            err
        );
        // Sender should not have been called since repo failed first
        assert_eq!(sender.sent_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_send_task_webhook_sender_failure_propagates() {
        let sender: Arc<dyn WebhookSender> = Arc::new(FailingWebhookSender);
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender, repo.clone(), "secret");

        let task = create_test_task();
        let result = service.trigger_completion(&task).await;
        assert!(result.is_err());
        // Repo create was called (succeeded), but send failed
        assert_eq!(repo.created_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_trigger_failure_includes_error_in_payload() {
        // Use capturing sender to inspect payload
        use std::sync::Mutex;

        struct PayloadCapturingSender {
            captured: Mutex<Option<Value>>,
        }

        #[async_trait]
        impl WebhookSender for PayloadCapturingSender {
            async fn send(
                &self,
                _url: &str,
                payload: &Value,
                _headers: Option<&HashMap<String, String>>,
            ) -> Result<()> {
                *self.captured.lock().unwrap() = Some(payload.clone());
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

        let sender = Arc::new(PayloadCapturingSender {
            captured: Mutex::new(None),
        });
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender.clone(), repo, "secret");

        let task = create_test_task();
        let err_msg = "scrape blew up";
        service
            .trigger_failure(&task, err_msg.to_string())
            .await
            .expect("trigger should succeed");

        let payload = sender
            .captured
            .lock()
            .unwrap()
            .clone()
            .expect("payload captured");
        assert_eq!(payload["status"], json!("failed"));
        assert_eq!(payload["error"], json!(err_msg));
        assert_eq!(payload["task_id"], json!(task.id));
        assert_eq!(payload["url"], json!(task.url));
    }

    #[tokio::test]
    async fn test_trigger_completion_payload_has_completed_status() {
        use std::sync::Mutex;

        struct PayloadCapturingSender {
            captured: Mutex<Option<Value>>,
        }

        #[async_trait]
        impl WebhookSender for PayloadCapturingSender {
            async fn send(
                &self,
                _url: &str,
                payload: &Value,
                _headers: Option<&HashMap<String, String>>,
            ) -> Result<()> {
                *self.captured.lock().unwrap() = Some(payload.clone());
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

        let sender = Arc::new(PayloadCapturingSender {
            captured: Mutex::new(None),
        });
        let repo = Arc::new(MockWebhookEventRepository::default());
        let service = make_service(sender.clone(), repo, "secret");

        let task = create_test_task();
        service.trigger_completion(&task).await.expect("trigger ok");

        let payload = sender
            .captured
            .lock()
            .unwrap()
            .clone()
            .expect("payload captured");
        assert_eq!(payload["status"], json!("completed"));
        // No error field for completion
        assert!(payload.get("error").is_none() || payload["error"].is_null());
        assert_eq!(payload["task_id"], json!(task.id));
    }

    // ---- validate_timestamp (free fn) ----

    #[test]
    fn test_validate_timestamp_now_is_valid() {
        let now = Utc::now().timestamp();
        assert!(validate_timestamp(now));
    }

    #[test]
    fn test_validate_timestamp_within_window() {
        let now = Utc::now().timestamp();
        // Just inside the 5-minute window
        assert!(validate_timestamp(now - MAX_TIMESTAMP_AGE));
        assert!(validate_timestamp(now + MAX_TIMESTAMP_AGE));
    }

    #[test]
    fn test_validate_timestamp_outside_window() {
        let now = Utc::now().timestamp();
        // Just outside the window
        assert!(!validate_timestamp(now - MAX_TIMESTAMP_AGE - 1));
        assert!(!validate_timestamp(now + MAX_TIMESTAMP_AGE + 1));
    }

    #[test]
    fn test_validate_timestamp_far_past() {
        let now = Utc::now().timestamp();
        assert!(!validate_timestamp(now - 86_400 * 30)); // 30 days ago
    }

    // ---- generate_signature (free fn) ----

    #[test]
    fn test_generate_signature_free_fn_returns_hex() {
        let sig = generate_signature("secret", r#"{"x":1}"#, 1_700_000_000);
        assert_eq!(sig.len(), 64);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_signature_free_fn_deterministic() {
        let s1 = generate_signature("secret", "payload", 100);
        let s2 = generate_signature("secret", "payload", 100);
        assert_eq!(s1, s2);
    }

    #[test]
    fn test_generate_signature_free_fn_changes_with_secret() {
        let s1 = generate_signature("secret1", "payload", 100);
        let s2 = generate_signature("secret2", "payload", 100);
        assert_ne!(s1, s2);
    }

    // ---- verify_webhook_signature ----

    #[test]
    fn test_verify_webhook_signature_valid() {
        let secret = "mysecret";
        let payload = r#"{"task_id":"abc"}"#;
        let timestamp = Utc::now().timestamp();
        let signature = generate_signature(secret, payload, timestamp);
        assert!(verify_webhook_signature(
            secret, payload, timestamp, &signature
        ));
    }

    #[test]
    fn test_verify_webhook_signature_invalid_signature() {
        let secret = "mysecret";
        let payload = r#"{"task_id":"abc"}"#;
        let timestamp = Utc::now().timestamp();
        // Wrong signature
        assert!(!verify_webhook_signature(
            secret, payload, timestamp, "deadbeef"
        ));
    }

    #[test]
    fn test_verify_webhook_signature_wrong_secret() {
        let payload = r#"{"task_id":"abc"}"#;
        let timestamp = Utc::now().timestamp();
        let signature = generate_signature("real-secret", payload, timestamp);
        assert!(!verify_webhook_signature(
            "wrong-secret",
            payload,
            timestamp,
            &signature
        ));
    }

    #[test]
    fn test_verify_webhook_signature_wrong_payload() {
        let secret = "mysecret";
        let timestamp = Utc::now().timestamp();
        let signature = generate_signature(secret, r#"{"a":1}"#, timestamp);
        assert!(!verify_webhook_signature(
            secret,
            r#"{"a":2}"#,
            timestamp,
            &signature
        ));
    }

    #[test]
    fn test_verify_webhook_signature_old_timestamp_rejected() {
        let secret = "mysecret";
        let payload = r#"{"task_id":"abc"}"#;
        let timestamp = Utc::now().timestamp() - 86_400; // 1 day ago, outside window
        let signature = generate_signature(secret, payload, timestamp);
        // Even with correct signature, old timestamp should be rejected
        assert!(!verify_webhook_signature(
            secret, payload, timestamp, &signature
        ));
    }

    #[test]
    fn test_verify_webhook_signature_future_timestamp_rejected() {
        let secret = "mysecret";
        let payload = r#"{"task_id":"abc"}"#;
        let timestamp = Utc::now().timestamp() + 86_400; // 1 day in future
        let signature = generate_signature(secret, payload, timestamp);
        assert!(!verify_webhook_signature(
            secret, payload, timestamp, &signature
        ));
    }

    #[test]
    fn test_verify_webhook_signature_correct_signature_succeeds_at_boundary() {
        let secret = "mysecret";
        let payload = r#"{"task_id":"abc"}"#;
        let now = Utc::now().timestamp();
        let timestamp = now - MAX_TIMESTAMP_AGE; // exactly at boundary - should be valid (<=)
        let signature = generate_signature(secret, payload, timestamp);
        assert!(verify_webhook_signature(
            secret, payload, timestamp, &signature
        ));
    }

    // ---- constant_time_eq ----

    #[test]
    fn test_constant_time_eq_same_strings() {
        assert!(constant_time_eq("hello", "hello"));
        assert!(constant_time_eq("", ""));
        assert!(constant_time_eq("a", "a"));
    }

    #[test]
    fn test_constant_time_eq_different_strings() {
        assert!(!constant_time_eq("hello", "world"));
        assert!(!constant_time_eq("abc", "abd"));
    }

    #[test]
    fn test_constant_time_eq_different_lengths() {
        assert!(!constant_time_eq("a", "ab"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("", "a"));
    }

    #[test]
    fn test_constant_time_eq_case_sensitive() {
        assert!(!constant_time_eq("Hello", "hello"));
    }

    #[tokio::test]
    async fn test_mock_webhook_event_repo_remaining_methods_return_defaults() {
        let repo = MockWebhookEventRepository::default();
        let id = Uuid::new_v4();
        assert!(repo.find_by_id(id).await.unwrap().is_none());
        assert!(repo.find_pending(10).await.unwrap().is_empty());
        assert!(repo
            .find_by_team_id_paginated(id, 10, 0)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(repo.count_by_team_id(id).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_failing_webhook_event_repo_remaining_methods_return_defaults() {
        let repo = FailingWebhookEventRepository;
        let id = Uuid::new_v4();
        assert!(repo.find_by_id(id).await.unwrap().is_none());
        assert!(repo.find_pending(10).await.unwrap().is_empty());
        assert!(repo
            .find_by_team_id_paginated(id, 10, 0)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(repo.count_by_team_id(id).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_mock_webhook_sender_send_with_status_returns_200() {
        let sender = MockWebhookSender::default();
        let payload = json!({"test": true});
        let status = sender
            .send_with_status("https://example.com", &payload, None)
            .await
            .unwrap();
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn test_failing_webhook_sender_send_with_status_returns_error() {
        let sender = FailingWebhookSender;
        let payload = json!({"test": true});
        let result = sender
            .send_with_status("https://example.com", &payload, None)
            .await;
        assert!(result.is_err());
    }

    // ============ WebhookManagementService mocks ============

    /// 可配置的 Webhook 仓库 mock（内存存储）
    #[derive(Default)]
    struct MockWebhookRepository {
        webhooks: std::sync::Mutex<Vec<Webhook>>,
    }

    impl MockWebhookRepository {
        fn with_webhooks(webhooks: Vec<Webhook>) -> Self {
            Self {
                webhooks: std::sync::Mutex::new(webhooks),
            }
        }
    }

    #[async_trait]
    impl WebhookRepository for MockWebhookRepository {
        async fn create(&self, webhook: &Webhook) -> Result<Webhook, RepositoryError> {
            let mut wh = self.webhooks.lock().unwrap();
            wh.push(webhook.clone());
            Ok(webhook.clone())
        }

        async fn find_by_id(&self, id: Uuid) -> Result<Option<Webhook>, RepositoryError> {
            let wh = self.webhooks.lock().unwrap();
            Ok(wh.iter().find(|w| w.id == id).cloned())
        }

        async fn find_by_team_id(&self, team_id: Uuid) -> Result<Vec<Webhook>, RepositoryError> {
            let wh = self.webhooks.lock().unwrap();
            Ok(wh
                .iter()
                .filter(|w| w.team_id == team_id)
                .cloned()
                .collect())
        }
    }

    /// 始终失败的 Webhook 仓库 mock
    struct FailingWebhookRepository;

    #[async_trait]
    impl WebhookRepository for FailingWebhookRepository {
        async fn create(&self, _webhook: &Webhook) -> Result<Webhook, RepositoryError> {
            Err(RepositoryError::Database(anyhow::anyhow!(
                "webhook repo down"
            )))
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Webhook>, RepositoryError> {
            Err(RepositoryError::Database(anyhow::anyhow!(
                "webhook repo down"
            )))
        }

        async fn find_by_team_id(&self, _team_id: Uuid) -> Result<Vec<Webhook>, RepositoryError> {
            Err(RepositoryError::Database(anyhow::anyhow!(
                "webhook repo down"
            )))
        }
    }

    /// 可配置的 WebhookService mock
    #[derive(Default)]
    struct MockWebhookService {
        send_count: AtomicU32,
        should_fail: bool,
    }

    #[async_trait]
    impl WebhookService for MockWebhookService {
        async fn send_webhook(&self, _event: &WebhookEvent) -> Result<()> {
            self.send_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                Err(anyhow!("mock send failed"))
            } else {
                Ok(())
            }
        }

        async fn trigger_completion(&self, _task: &Task) -> Result<()> {
            Ok(())
        }

        async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> Result<()> {
            Ok(())
        }
    }

    /// 可配置的 WebhookEvent 仓库 mock（支持 find_pending 返回指定事件）
    #[derive(Default)]
    struct ConfigurableWebhookEventRepository {
        events: std::sync::Mutex<Vec<WebhookEvent>>,
        update_count: AtomicU32,
    }

    #[async_trait]
    impl WebhookEventRepository for ConfigurableWebhookEventRepository {
        async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
            self.events.lock().unwrap().push(event.clone());
            Ok(event.clone())
        }

        async fn find_by_id(&self, id: Uuid) -> Result<Option<WebhookEvent>, RepositoryError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.id == id)
                .cloned())
        }

        async fn find_pending(&self, _limit: u64) -> Result<Vec<WebhookEvent>, RepositoryError> {
            Ok(self.events.lock().unwrap().clone())
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
            self.update_count.fetch_add(1, Ordering::SeqCst);
            let mut events = self.events.lock().unwrap();
            if let Some(e) = events.iter_mut().find(|e| e.id == event.id) {
                *e = event.clone();
            }
            Ok(event.clone())
        }
    }

    /// find_pending 始终失败的 WebhookEvent 仓库 mock
    struct FindPendingFailingEventRepository;

    #[async_trait]
    impl WebhookEventRepository for FindPendingFailingEventRepository {
        async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
            Ok(event.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<WebhookEvent>, RepositoryError> {
            Ok(None)
        }

        async fn find_pending(&self, _limit: u64) -> Result<Vec<WebhookEvent>, RepositoryError> {
            Err(RepositoryError::Database(anyhow::anyhow!(
                "find_pending failed"
            )))
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

    fn make_management_service(
        webhook_repo: Arc<dyn WebhookRepository>,
        event_repo: Arc<dyn WebhookEventRepository>,
        webhook_service: Arc<dyn WebhookService>,
    ) -> WebhookManagementServiceImpl {
        WebhookManagementServiceImpl::new(webhook_repo, event_repo, webhook_service)
    }

    fn make_test_webhook(team_id: Uuid, url: &str) -> Webhook {
        Webhook::new(Uuid::new_v4(), team_id, url.to_string())
    }

    // ---- register_webhook ----

    #[tokio::test]
    async fn test_register_webhook_success_returns_webhook() {
        let webhook_repo = Arc::new(MockWebhookRepository::default());
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo, webhook_service);

        let team_id = Uuid::new_v4();
        let result = service
            .register_webhook(team_id, "https://example.com/hook".to_string())
            .await;

        assert!(result.is_ok(), "register should succeed");
        let webhook = result.unwrap();
        assert_eq!(webhook.team_id, team_id);
        assert_eq!(webhook.url, "https://example.com/hook");
        assert!(!webhook.id.is_nil());
    }

    #[tokio::test]
    async fn test_register_webhook_invalid_url_empty_returns_error() {
        let webhook_repo = Arc::new(MockWebhookRepository::default());
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo, webhook_service);

        let result = service
            .register_webhook(Uuid::new_v4(), String::new())
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid webhook URL"),
            "should report invalid URL, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_register_webhook_invalid_scheme_returns_error() {
        let webhook_repo = Arc::new(MockWebhookRepository::default());
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo, webhook_service);

        let result = service
            .register_webhook(Uuid::new_v4(), "ftp://example.com/hook".to_string())
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid webhook URL"));
    }

    #[tokio::test]
    async fn test_register_webhook_repo_failure_propagates() {
        let webhook_repo: Arc<dyn WebhookRepository> = Arc::new(FailingWebhookRepository);
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo, webhook_service);

        let result = service
            .register_webhook(Uuid::new_v4(), "https://example.com/hook".to_string())
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to create webhook"),
            "should report repo failure, got: {}",
            err
        );
    }

    // ---- trigger_webhook ----

    #[tokio::test]
    async fn test_trigger_webhook_success_sends_event() {
        let team_id = Uuid::new_v4();
        let webhook = make_test_webhook(team_id, "https://example.com/hook");
        let webhook_repo = Arc::new(MockWebhookRepository::with_webhooks(vec![webhook.clone()]));
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service =
            make_management_service(webhook_repo, event_repo.clone(), webhook_service.clone());

        let result = service
            .trigger_webhook(
                webhook.id,
                WebhookEventType::ScrapeCompleted,
                json!({"task_id": "abc"}),
            )
            .await;

        assert!(result.is_ok(), "trigger should succeed");
        assert_eq!(
            webhook_service.send_count.load(Ordering::SeqCst),
            1,
            "send_webhook should be called once"
        );
        assert_eq!(
            event_repo.events.lock().unwrap().len(),
            1,
            "event should be created in repo"
        );
    }

    #[tokio::test]
    async fn test_trigger_webhook_not_found_returns_error() {
        let webhook_repo = Arc::new(MockWebhookRepository::default());
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo, webhook_service);

        let result = service
            .trigger_webhook(Uuid::new_v4(), WebhookEventType::ScrapeCompleted, json!({}))
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Webhook not found"),
            "should report not found, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_trigger_webhook_send_failure_propagates() {
        let team_id = Uuid::new_v4();
        let webhook = make_test_webhook(team_id, "https://example.com/hook");
        let webhook_repo = Arc::new(MockWebhookRepository::with_webhooks(vec![webhook.clone()]));
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        let mut webhook_service = MockWebhookService::default();
        webhook_service.should_fail = true;
        let webhook_service = Arc::new(webhook_service);
        let service = make_management_service(webhook_repo, event_repo, webhook_service);

        let result = service
            .trigger_webhook(webhook.id, WebhookEventType::ScrapeFailed, json!({}))
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("mock send failed"),
            "should propagate send error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_trigger_webhook_event_create_failure_propagates() {
        let team_id = Uuid::new_v4();
        let webhook = make_test_webhook(team_id, "https://example.com/hook");
        let webhook_repo = Arc::new(MockWebhookRepository::with_webhooks(vec![webhook.clone()]));
        let event_repo: Arc<dyn WebhookEventRepository> = Arc::new(FailingWebhookEventRepository);
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo, webhook_service);

        let result = service
            .trigger_webhook(webhook.id, WebhookEventType::ScrapeCompleted, json!({}))
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to create webhook event"),
            "should report event create failure, got: {}",
            err
        );
    }

    // ---- retry_failed ----

    #[tokio::test]
    async fn test_retry_failed_no_pending_returns_zero() {
        let webhook_repo = Arc::new(MockWebhookRepository::default());
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo, webhook_service.clone());

        let count = service.retry_failed(10).await.expect("should succeed");

        assert_eq!(count, 0, "no pending events -> 0 successes");
        assert_eq!(
            webhook_service.send_count.load(Ordering::SeqCst),
            0,
            "send should not be called"
        );
    }

    #[tokio::test]
    async fn test_retry_failed_all_succeed() {
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        {
            let mut events = event_repo.events.lock().unwrap();
            events.push(create_test_event());
            events.push(create_test_event());
            events.push(create_test_event());
        }
        let webhook_repo = Arc::new(MockWebhookRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service =
            make_management_service(webhook_repo, event_repo.clone(), webhook_service.clone());

        let count = service.retry_failed(10).await.expect("should succeed");

        assert_eq!(count, 3, "all 3 events should succeed");
        assert_eq!(
            webhook_service.send_count.load(Ordering::SeqCst),
            3,
            "send should be called 3 times"
        );
        assert_eq!(
            event_repo.update_count.load(Ordering::SeqCst),
            3,
            "all 3 events should be updated"
        );
    }

    #[tokio::test]
    async fn test_retry_failed_some_fail_returns_correct_count() {
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        {
            let mut events = event_repo.events.lock().unwrap();
            events.push(create_test_event());
            events.push(create_test_event());
        }
        let webhook_repo = Arc::new(MockWebhookRepository::default());
        let mut webhook_service = MockWebhookService::default();
        webhook_service.should_fail = true;
        let webhook_service = Arc::new(webhook_service);
        let service =
            make_management_service(webhook_repo, event_repo.clone(), webhook_service.clone());

        let count = service.retry_failed(10).await.expect("should succeed");

        assert_eq!(count, 0, "no events should succeed");
        assert_eq!(
            webhook_service.send_count.load(Ordering::SeqCst),
            2,
            "send should be called for both events"
        );
        assert_eq!(
            event_repo.update_count.load(Ordering::SeqCst),
            2,
            "both events should be updated with failure"
        );
    }

    #[tokio::test]
    async fn test_retry_failed_skips_non_retryable_events() {
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        {
            let mut events = event_repo.events.lock().unwrap();
            let mut delivered = create_test_event();
            delivered.status = crate::domain::models::WebhookStatus::Delivered;
            events.push(delivered);
            events.push(create_test_event());
        }
        let webhook_repo = Arc::new(MockWebhookRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo, webhook_service.clone());

        let count = service.retry_failed(10).await.expect("should succeed");

        assert_eq!(count, 1, "only 1 retryable event should succeed");
        assert_eq!(
            webhook_service.send_count.load(Ordering::SeqCst),
            1,
            "send should be called once (skipped delivered)"
        );
    }

    #[tokio::test]
    async fn test_retry_failed_find_pending_error_propagates() {
        let webhook_repo = Arc::new(MockWebhookRepository::default());
        let event_repo: Arc<dyn WebhookEventRepository> =
            Arc::new(FindPendingFailingEventRepository);
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo, webhook_service);

        let result = service.retry_failed(10).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to find pending webhook events"),
            "should report find_pending failure, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_retry_failed_updates_event_status_on_success() {
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        let original_event = create_test_event();
        {
            event_repo
                .events
                .lock()
                .unwrap()
                .push(original_event.clone());
        }
        let webhook_repo = Arc::new(MockWebhookRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo.clone(), webhook_service);

        service.retry_failed(10).await.expect("should succeed");

        let events = event_repo.events.lock().unwrap();
        let updated = events
            .iter()
            .find(|e| e.id == original_event.id)
            .expect("event should exist");
        assert_eq!(
            updated.status,
            crate::domain::models::WebhookStatus::Delivered,
            "event should be marked Delivered after successful retry"
        );
        assert_eq!(updated.attempt_count, 1, "attempt_count should be 1");
    }

    // ---- list_webhooks ----

    #[tokio::test]
    async fn test_list_webhooks_empty_returns_empty_vec() {
        let webhook_repo = Arc::new(MockWebhookRepository::default());
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo, webhook_service);

        let result = service.list_webhooks(Uuid::new_v4()).await;

        assert!(result.is_ok());
        let webhooks = result.unwrap();
        assert!(webhooks.is_empty(), "should return empty vec");
    }

    #[tokio::test]
    async fn test_list_webhooks_returns_only_team_webhooks() {
        let team_a = Uuid::new_v4();
        let team_b = Uuid::new_v4();
        let wh_a1 = make_test_webhook(team_a, "https://a1.example.com");
        let wh_a2 = make_test_webhook(team_a, "https://a2.example.com");
        let wh_b1 = make_test_webhook(team_b, "https://b1.example.com");
        let webhook_repo = Arc::new(MockWebhookRepository::with_webhooks(vec![
            wh_a1, wh_a2, wh_b1,
        ]));
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo, webhook_service);

        let result = service.list_webhooks(team_a).await;

        assert!(result.is_ok());
        let webhooks = result.unwrap();
        assert_eq!(webhooks.len(), 2, "should return 2 webhooks for team A");
        assert!(
            webhooks.iter().all(|w| w.team_id == team_a),
            "all returned webhooks should belong to team A"
        );
    }

    #[tokio::test]
    async fn test_list_webhooks_repo_failure_propagates() {
        let webhook_repo: Arc<dyn WebhookRepository> = Arc::new(FailingWebhookRepository);
        let event_repo = Arc::new(ConfigurableWebhookEventRepository::default());
        let webhook_service = Arc::new(MockWebhookService::default());
        let service = make_management_service(webhook_repo, event_repo, webhook_service);

        let result = service.list_webhooks(Uuid::new_v4()).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to list webhooks"),
            "should report repo failure, got: {}",
            err
        );
    }
}
