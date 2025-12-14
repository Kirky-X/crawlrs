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

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Webhook实体
///
/// 表示一个Webhook端点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Webhook {
    /// Webhook唯一标识符
    pub id: Uuid,
    /// 所属团队ID
    pub team_id: Uuid,
    /// Webhook回调URL
    pub url: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

impl Webhook {
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
/// 表示一个待发送的Webhook通知事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEvent {
    /// 事件唯一标识符
    pub id: Uuid,
    /// 所属团队ID
    pub team_id: Uuid,
    /// Webhook ID
    pub webhook_id: Uuid,
    /// 事件类型
    pub event_type: WebhookEventType,
    /// 事件负载数据
    pub payload: serde_json::Value,
    /// Webhook回调URL
    pub webhook_url: String,
    /// 事件状态
    pub status: WebhookStatus,
    /// 已重试次数
    pub attempt_count: i32,
    /// 最大重试次数
    pub max_retries: i32,
    /// 响应状态码
    pub response_status: Option<i32>,
    /// 响应体
    pub response_body: Option<String>,
    /// 错误信息
    pub error_message: Option<String>,
    /// 下次重试时间
    pub next_retry_at: Option<DateTime<Utc>>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: DateTime<Utc>,
    /// 发送时间
    pub delivered_at: Option<DateTime<Utc>>,
}

/// Webhook事件类型枚举
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEventType {
    /// 爬取任务完成
    CrawlCompleted,
    /// 爬取任务失败
    CrawlFailed,
    /// 抓取任务完成
    ScrapeCompleted,
    /// 抓取任务失败
    ScrapeFailed,
    /// 其他事件类型
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WebhookStatus {
    /// 待处理
    #[default]
    Pending,
    /// 已发送
    Delivered,
    /// 发送失败
    Failed,
    /// 死信
    Dead,
}
