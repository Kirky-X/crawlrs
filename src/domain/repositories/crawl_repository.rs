// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::task_repository::RepositoryError;
use crate::domain::models::crawl::Crawl;
use async_trait::async_trait;
use uuid::Uuid;

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
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Crawl>, RepositoryError>;

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
    /// * `Ok(())` - 成功增加计数
    /// * `Err(RepositoryError)` - 操作失败时返回错误
    async fn increment_completed_tasks(&self, id: Uuid) -> Result<(), RepositoryError>;

    /// 增加失败任务计数
    ///
    /// # 参数
    ///
    /// * `id` - 爬取任务的唯一标识符
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功增加计数
    /// * `Err(RepositoryError)` - 操作失败时返回错误
    async fn increment_failed_tasks(&self, id: Uuid) -> Result<(), RepositoryError>;

    /// 更新爬取任务状态
    ///
    /// # 参数
    ///
    /// * `id` - 爬取任务的唯一标识符
    /// * `status` - 新的任务状态
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功更新状态
    /// * `Err(RepositoryError)` - 更新失败时返回错误
    async fn update_status(
        &self,
        id: Uuid,
        status: crate::domain::models::crawl::CrawlStatus,
    ) -> Result<(), RepositoryError>;

    /// 增加总任务计数
    ///
    /// # 参数
    ///
    /// * `id` - 爬取任务的唯一标识符
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功增加计数
    /// * `Err(RepositoryError)` - 操作失败时返回错误
    async fn increment_total_tasks(&self, id: Uuid) -> Result<(), RepositoryError>;
}
