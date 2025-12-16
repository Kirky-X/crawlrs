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

use crate::domain::models::task::Task;
use async_trait::async_trait;
use sea_orm::DbErr;
use thiserror::Error;
use uuid::Uuid;

/// 仓库错误类型
#[derive(Error, Debug)]
pub enum RepositoryError {
    /// 数据库错误
    #[error("Database error: {0}")]
    Database(#[from] DbErr),
    /// 记录未找到
    #[error("Record not found")]
    NotFound,
}

/// 任务仓库特质
///
/// 定义任务数据访问接口
#[async_trait]
pub trait TaskRepository: Send + Sync {
    /// 创建新任务
    async fn create(&self, task: &Task) -> Result<Task, RepositoryError>;
    /// 根据ID查找任务
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError>;
    /// 更新任务
    async fn update(&self, task: &Task) -> Result<Task, RepositoryError>;
    /// 获取下一个待处理任务
    async fn acquire_next(&self, worker_id: Uuid) -> Result<Option<Task>, RepositoryError>;
    /// 标记任务已完成
    async fn mark_completed(&self, id: Uuid) -> Result<(), RepositoryError>;
    /// 标记任务已失败
    async fn mark_failed(&self, id: Uuid) -> Result<(), RepositoryError>;
    /// 标记任务已取消
    async fn mark_cancelled(&self, id: Uuid) -> Result<(), RepositoryError>;
    /// 检查URL是否存在
    async fn exists_by_url(&self, url: &str) -> Result<bool, RepositoryError>;
    /// 重置卡住的任务（长时间处于Active状态）
    async fn reset_stuck_tasks(&self, timeout: chrono::Duration) -> Result<u64, RepositoryError>;
    /// 取消与特定 Crawl ID 相关的所有任务
    async fn cancel_tasks_by_crawl_id(&self, crawl_id: Uuid) -> Result<u64, RepositoryError>;
    /// 标记过期任务为失败
    async fn expire_tasks(&self) -> Result<u64, RepositoryError>;
    /// 根据 Crawl ID 查找所有任务
    async fn find_by_crawl_id(&self, crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError>;
}
