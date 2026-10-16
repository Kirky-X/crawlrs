// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use crate::domain::models::ScrapeResult;
use anyhow::Result;
use async_trait::async_trait;

/// 爬取结果仓库特质
///
/// 定义爬取结果数据访问接口
#[async_trait]
pub trait ScrapeResultRepository: Send + Sync {
    /// 保存爬取结果
    async fn save(&self, result: ScrapeResult) -> Result<()>;
    /// 根据任务ID查找结果
    async fn find_by_task_id(&self, task_id: crate::domain::id::Id)
        -> Result<Option<ScrapeResult>>;
    /// 根据任务ID列表批量查找结果
    async fn find_by_task_ids(
        &self,
        task_ids: &[crate::domain::id::Id],
    ) -> Result<Vec<ScrapeResult>>;
    /// 任务是否已有结果行（存在性投影，默认委托 find_by_task_id；
    /// 实现方可覆写为 COUNT 投影避免整行回读）
    async fn has_result_for_task(&self, task_id: crate::domain::id::Id) -> Result<bool> {
        Ok(self.find_by_task_id(task_id).await?.is_some())
    }
    /// 获取团队的平均响应时间
    ///
    /// 计算指定团队在过去30天内的平均响应时间
    async fn get_team_avg_response_time(&self, team_id: crate::domain::id::Id) -> Result<f64>;
    /// 清理超过保留期的爬取结果
    ///
    /// 删除 `created_at` 早于 `now - retention_days` 的结果行，返回删除的行数。
    /// `retention_days <= 0` 表示禁用清理，实现必须直接返回 `Ok(0)`，不得删除任何数据。
    async fn cleanup_expired(&self, retention_days: i64) -> Result<u64>;
    /// 分页列出结果表 distinct URL（断点续爬 seen 预热补充，F3）
    ///
    /// keyset 分页语义与 [`crate::domain::repositories::task_repository::TaskRepository::list_distinct_urls`]
    /// 对齐：`after_url` 传入时仅返回严格大于它的 URL（`url > ?` ORDER BY url LIMIT ?）。
    /// 覆盖 tasks.url 之外的重定向 final_url（仅存于结果表）。
    ///
    /// 默认实现返回错误（失败显性化：未实现该能力的仓库在预热路径报错，
    /// 而非静默返回空集被当成"无历史 URL"）。
    async fn list_distinct_urls(
        &self,
        _limit: u32,
        _after_url: Option<String>,
    ) -> Result<Vec<String>> {
        Err(anyhow::anyhow!(
            "list_distinct_urls not implemented by this repository (dedup warmup unavailable)"
        ))
    }
}
