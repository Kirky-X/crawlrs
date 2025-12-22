// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::task::{TaskStatus, TaskType};
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

/// 任务查询请求DTO
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct TaskQueryRequestDto {
    /// 任务ID列表（批量查询）
    pub task_ids: Option<Vec<Uuid>>,

    /// 团队ID（必填）
    pub team_id: Uuid,

    /// 任务类型过滤
    pub task_types: Option<Vec<TaskType>>,

    /// 任务状态过滤
    pub statuses: Option<Vec<TaskStatus>>,

    /// 创建时间范围过滤（开始时间）
    pub created_after: Option<DateTime<FixedOffset>>,

    /// 创建时间范围过滤（结束时间）
    pub created_before: Option<DateTime<FixedOffset>>,

    /// 爬取任务ID过滤（获取特定爬取任务的所有子任务）
    pub crawl_id: Option<Uuid>,

    /// 结果包含控制
    #[validate(range(min = 1, max = 1000))]
    pub limit: Option<u32>,

    /// 分页偏移
    pub offset: Option<u32>,

    /// 是否包含任务结果数据
    pub include_results: Option<bool>,

    /// 同步等待时长（毫秒，默认 5000，最大 30000）
    #[validate(range(min = 0, max = 30000))]
    pub sync_wait_ms: Option<u32>,
}

impl Default for TaskQueryRequestDto {
    fn default() -> Self {
        Self {
            task_ids: None,
            team_id: Uuid::nil(),
            task_types: None,
            statuses: None,
            created_after: None,
            created_before: None,
            crawl_id: None,
            limit: Some(100),
            offset: Some(0),
            include_results: Some(false),
            sync_wait_ms: Some(5000),
        }
    }
}

/// 任务查询响应DTO
#[derive(Debug, Serialize)]
pub struct TaskQueryResponseDto {
    pub success: bool,
    pub status: String,
    pub data: TaskQueryDataDto,
    pub credits_used: u32,
    pub response_time_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct TaskQueryDataDto {
    pub tasks: Vec<TaskInfoDto>,
    pub total: u64,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
pub struct TaskInfoDto {
    pub id: Uuid,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub priority: i32,
    pub url: String,
    pub attempt_count: i32,
    pub max_retries: i32,
    pub created_at: DateTime<FixedOffset>,
    pub started_at: Option<DateTime<FixedOffset>>,
    pub completed_at: Option<DateTime<FixedOffset>>,
    pub crawl_id: Option<Uuid>,
    pub result: Option<serde_json::Value>,
}

/// 任务取消请求DTO
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct TaskCancelRequestDto {
    /// 任务ID列表（批量取消）
    pub task_ids: Vec<Uuid>,

    /// 团队ID（必填）
    pub team_id: Uuid,

    /// 是否强制取消（即使任务正在执行中）
    pub force: Option<bool>,

    /// 同步等待时长（毫秒，默认 5000，最大 30000）
    #[validate(range(min = 0, max = 30000))]
    pub sync_wait_ms: Option<u32>,
}

/// 任务取消响应DTO
#[derive(Debug, Serialize)]
pub struct TaskCancelResponseDto {
    pub success: bool,
    pub status: String,
    pub data: TaskCancelDataDto,
    pub credits_used: u32,
    pub response_time_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct TaskCancelDataDto {
    pub cancelled_tasks: Vec<CancelledTaskInfoDto>,
    pub failed_tasks: Vec<FailedTaskInfoDto>,
    pub total_cancelled: u64,
    pub total_failed: u64,
}

#[derive(Debug, Serialize)]
pub struct CancelledTaskInfoDto {
    pub task_id: Uuid,
    pub status: String,
    pub cancelled_at: DateTime<FixedOffset>,
}

#[derive(Debug, Serialize)]
pub struct FailedTaskInfoDto {
    pub task_id: Uuid,
    pub reason: String,
}
