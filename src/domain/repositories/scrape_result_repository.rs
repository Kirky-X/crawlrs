// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::ScrapeResult;
use anyhow::Result;
use async_trait::async_trait;

use uuid::Uuid;

/// 爬取结果仓库特质
///
/// 定义爬取结果数据访问接口
#[async_trait]
pub trait ScrapeResultRepository: Send + Sync {
    /// 保存爬取结果
    async fn save(&self, result: ScrapeResult) -> Result<()>;
    /// 根据任务ID查找结果
    async fn find_by_task_id(&self, task_id: Uuid) -> Result<Option<ScrapeResult>>;
    /// 根据任务ID列表批量查找结果
    async fn find_by_task_ids(&self, task_ids: &[Uuid]) -> Result<Vec<ScrapeResult>>;
    /// 获取团队的平均响应时间
    ///
    /// 计算指定团队在过去30天内的平均响应时间
    async fn get_team_avg_response_time(&self, team_id: Uuid) -> Result<f64>;
    /// 按保留期删除过期结果（R-retention-002）
    ///
    /// 删除 `created_at` 早于 `NOW() - retention_days` 的行，返回删除行数。
    async fn cleanup_expired(&self, retention_days: i64) -> Result<u64>;
}
