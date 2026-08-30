// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use super::task_repository::RepositoryError;
use crate::domain::models::WebhookEvent;
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
    /// 根据团队ID分页查询Webhook事件
    async fn find_by_team_id_paginated(
        &self,
        team_id: Uuid,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<WebhookEvent>, RepositoryError>;
    /// 统计团队Webhook事件数量
    async fn count_by_team_id(&self, team_id: Uuid) -> Result<u64, RepositoryError>;
    /// 更新Webhook事件
    async fn update(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError>;

    /// 按保留期分批删除终态事件（R-retention-004）
    ///
    /// 循环分批删除 `status='delivered'` 且 `delivered_at` 早于 DB `NOW() - retention_days`，
    /// 或 `status='dead'` 且 `updated_at` 早于该 cutoff 的行，直到删净或累计达
    /// `policy.max_rows_per_cycle`；每批独立短事务并带 `statement_timeout`。
    async fn cleanup_terminal(
        &self,
        retention_days: i64,
        policy: &crate::domain::retention_policy::RetentionBatchPolicy,
    ) -> Result<u64, RepositoryError>;
}
