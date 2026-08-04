// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook request and response DTOs

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 创建 Webhook 的请求 DTO
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateWebhookRequest {
    /// Webhook 回调 URL
    pub url: String,
}

/// Webhook 响应 DTO
#[derive(Debug, Clone, Serialize)]
pub struct WebhookResponse {
    /// Webhook ID
    pub id: Uuid,
    /// 团队 ID
    pub team_id: Uuid,
    /// Webhook 回调 URL
    pub url: String,
    /// 创建时间
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 是否已激活
    pub is_active: bool,
    /// 密钥（仅在创建时返回）
    pub secret: Option<String>,
}

/// Webhook 列表条目（不含密钥，防止列表接口泄露 secret）
#[derive(Debug, Clone, Serialize)]
pub struct WebhookListEntry {
    pub id: Uuid,
    pub team_id: Uuid,
    pub url: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub is_active: bool,
}

impl From<WebhookResponse> for WebhookListEntry {
    fn from(resp: WebhookResponse) -> Self {
        Self {
            id: resp.id,
            team_id: resp.team_id,
            url: resp.url,
            created_at: resp.created_at,
            is_active: resp.is_active,
        }
    }
}

/// Webhook 列表响应 DTO
#[derive(Debug, Clone, Serialize)]
pub struct WebhookListResponse {
    pub webhooks: Vec<WebhookListEntry>,
    pub total: usize,
}
