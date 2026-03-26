// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::{Task, TaskStatus, TaskType};
use async_trait::async_trait;
use sea_orm::DbErr;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use shaku::Interface;
use std::collections::HashSet;
use thiserror::Error;

/// 仓库错误类型
#[derive(Error, Debug)]
pub enum RepositoryError {
    /// 数据库错误
    #[error("Database error: {0}")]
    Database(anyhow::Error),
    /// 记录未找到
    #[error("Record not found")]
    NotFound,
}

/// 实现 From<sea_orm::DbErr> trait，支持 ? 操作符自动转换
impl From<DbErr> for RepositoryError {
    fn from(err: DbErr) -> Self {
        RepositoryError::Database(anyhow::anyhow!(err))
    }
}

/// 任务查询参数
#[derive(Debug, Default, Clone)]
pub struct TaskQueryParams {
    pub team_id: Uuid,
    pub task_ids: Option<Vec<Uuid>>,
    pub task_types: Option<Vec<TaskType>>,
    pub statuses: Option<Vec<TaskStatus>>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
    pub crawl_id: Option<Uuid>,
    pub limit: u32,
    pub offset: u32,
    /// 游标分页：基于创建时间
    pub cursor: Option<DateTime<Utc>>,
    /// 游标分页：基于任务ID（用于处理相同创建时间的记录）
    pub cursor_id: Option<Uuid>,
}

/// 任务仓库特质
///
/// 定义任务数据访问接口
#[async_trait]
pub trait TaskRepository: Interface + Send + Sync {
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
    /// 批量检查URL是否存在（优化 N+1 查询）
    async fn find_existing_urls(&self, urls: &[String])
        -> Result<HashSet<String>, RepositoryError>;
    /// 重置卡住的任务（长时间处于Active状态）
    async fn reset_stuck_tasks(&self, timeout: chrono::Duration) -> Result<u64, RepositoryError>;
    /// 取消与特定 Crawl ID 相关的所有任务
    async fn cancel_tasks_by_crawl_id(&self, crawl_id: Uuid) -> Result<u64, RepositoryError>;
    /// 标记过期任务为失败
    async fn expire_tasks(&self) -> Result<u64, RepositoryError>;
    /// 根据 Crawl ID 查找所有任务
    async fn find_by_crawl_id(&self, crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError>;
    /// 高级任务查询
    async fn query_tasks(
        &self,
        params: TaskQueryParams,
    ) -> Result<(Vec<Task>, u64), RepositoryError>;
    /// 批量取消任务
    async fn batch_cancel(
        &self,
        task_ids: Vec<Uuid>,
        team_id: Uuid,
        force: bool,
    ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError>;
}
