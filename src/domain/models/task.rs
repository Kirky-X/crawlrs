// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

/// 任务实体
///
/// 表示系统中一个待处理的工作单元，可以是网页抓取、
/// 网站爬取或内容提取等不同类型的任务。任务具有状态、
/// 优先级、重试机制和锁定机制等属性。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// 任务唯一标识符
    pub id: Uuid,
    /// 任务类型，决定任务的处理方式和业务逻辑
    pub task_type: TaskType,
    /// 任务状态，跟踪任务在其生命周期中的当前阶段
    pub status: TaskStatus,
    /// 任务优先级，数值越大优先级越高
    pub priority: i32,
    /// 所属团队ID，用于权限隔离和资源分配
    pub team_id: Uuid,
    /// 目标URL，任务要处理的具体网址
    pub url: String,
    /// 任务负载数据，包含任务执行所需的参数和配置
    pub payload: serde_json::Value,
    /// 已重试次数，记录任务已经尝试执行的次数
    pub attempt_count: i32,
    /// 最大重试次数，任务失败时的最大重试限制
    pub max_retries: i32,
    /// 计划执行时间，可选的延迟执行时间
    pub scheduled_at: Option<DateTime<FixedOffset>>,
    /// 过期时间，任务超过此时间将不再执行
    pub expires_at: Option<DateTime<FixedOffset>>,
    /// 创建时间，任务创建的时间戳
    pub created_at: DateTime<FixedOffset>,
    /// 开始执行时间，任务开始处理的时间戳
    pub started_at: Option<DateTime<FixedOffset>>,
    /// 完成时间，任务处理完成的时间戳
    pub completed_at: Option<DateTime<FixedOffset>>,
    /// 爬取任务ID，关联到父级爬取任务的ID（可选）
    pub crawl_id: Option<Uuid>,
    /// 更新时间，任务信息最后更新的时间戳
    pub updated_at: DateTime<FixedOffset>,
    /// 锁定令牌，用于分布式环境下的任务锁定
    pub lock_token: Option<Uuid>,
    /// 锁定过期时间，锁定自动释放的时间点
    pub lock_expires_at: Option<DateTime<FixedOffset>>,
}

/// 任务类型枚举
///
/// 定义了系统中支持的不同类型的任务，每种类型对应不同的
/// 处理逻辑和业务规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    /// 网页抓取任务，抓取单个网页的内容
    #[default]
    Scrape,
    /// 网站爬取任务，爬取整个网站或多个页面
    Crawl,
    /// 内容提取任务，从已抓取的内容中提取特定信息
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
///
/// 表示任务在其生命周期中的不同状态，用于跟踪任务的执行进度。
/// 状态转换遵循以下流程：
/// Queued → Active → Completed/Failed/Cancelled
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// 已入队，任务已创建但尚未开始执行
    #[default]
    Queued,
    /// 活跃中，任务正在被执行
    Active,
    /// 已完成，任务成功执行完成
    Completed,
    /// 已失败，任务执行失败且已达到最大重试次数
    Failed,
    /// 已取消，任务被取消执行
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
///
/// 表示在领域层可能发生的各种错误情况，包括状态转换错误、
/// 验证失败和引擎相关的错误。
#[derive(Error, Debug)]
pub enum DomainError {
    /// 无效的状态转换，当任务状态转换不符合业务规则时发生
    #[error("Invalid state transition")]
    InvalidStateTransition,

    /// 验证错误，当输入数据不符合领域规则时发生
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// 引擎错误，当底层执行引擎出现问题时发生
    #[error("Engine error: {0}")]
    EngineError(String),
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
            expires_at: None,
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
