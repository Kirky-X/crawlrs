// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook Service
//!
//! Unified webhook service for task completion and failure notifications.
//! Supports dependency injection via trait-kit.

// Webhook 子模块（WebhookManagementServiceImpl 已拆分到 webhook/management.rs）
#[path = "webhook/mod.rs"]
mod webhook;
#[cfg(feature = "webhook")]
pub use webhook::WebhookManagementServiceImpl;

#[cfg(feature = "webhook")]
use super::webhook_event_builder::WebhookEventBuilder;
use crate::domain::models::{Task, Webhook};
use crate::domain::models::{WebhookEvent, WebhookEventType};
// WebhookStatus 仅 WebhookServiceImpl（webhook-on）使用，webhook-off 时需门控避免 unused import
#[cfg(feature = "webhook")]
use crate::domain::models::WebhookStatus;
// 以下 import 仅 WebhookServiceImpl 使用，
// webhook-off 时 Impl 不编译，import 需同步门控避免 unused imports warning。
#[cfg(feature = "webhook")]
use crate::application::dto::scrape_request::ScrapeRequestDto;
#[cfg(feature = "webhook")]
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
#[cfg(feature = "webhook")]
use crate::domain::services::webhook_sender::WebhookSender;
// 架构（M1 折中说明）：本 import 引入 domain → infrastructure 的依赖箭头。
// `constant_time_eq_str` 是无状态纯函数（无 I/O、无 DB、无全局状态），位于
// `infrastructure::security::constant_time_compare` 是历史组织结果（与 auth_middleware 共用）。
// 严格 DDD 下应将此 helper 迁移到 `domain::shared` 或 `crate::common` 等中性位置，
// 但当前选择 **DRY 优先于分层纯净** — 复用公共 helper（含完整文档 + 7 个单元测试）
// 优于在 domain 层复制一份。此为已知折中，未来可在 `domain::shared` 重构时统一迁移。
use crate::infrastructure::security::constant_time_eq_str;
use anyhow::Result;
// anyhow::anyhow! 宏仅 Impl 中使用，webhook-off 时不导入
#[cfg(feature = "webhook")]
use anyhow::anyhow;
use async_trait::async_trait;
use chrono::Utc;
// T-webhook-001：使用 standardwebhooks 标准化 webhook 签名/验签
use standardwebhooks::Webhook as StandardWebhook;
// log::{error, info} 仅 Impl 中使用，webhook-off 时不导入
#[cfg(feature = "webhook")]
use log::{error, info};
// serde_json::json 已迁移到 webhook_event_builder
// use serde_json::json;
// Arc 仅 Impl 中使用，webhook-off 时不导入
#[cfg(feature = "webhook")]
use std::sync::Arc;
use uuid::Uuid;

/// Webhook服务接口（支持 DI）
#[async_trait]
pub trait WebhookService: Send + Sync {
    /// 发送Webhook事件
    ///
    /// 返回投递的实际 HTTP 状态码，供投递记录（response_status）使用
    async fn send_webhook(&self, event: &WebhookEvent) -> Result<u16>;

    /// 触发任务完成 webhook
    async fn trigger_completion(&self, task: &Task) -> Result<()>;

    /// 触发任务失败 webhook
    async fn trigger_failure(&self, task: &Task, error_msg: String) -> Result<()>;
}

/// Webhook服务实现
///
/// webhook feature 关闭时不编译此类型。
/// webhook-off 模式下，`init_services` 装配 `NoopWebhookService` 替代。
#[cfg(feature = "webhook")]
pub struct WebhookServiceImpl {
    /// Webhook 发送器
    webhook_sender: Arc<dyn WebhookSender>,
    /// Webhook 签名密钥
    secret: String,
    /// Webhook 事件仓库
    repository: Arc<dyn WebhookEventRepository>,
}

