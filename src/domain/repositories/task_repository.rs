// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::{Task, TaskStatus, TaskType};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use thiserror::Error;
use uuid::Uuid;

/// 仓库错误类型（纯领域定义，不依赖具体存储技术）
#[derive(Error, Debug)]
pub enum RepositoryError {
    /// 底层存储或连接错误
    #[error("Database error: {0}")]
    Database(anyhow::Error),
    /// 指定记录不存在
    #[error("Record not found")]
    NotFound,
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
    ///
    /// `lock_token` 为锁守卫：仅当任务当前锁与调用者身份一致（或任务无锁）时
    /// 才允许终结，防止锁过期被其他 worker 抢占后陈旧 worker 双写。
    /// 返回受影响行数；0 表示任务已被他人终结/认领。
    async fn mark_completed(
        &self,
        id: Uuid,
        lock_token: Option<Uuid>,
    ) -> Result<u64, RepositoryError>;
    /// 标记任务已失败
    ///
    /// 锁守卫语义同 [`TaskRepository::mark_completed`]。
    async fn mark_failed(&self, id: Uuid, lock_token: Option<Uuid>)
        -> Result<u64, RepositoryError>;
    /// 标记任务已取消（管理路径，无锁身份）
    async fn mark_cancelled(&self, id: Uuid) -> Result<u64, RepositoryError>;
    /// 条件回队：将任务置回 queued 并清空认领信息
    ///
    /// 锁守卫语义同 [`TaskRepository::mark_completed`]：仅当任务仍被 `lock_token`
    /// 认领（或无锁）且状态仍为 queued/active 时才回队，防止覆盖并发发生的
    /// 取消/终结等状态迁移。
    /// `scheduled_at` 为重排时间（如并发超限延迟 30s）；None 表示立即就绪。
    /// 返回 `Ok(true)` 表示回队成功；`Ok(false)` 表示任务已被他方迁移。
    ///
    /// 默认实现保守返回 `Ok(false)`（视为任务已被他方迁移，调用方放弃回队），
    /// 由具备条件更新能力的仓储实现覆盖。
    async fn requeue_task(
        &self,
        _id: Uuid,
        _lock_token: Option<Uuid>,
        _scheduled_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<bool, RepositoryError> {
        log::warn!(
            "requeue_task called on a repository without conditional-update support \
             (task {} left untouched)",
            _id
        );
        Ok(false)
    }

    /// 守卫式生命周期更新：按内存快照写入状态/调度/计数/payload 等字段，
    /// 但仅当 DB 行仍处于活跃态（queued/active）且锁仍归属调用者快照中的
    /// `lock_token` 时才生效。
    ///
    /// 与 [`TaskRepository::update`]（无守卫全行覆盖）不同，本方法保证不会
    /// 覆盖并发发生的取消/终结等状态迁移。
    /// 返回受影响行数；0 表示守卫未通过（任务已被他方迁移），调用方应放弃写入。
    ///
    /// 默认实现保守返回 `Ok(0)`，由具备条件更新能力的仓储实现覆盖。
    async fn update_task_guarded(
        &self,
        _task: &crate::domain::models::task_model::Task,
    ) -> Result<u64, RepositoryError> {
        log::warn!(
            "update_task_guarded called on a repository without conditional-update support \
             (task {} left untouched)",
            _task.id
        );
        Ok(0)
    }
    /// 续期任务锁（锁续期心跳）
    ///
    /// 仅当任务处于 active 且锁仍归属 `worker_id` 时续期成功。
    /// 返回 `Ok(true)` 表示续期成功；`Ok(false)` 表示锁已丢失（任务被抢占或已终结），
    /// 调用方应停止续期并以锁守卫拒绝后续终结请求。
    async fn renew_lock(
        &self,
        task_id: Uuid,
        worker_id: Uuid,
        extend_seconds: i64,
    ) -> Result<bool, RepositoryError>;
    /// 检查URL是否存在
    async fn exists_by_url(&self, url: &str) -> Result<bool, RepositoryError>;
    /// 批量检查URL是否存在（优化 N+1 查询）
    async fn find_existing_urls(&self, urls: &[String])
        -> Result<HashSet<String>, RepositoryError>;
    /// 重置卡住的任务（长时间处于Active状态）
    async fn reset_stuck_tasks(&self, timeout: chrono::Duration) -> Result<u64, RepositoryError>;
    /// 按本副本 worker 身份回滚在途任务
    ///
    /// 仅回滚 `lock_token` 属于 `worker_ids` 且活跃的任务，防止多副本部署下
    /// 一方停机把另一方正在执行的任务误回滚 queued。
    /// 默认实现委托 [`TaskRepository::reset_stuck_tasks`]（无身份过滤），
    /// 具备身份感知能力的仓储实现应覆盖本方法。
    async fn reset_stuck_tasks_for_workers(
        &self,
        timeout: chrono::Duration,
        worker_ids: &[Uuid],
    ) -> Result<u64, RepositoryError> {
        let _ = worker_ids;
        self.reset_stuck_tasks(timeout).await
    }
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
