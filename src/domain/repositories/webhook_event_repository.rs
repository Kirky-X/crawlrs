// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::task_repository::RepositoryError;
use crate::domain::models::webhook::WebhookEvent;
use async_trait::async_trait;
use uuid::Uuid;

/// Webhook仓库特质
///
/// 定义Webhook事件数据访问接口
#[async_trait]
pub trait WebhookEventRepository: Send + Sync {
    /// 创建Webhook事件
    async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError>;
    /// 根据ID查找Webhook事件
    async fn find_by_id(&self, id: Uuid) -> Result<Option<WebhookEvent>, RepositoryError>;
    /// 查找待处理的Webhook事件
    async fn find_pending(&self, limit: u64) -> Result<Vec<WebhookEvent>, RepositoryError>;
    /// 更新Webhook事件
    async fn update(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError>;
}