/// webhook feature 关闭时不编译此 impl
#[cfg(feature = "webhook")]
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

    /// 为负载生成签名（standardwebhooks 标准格式）
    ///
    /// 签名格式：`v1,<base64>`，签名消息：`{msg_id}.{timestamp}.{payload}`。
    /// `msg_id` 必须与 `webhook-id` header 一致——接收方按标准用该值重算 HMAC。
    /// 签名失败返回 `None`，调用方必须中止投递（发送空签名必然验签失败）。
    fn generate_signature(&self, msg_id: &str, payload: &str, timestamp: i64) -> Option<String> {
        let wh = match StandardWebhook::from_bytes(self.secret.as_bytes().to_vec()) {
            Ok(w) => w,
            Err(e) => {
                log::error!("Failed to create standardwebhook: {:?}", e);
                return None;
            }
        };
        wh.sign(msg_id, timestamp, payload.as_bytes())
            .map_err(|e| {
                log::error!("Failed to sign webhook payload: {:?}", e);
                e
            })
            .ok()
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

/// webhook feature 关闭时不编译此 impl
#[cfg(feature = "webhook")]
#[async_trait]
impl WebhookService for WebhookServiceImpl {
    async fn send_webhook(&self, event: &WebhookEvent) -> Result<u16> {
        let timestamp = chrono::Utc::now().timestamp();
        // Serialize only for HMAC signature computation; pass event.payload directly to sender
        let payload_str = serde_json::to_string(&event.payload)?;
        // msg_id 与 webhook-id header 保持一致（事件 UUID），否则标准验签必失败
        let signature = self
            .generate_signature(&event.id.to_string(), &payload_str, timestamp)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Failed to generate webhook signature for event {}",
                    event.id
                )
            })?;

        let mut headers = std::collections::HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert(
            standardwebhooks::HEADER_WEBHOOK_SIGNATURE.to_string(),
            signature,
        );
        headers.insert(
            standardwebhooks::HEADER_WEBHOOK_TIMESTAMP.to_string(),
            timestamp.to_string(),
        );
        headers.insert(
            standardwebhooks::HEADER_WEBHOOK_ID.to_string(),
            event.id.to_string(),
        );

        // pass payload directly instead of serialize-then-deserialize round-trip;
        // 使用 send_with_status 记录真实响应状态码（替代硬编码 200）
        let status = self
            .webhook_sender
            .send_with_status(&event.webhook_url, &event.payload, Some(&headers))
            .await?;

        info!(
            "Webhook sent successfully for event {} (status {})",
            event.id, status
        );
        Ok(status)
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

/// webhook feature 关闭时不编译此 impl
#[cfg(feature = "webhook")]
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

        let payload = WebhookEventBuilder::build_task_payload(task, error_msg.as_deref());

        let mut event =
            WebhookEventBuilder::build_task_event(task, event_type, payload, webhook_url);

        // 事件由本次内联发送负责投递：入库即置 processing，
        // 避免内联发送期间 webhook-worker 再次认领造成重复投递。
        event.status = WebhookStatus::Processing;

        // Save event to repository
        if let Err(e) = self.repository.create(&event).await {
            error!("Failed to create webhook event for task {}: {}", task.id, e);
            return Err(anyhow!("Failed to create webhook event: {}", e));
        }

        // Send webhook
        match self.send_webhook(&event).await {
            Ok(status) => {
                // 内联发送成功：终结事件，worker 不再重发
                event.status = WebhookStatus::Delivered;
                event.delivered_at = Some(chrono::Utc::now());
                event.response_status = Some(status as i32);
                if let Err(e) = self.repository.update(&event).await {
                    error!("Failed to mark webhook event {} delivered: {}", event.id, e);
                }
                Ok(())
            }
            Err(e) => {
                // 内联发送失败：交还 worker 按重试策略投递（5 秒后首次重试）
                error!("Failed to send webhook for task {}: {}", task.id, e);
                event.status = WebhookStatus::Failed;
                event.next_retry_at = Some(chrono::Utc::now() + chrono::Duration::seconds(5));
                if let Err(update_err) = self.repository.update(&event).await {
                    error!(
                        "Failed to schedule retry for webhook event {}: {}",
                        event.id, update_err
                    );
                }
                Err(e)
            }
        }
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
    /// * `url` - webhook 端点 URL（必须是 http:// 或 https）
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

