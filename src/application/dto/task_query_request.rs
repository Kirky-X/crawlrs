// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::{TaskStatus, TaskType};
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

/// 任务查询响应数据DTO
#[derive(Debug, Serialize)]
pub struct TaskQueryDataDto {
    /// 任务列表
    pub tasks: Vec<TaskInfoDto>,
    /// 总数
    pub total: u64,
    /// 是否有更多
    pub has_more: bool,
}

/// 任务信息DTO
#[derive(Debug, Serialize)]
pub struct TaskInfoDto {
    /// 任务ID
    pub id: Uuid,
    /// 任务类型
    pub task_type: TaskType,
    /// 任务状态
    pub status: TaskStatus,
    /// 优先级
    pub priority: i32,
    /// URL
    pub url: String,
    /// 尝试次数
    pub attempt_count: i32,
    /// 最大重试次数
    pub max_retries: i32,
    /// 创建时间
    pub created_at: DateTime<FixedOffset>,
    /// 开始时间
    pub started_at: Option<DateTime<FixedOffset>>,
    /// 完成时间
    pub completed_at: Option<DateTime<FixedOffset>>,
    /// 爬取任务ID
    pub crawl_id: Option<Uuid>,
    /// 结果数据
    pub result: Option<ScrapeResultInfoDto>,
}

/// 抓取结果信息DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeResultInfoDto {
    /// 结果ID
    pub id: Uuid,
    /// HTTP状态码
    pub status_code: u16,
    /// 内容
    pub content: String,
    /// 元数据
    pub metadata: Option<Value>,
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

/// 任务取消响应数据DTO
#[derive(Debug, Serialize)]
pub struct TaskCancelDataDto {
    /// 已取消的任务列表
    pub cancelled_tasks: Vec<CancelledTaskInfoDto>,
    /// 失败的任务列表
    pub failed_tasks: Vec<FailedTaskInfoDto>,
    /// 已取消总数
    pub total_cancelled: u64,
    /// 失败总数
    pub total_failed: u64,
}

/// 已取消任务信息DTO
#[derive(Debug, Serialize)]
pub struct CancelledTaskInfoDto {
    /// 任务ID
    pub task_id: Uuid,
    /// 状态
    pub status: String,
    /// 取消时间
    pub cancelled_at: DateTime<FixedOffset>,
}

/// 失败任务信息DTO
#[derive(Debug, Serialize)]
pub struct FailedTaskInfoDto {
    /// 任务ID
    pub task_id: Uuid,
    /// 失败原因
    pub reason: String,
}
