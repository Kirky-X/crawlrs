// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use super::task_repository::RepositoryError;
use crate::domain::models::Webhook;
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
    /// 根据团队ID查找所有Webhook
    async fn find_by_team_id(&self, team_id: Uuid) -> Result<Vec<Webhook>, RepositoryError>;
}
