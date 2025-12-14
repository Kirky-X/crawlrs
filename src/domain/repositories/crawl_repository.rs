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

use super::task_repository::RepositoryError;
use crate::domain::models::crawl::Crawl;
use async_trait::async_trait;
use uuid::Uuid;

/// 爬取任务仓库特质
///
/// 定义爬取任务数据访问接口
#[async_trait]
pub trait CrawlRepository: Send + Sync {
    /// 创建爬取任务
    async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError>;
    /// 根据ID查找爬取任务
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Crawl>, RepositoryError>;
    /// 更新爬取任务
    async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError>;
    /// 增加已完成任务计数
    async fn increment_completed_tasks(&self, id: Uuid) -> Result<(), RepositoryError>;
    /// 增加失败任务计数
    async fn increment_failed_tasks(&self, id: Uuid) -> Result<(), RepositoryError>;
    /// 更新爬取任务状态
    async fn update_status(
        &self,
        id: Uuid,
        status: crate::domain::models::crawl::CrawlStatus,
    ) -> Result<(), RepositoryError>;
    /// 增加总任务计数
    async fn increment_total_tasks(&self, id: Uuid) -> Result<(), RepositoryError>;
}
