// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::task_repository::RepositoryError;
use crate::domain::models::webhook::Webhook;
use async_trait::async_trait;
use uuid::Uuid;

/// Webhook仓库特质
///
/// 定义Webhook数据访问接口
#[async_trait]
pub trait WebhookRepository: Send + Sync {
    /// 创建Webhook
    async fn create(&self, webhook: &Webhook) -> Result<Webhook, RepositoryError>;
    /// 根据ID查找Webhook
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Webhook>, RepositoryError>;
}
