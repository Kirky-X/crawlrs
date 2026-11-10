// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use super::task_repository::RepositoryError;
use crate::domain::models::{Crawl, CrawlStatus};
use async_trait::async_trait;

/// 爬取任务仓库特质
///
/// 定义爬取任务数据访问接口，提供对爬取任务的CRUD操作和状态管理功能。
/// 该特质遵循依赖倒置原则，确保领域层不依赖于具体的数据存储实现。
#[async_trait]
pub trait CrawlRepository: Send + Sync {
    /// 创建爬取任务
    ///
    /// # 参数
    ///
    /// * `crawl` - 要创建的爬取任务实体
    ///
    /// # 返回值
    ///
    /// * `Ok(Crawl)` - 成功创建后返回爬取任务（可能包含生成的ID）
    /// * `Err(RepositoryError)` - 创建失败时返回错误
    async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError>;

    /// 根据ID查找爬取任务
    ///
    /// # 参数
    ///
    /// * `id` - 爬取任务的唯一标识符
    ///
    /// # 返回值
    ///
    /// * `Ok(Some(Crawl))` - 找到任务时返回任务实体
    /// * `Ok(None)` - 未找到任务时返回空
    /// * `Err(RepositoryError)` - 查询失败时返回错误
    async fn find_by_id(&self, id: crate::domain::id::Id)
        -> Result<Option<Crawl>, RepositoryError>;

    /// 更新爬取任务
    ///
    /// # 参数
    ///
    /// * `crawl` - 包含更新数据的爬取任务实体
    ///
    /// # 返回值
    ///
    /// * `Ok(Crawl)` - 成功更新后返回更新后的任务
    /// * `Err(RepositoryError)` - 更新失败时返回错误
    async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError>;

    /// 增加已完成任务计数
    ///
    /// # 参数
    ///
    /// * `id` - 爬取任务的唯一标识符
    ///
    /// # 返回值
    ///
    /// * `Ok()` - 成功增加计数
    /// * `Err(RepositoryError)` - 操作失败时返回错误
    async fn increment_completed_tasks(
        &self,
        id: crate::domain::id::Id,
    ) -> Result<(), RepositoryError>;

    /// 增加失败任务计数
    ///
    /// # 参数
    ///
    /// * `id` - 爬取任务的唯一标识符
    ///
    /// # 返回值
    ///
    /// * `Ok()` - 成功增加计数
    /// * `Err(RepositoryError)` - 操作失败时返回错误
    async fn increment_failed_tasks(
        &self,
        id: crate::domain::id::Id,
    ) -> Result<(), RepositoryError>;

    /// 更新爬取任务状态
    ///
    /// # 参数
    ///
    /// * `id` - 爬取任务的唯一标识符
    /// * `status` - 新的任务状态
    ///
    /// # 返回值
    ///
    /// * `Ok()` - 成功更新状态
    /// * `Err(RepositoryError)` - 更新失败时返回错误
    async fn update_status(
        &self,
        id: crate::domain::id::Id,
        status: CrawlStatus,
    ) -> Result<(), RepositoryError>;

    /// 条件状态写入：仅当 crawl 尚未处于终态时更新；终态 status 同时写
    /// `completed_at`。返回 `Ok(true)` 表示写入成功，`Ok(false)` 表示 crawl
    /// 已是终态（并发取消/完成）或不存在，调用方不得按“已写入”处理。
    ///
    /// 完成判定与自适应停止必须使用本方法，避免迟到事件覆盖 Cancelled。
    ///
    /// 默认实现返回 `Err`（而非 `Ok(false)`）：`Ok(false)` 的语义是“已是终态/
    /// 不存在”，与“仓储未实现”不可混为一谈，否则升级后的仓储会静默不落状态。
    /// 生产仓储必须覆盖。
    async fn update_status_guarded(
        &self,
        id: crate::domain::id::Id,
        _status: CrawlStatus,
    ) -> Result<bool, RepositoryError> {
        Err(RepositoryError::Database(anyhow::anyhow!(
            "update_status_guarded is not implemented by this crawl repository (crawl {id})"
        )))
    }

    /// 增加总任务计数
    ///
    /// # 参数
    ///
    /// * `id` - 爬取任务的唯一标识符
    ///
    /// # 返回值
    ///
    /// * `Ok()` - 成功增加计数
    /// * `Err(RepositoryError)` - 操作失败时返回错误
    async fn increment_total_tasks(&self, id: crate::domain::id::Id)
        -> Result<(), RepositoryError>;

    /// 根据团队ID分页查询爬取任务
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队的唯一标识符
    /// * `limit` - 返回记录的最大数量
    /// * `offset` - 跳过的记录数量（用于分页）
    ///
    /// # 返回值
    ///
    /// * `Ok(Vec<Crawl>)` - 返回爬取任务列表
    /// * `Err(RepositoryError)` - 查询失败时返回错误
    async fn find_by_team_id_paginated(
        &self,
        team_id: crate::domain::id::Id,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Crawl>, RepositoryError>;

    /// 统计团队爬取任务数量
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队的唯一标识符
    ///
    /// # 返回值
    ///
    /// * `Ok(u64)` - 返回任务总数
    /// * `Err(RepositoryError)` - 统计失败时返回错误
    async fn count_by_team_id(
        &self,
        team_id: crate::domain::id::Id,
    ) -> Result<u64, RepositoryError>;
}
