// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Webhook实体
///
/// 表示一个Webhook端点配置，用于接收系统事件通知。
/// Webhook允许外部系统订阅和接收爬取任务的状态变化通知。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Webhook {
    /// Webhook唯一标识符
    pub id: Uuid,
    /// 所属团队ID，用于权限隔离和归属管理
    pub team_id: Uuid,
    /// Webhook回调URL，接收通知的目标地址
    pub url: String,
    /// 创建时间，Webhook配置创建的时间戳
    pub created_at: DateTime<Utc>,
}

impl Webhook {
    /// 创建一个新的Webhook配置
    ///
    /// # 参数
    ///
    /// * `team_id` - 所属团队ID
    /// * `url` - Webhook回调URL
    ///
    /// # 返回值
    ///
    /// 返回一个新的Webhook实例，包含生成的唯一ID和当前时间戳
    pub fn new(team_id: Uuid, url: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            team_id,
            url,
            created_at: Utc::now(),
        }
    }
}

/// Webhook事件实体
///
/// 表示一个待发送的Webhook通知事件，包含事件类型、
/// 负载数据、发送状态和重试机制等信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEvent {
    /// 事件唯一标识符
    pub id: Uuid,
    /// 所属团队ID，用于权限隔离和归属管理
    pub team_id: Uuid,
    /// Webhook ID，关联到目标Webhook配置
    pub webhook_id: Uuid,
    /// 事件类型，决定通知的内容和格式
    pub event_type: WebhookEventType,
    /// 事件负载数据，包含具体的通知内容
    pub payload: serde_json::Value,
    /// Webhook回调URL，事件发送的目标地址
    pub webhook_url: String,
    /// 事件状态，跟踪事件的发送进度
    pub status: WebhookStatus,
    /// 已重试次数，记录事件已经尝试发送的次数
    pub attempt_count: i32,
    /// 最大重试次数，事件发送失败时的最大重试限制
    pub max_retries: i32,
    /// 响应状态码，最后一次发送的HTTP响应状态
    pub response_status: Option<i32>,
    /// 响应体，最后一次发送的HTTP响应内容
    pub response_body: Option<String>,
    /// 错误信息，发送失败时的错误描述
    pub error_message: Option<String>,
    /// 下次重试时间，计划的下一次重试时间点
    pub next_retry_at: Option<DateTime<Utc>>,
    /// 创建时间，事件创建的时间戳
    pub created_at: DateTime<Utc>,
    /// 更新时间，事件信息最后更新的时间戳
    pub updated_at: DateTime<Utc>,
    /// 发送时间，事件成功发送的时间戳
    pub delivered_at: Option<DateTime<Utc>>,
}

/// Webhook事件类型枚举
///
/// 定义了系统中支持的不同类型的Webhook事件，每种类型
/// 对应不同的业务场景和通知内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEventType {
    /// 爬取任务完成，当爬取任务成功完成时触发
    CrawlCompleted,
    /// 爬取任务失败，当爬取任务执行失败时触发
    CrawlFailed,
    /// 抓取任务完成，当单个抓取任务成功完成时触发
    ScrapeCompleted,
    /// 抓取任务失败，当单个抓取任务执行失败时触发
    ScrapeFailed,
    /// 其他事件类型，用于扩展自定义事件
    Custom(String),
}

impl fmt::Display for WebhookEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WebhookEventType::CrawlCompleted => write!(f, "crawl.completed"),
            WebhookEventType::CrawlFailed => write!(f, "crawl.failed"),
            WebhookEventType::ScrapeCompleted => write!(f, "scrape.completed"),
            WebhookEventType::ScrapeFailed => write!(f, "scrape.failed"),
            WebhookEventType::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// Webhook状态枚举
///
/// 表示Webhook事件在其生命周期中的不同状态，用于跟踪
/// 事件的发送进度和结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WebhookStatus {
    /// 待处理，事件已创建但尚未发送
    #[default]
    Pending,
    /// 已发送，事件已成功发送到目标URL
    Delivered,
    /// 发送失败，事件发送失败但仍在重试中
    Failed,
    /// 死信，事件发送失败且已达到最大重试次数
    Dead,
}
