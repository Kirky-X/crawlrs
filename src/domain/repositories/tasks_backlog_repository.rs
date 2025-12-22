// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::repositories::task_repository::RepositoryError;
use crate::infrastructure::database::entities::tasks_backlog::Model as TasksBacklogModel;

/// 任务积压状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TasksBacklogStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Expired,
}

impl std::fmt::Display for TasksBacklogStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TasksBacklogStatus::Pending => write!(f, "pending"),
            TasksBacklogStatus::Processing => write!(f, "processing"),
            TasksBacklogStatus::Completed => write!(f, "completed"),
            TasksBacklogStatus::Failed => write!(f, "failed"),
            TasksBacklogStatus::Expired => write!(f, "expired"),
        }
    }
}

impl std::str::FromStr for TasksBacklogStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(TasksBacklogStatus::Pending),
            "processing" => Ok(TasksBacklogStatus::Processing),
            "completed" => Ok(TasksBacklogStatus::Completed),
            "failed" => Ok(TasksBacklogStatus::Failed),
            "expired" => Ok(TasksBacklogStatus::Expired),
            _ => Err(format!("Invalid tasks backlog status: {}", s)),
        }
    }
}

/// 任务积压领域模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasksBacklog {
    pub id: Uuid,
    pub task_id: Uuid,
    pub team_id: Uuid,
    pub task_type: String,
    pub priority: i32,
    pub payload: serde_json::Value,
    pub max_retries: i32,
    pub retry_count: i32,
    pub status: TasksBacklogStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub processed_at: Option<DateTime<Utc>>,
}

impl TasksBacklog {
    /// 创建新的任务积压项
    pub fn new(
        task_id: Uuid,
        team_id: Uuid,
        task_type: String,
        priority: i32,
        payload: serde_json::Value,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            task_id,
            team_id,
            task_type,
            priority,
            payload,
            max_retries: 3,
            retry_count: 0,
            status: TasksBacklogStatus::Pending,
            created_at: now,
            updated_at: now,
            scheduled_at: None,
            expires_at,
            processed_at: None,
        }
    }

    /// 标记为处理中
    pub fn mark_processing(&mut self) -> Result<(), String> {
        if self.status != TasksBacklogStatus::Pending {
            return Err("Only pending tasks can be marked as processing".to_string());
        }
        self.status = TasksBacklogStatus::Processing;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// 标记为已完成
    pub fn mark_completed(&mut self) -> Result<(), String> {
        if self.status != TasksBacklogStatus::Processing {
            return Err("Only processing tasks can be marked as completed".to_string());
        }
        self.status = TasksBacklogStatus::Completed;
        self.processed_at = Some(Utc::now());
        self.updated_at = Utc::now();
        Ok(())
    }

    /// 标记为失败
    pub fn mark_failed(&mut self) -> Result<(), String> {
        self.status = TasksBacklogStatus::Failed;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// 标记为已过期
    pub fn mark_expired(&mut self) -> Result<(), String> {
        self.status = TasksBacklogStatus::Expired;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// 增加重试次数
    pub fn increment_retry_count(&mut self) {
        self.retry_count += 1;
        self.updated_at = Utc::now();
    }

    /// 检查是否已过期
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            return Utc::now() >= expires_at;
        }
        false
    }

    /// 检查是否可以重试
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }
}

impl From<TasksBacklogModel> for TasksBacklog {
    fn from(model: TasksBacklogModel) -> Self {
        Self {
            id: model.id,
            task_id: model.task_id,
            team_id: model.team_id,
            task_type: model.task_type,
            priority: model.priority,
            payload: model.payload,
            max_retries: model.max_retries,
            retry_count: model.retry_count,
            status: model.status.parse().unwrap_or(TasksBacklogStatus::Pending),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
            scheduled_at: model.scheduled_at.map(|dt| dt.into()),
            expires_at: model.expires_at.map(|dt| dt.into()),
            processed_at: model.processed_at.map(|dt| dt.into()),
        }
    }
}

/// 任务积压仓储接口
#[async_trait]
pub trait TasksBacklogRepository: Send + Sync {
    /// 创建任务积压项
    async fn create(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError>;

    /// 根据ID查找任务积压项
    async fn find_by_id(&self, id: Uuid) -> Result<Option<TasksBacklog>, RepositoryError>;

    /// 根据任务ID查找任务积压项
    async fn find_by_task_id(&self, task_id: Uuid)
        -> Result<Option<TasksBacklog>, RepositoryError>;

    /// 更新任务积压项
    async fn update(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError>;

    /// 删除任务积压项
    async fn delete(&self, id: Uuid) -> Result<(), RepositoryError>;

    /// 获取待处理的任务积压项（按优先级排序）
    async fn get_pending_tasks(
        &self,
        team_id: Option<Uuid>,
        limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError>;

    /// 获取过期的任务积压项
    async fn get_expired_tasks(
        &self,
        limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError>;

    /// 统计任务积压项数量
    async fn count_by_status(
        &self,
        team_id: Option<Uuid>,
        status: TasksBacklogStatus,
    ) -> Result<i64, RepositoryError>;

    /// 批量更新任务积压项状态
    async fn update_status_batch(
        &self,
        ids: &[Uuid],
        status: TasksBacklogStatus,
    ) -> Result<u64, RepositoryError>;
}