/// 统一的 webhook 认证失败错误消息
///
/// 架构此常量从 `presentation::handlers::webhook_handler` 迁移至 domain 层。
/// 不区分具体失败阶段（缺失 header / 格式错误 / 签名不匹配 / 时间戳过期），
/// 避免向攻击者泄露验证步骤信息。所有失败路径均返回此消息。
///
/// 接收方 webhook handler 应将本常量映射到 HTTP 401 响应体或 `CrawlRsError::Authentication`。
pub const WEBHOOK_AUTH_FAILED: &str = "webhook authentication failed";

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

/// 为负载生成签名（standardwebhooks 标准格式）
///
/// 签名格式：`v1,<base64>`，签名消息：`{msg_id}.{timestamp}.{payload}`
fn generate_signature(secret: &str, msg_id: &str, payload: &[u8], timestamp: i64) -> String {
    let wh = match StandardWebhook::from_bytes(secret.as_bytes().to_vec()) {
        Ok(w) => w,
        Err(e) => {
            log::error!("Failed to create standardwebhook: {:?}", e);
            return String::new();
        }
    };
    wh.sign(msg_id, timestamp, payload).unwrap_or_default()
}

/// 验证 webhook 签名（接受 `&str` payload，兼容旧调用方）
///
/// 供接收方使用以验证 webhook authenticity 和 freshness。
pub fn verify_webhook_signature(
    secret: &str,
    msg_id: &str,
    payload: &str,
    timestamp: i64,
    signature: &str,
) -> bool {
    verify_webhook_signature_bytes(secret, msg_id, payload.as_bytes(), timestamp, signature)
}

/// 验证 webhook 签名（接受 `&[u8]` payload，standardwebhooks 标准化实现）
///
/// 使用 standardwebhooks 重新计算签名并与提供的签名进行常量时间比较。
/// 时间戳验证由 standardwebhooks 内部处理（5 分钟窗口）。
pub fn verify_webhook_signature_bytes(
    secret: &str,
    msg_id: &str,
    payload: &[u8],
    timestamp: i64,
    signature: &str,
) -> bool {
    // 首先验证时间戳是否在有效期内
    if !validate_timestamp(timestamp) {
        log::warn!("Webhook timestamp is outside valid window");
        return false;
    }

    // 使用 standardwebhooks 重新计算签名并比较
    let expected_signature = generate_signature(secret, msg_id, payload, timestamp);
    constant_time_eq_str(signature, &expected_signature)
}

/// 从字符串形式的 signature + timestamp + msg_id 验证 webhook 签名
///
/// 架构本函数承担 **domain 层**的 webhook 认证逻辑，
/// 包括 timestamp 字符串解析 + 签名验证。
///
/// # 参数
///
/// * `secret` - webhook 共享密钥
/// * `signature` - 来自请求头的 standardwebhooks 签名（`v1,<base64>`）
/// * `timestamp_str` - 来自请求头的时间戳字符串（Unix 秒）
/// * `msg_id` - webhook-id 头（消息唯一标识）
/// * `body` - 请求 body 原始字节
///
/// # 返回值
///
/// * `Ok(())` - 签名验证通过
/// * `Err(WEBHOOK_AUTH_FAILED)` - 时间戳格式错误或签名不匹配
///
/// # 安全说明
///
/// 失败原因统一为 [`WEBHOOK_AUTH_FAILED`]，不区分缺失/格式错误/签名不匹配/时间戳过期，
/// 避免向攻击者泄露验证步骤信息。
pub fn verify_webhook_signature_from_parts(
    secret: &str,
    signature: &str,
    timestamp_str: &str,
    msg_id: &str,
    body: &[u8],
) -> Result<(), &'static str> {
    let timestamp: i64 = timestamp_str.parse().map_err(|_| WEBHOOK_AUTH_FAILED)?;
    if !verify_webhook_signature_bytes(secret, msg_id, body, timestamp, signature) {
        return Err(WEBHOOK_AUTH_FAILED);
    }
    Ok(())
}

#[cfg(all(test, feature = "webhook"))]
#[path = "tests/webhook_service_test.rs"]
mod tests;
