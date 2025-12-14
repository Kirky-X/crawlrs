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

use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

/// 任务实体
///
/// 表示系统中一个待处理的工作单元
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// 任务唯一标识符
    pub id: Uuid,
    /// 任务类型
    pub task_type: TaskType,
    /// 任务状态
    pub status: TaskStatus,
    /// 任务优先级
    pub priority: i32,
    /// 所属团队ID
    pub team_id: Uuid,
    /// 目标URL
    pub url: String,
    /// 任务负载数据
    pub payload: serde_json::Value,
    /// 已重试次数
    pub attempt_count: i32,
    /// 最大重试次数
    pub max_retries: i32,
    /// 计划执行时间
    pub scheduled_at: Option<DateTime<FixedOffset>>,
    /// 创建时间
    pub created_at: DateTime<FixedOffset>,
    /// 开始执行时间
    pub started_at: Option<DateTime<FixedOffset>>,
    /// 完成时间
    pub completed_at: Option<DateTime<FixedOffset>>,
    /// 爬取任务ID
    pub crawl_id: Option<Uuid>,
    /// 更新时间
    pub updated_at: DateTime<FixedOffset>,
    /// 锁定令牌
    pub lock_token: Option<Uuid>,
    /// 锁定过期时间
    pub lock_expires_at: Option<DateTime<FixedOffset>>,
}

/// 任务类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    /// 网页抓取任务
    #[default]
    Scrape,
    /// 网站爬取任务
    Crawl,
    /// 内容提取任务
    Extract,
}

impl fmt::Display for TaskType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TaskType::Scrape => write!(f, "scrape"),
            TaskType::Crawl => write!(f, "crawl"),
            TaskType::Extract => write!(f, "extract"),
        }
    }
}

impl FromStr for TaskType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "scrape" => Ok(TaskType::Scrape),
            "crawl" => Ok(TaskType::Crawl),
            "extract" => Ok(TaskType::Extract),
            _ => Err(()),
        }
    }
}

/// 任务状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// 已入队
    #[default]
    Queued,
    /// 活跃中
    Active,
    /// 已完成
    Completed,
    /// 已失败
    Failed,
    /// 已取消
    Cancelled,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TaskStatus::Queued => write!(f, "queued"),
            TaskStatus::Active => write!(f, "active"),
            TaskStatus::Completed => write!(f, "completed"),
            TaskStatus::Failed => write!(f, "failed"),
            TaskStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl FromStr for TaskStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(TaskStatus::Queued),
            "active" => Ok(TaskStatus::Active),
            "completed" => Ok(TaskStatus::Completed),
            "failed" => Ok(TaskStatus::Failed),
            "cancelled" => Ok(TaskStatus::Cancelled),
            _ => Err(()),
        }
    }
}

/// 领域错误类型
#[derive(Error, Debug)]
pub enum DomainError {
    /// 无效的状态转换
    #[error("Invalid state transition")]
    InvalidStateTransition,
}

impl Task {
    /// 创建一个新的任务
    ///
    /// # 参数
    ///
    /// * `task_type` - 任务类型
    /// * `team_id` - 所属团队ID
    /// * `url` - 目标URL
    /// * `payload` - 任务负载数据
    ///
    /// # 返回值
    ///
    /// 返回新创建的任务实例
    pub fn new(
        task_type: TaskType,
        team_id: Uuid,
        url: String,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            task_type,
            status: TaskStatus::Queued,
            priority: 0,
            team_id,
            url,
            payload,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            created_at: Utc::now().into(),
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: Utc::now().into(),
            lock_token: None,
            lock_expires_at: None,
        }
    }

    /// 启动任务
    ///
    /// 将任务状态从Queued变更为Active
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 成功启动的任务
    /// * `Err(DomainError)` - 状态转换失败
    pub fn start(mut self) -> Result<Self, DomainError> {
        match self.status {
            TaskStatus::Queued => {
                self.status = TaskStatus::Active;
                self.started_at = Some(Utc::now().into());
                Ok(self)
            }
            _ => Err(DomainError::InvalidStateTransition),
        }
    }

    /// 完成任务
    ///
    /// 将任务状态从Active变更为Completed
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 成功完成的任务
    /// * `Err(DomainError)` - 状态转换失败
    pub fn complete(mut self) -> Result<Self, DomainError> {
        match self.status {
            TaskStatus::Active => {
                self.status = TaskStatus::Completed;
                self.completed_at = Some(Utc::now().into());
                Ok(self)
            }
            _ => Err(DomainError::InvalidStateTransition),
        }
    }

    /// 标记任务失败
    ///
    /// 将任务状态从Active变更为Failed
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 失败的任务
    /// * `Err(DomainError)` - 状态转换失败
    pub fn fail(mut self) -> Result<Self, DomainError> {
        match self.status {
            TaskStatus::Active => {
                self.status = TaskStatus::Failed;
                self.completed_at = Some(Utc::now().into());
                Ok(self)
            }
            _ => Err(DomainError::InvalidStateTransition),
        }
    }

    /// 取消任务
    ///
    /// 将任务状态变更为Cancelled
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 已取消的任务
    /// * `Err(DomainError)` - 状态转换失败
    pub fn cancel(mut self) -> Result<Self, DomainError> {
        match self.status {
            TaskStatus::Queued | TaskStatus::Active => {
                self.status = TaskStatus::Cancelled;
                self.completed_at = Some(Utc::now().into());
                Ok(self)
            }
            _ => Err(DomainError::InvalidStateTransition),
        }
    }

    /// 判断任务是否可以重试
    ///
    /// # 返回值
    ///
    /// 如果任务处于失败状态且未达到最大重试次数则返回true，否则返回false
    pub fn can_retry(&self) -> bool {
        self.status == TaskStatus::Failed && self.attempt_count < self.max_retries
    }
}
