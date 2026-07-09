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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // ============ TaskQueryRequestDto ============

    #[test]
    fn test_task_query_request_default_values() {
        let dto = TaskQueryRequestDto::default();
        assert_eq!(dto.team_id, Uuid::nil());
        assert!(dto.task_ids.is_none());
        assert!(dto.task_types.is_none());
        assert!(dto.statuses.is_none());
        assert!(dto.created_after.is_none());
        assert!(dto.created_before.is_none());
        assert!(dto.crawl_id.is_none());
        assert_eq!(dto.limit, Some(100));
        assert_eq!(dto.offset, Some(0));
        assert_eq!(dto.include_results, Some(false));
        assert_eq!(dto.sync_wait_ms, Some(5000));
    }

    #[test]
    fn test_task_query_request_serde_roundtrip_full() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let after = FixedOffset::east_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
            .unwrap();
        let before = FixedOffset::east_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2025, 6, 30, 23, 59, 59)
            .unwrap();

        let dto = TaskQueryRequestDto {
            task_ids: Some(vec![task_id]),
            team_id,
            task_types: Some(vec![TaskType::Crawl]),
            statuses: Some(vec![TaskStatus::Queued]),
            created_after: Some(after),
            created_before: Some(before),
            crawl_id: Some(crawl_id),
            limit: Some(50),
            offset: Some(10),
            include_results: Some(true),
            sync_wait_ms: Some(2000),
        };

        let json = serde_json::to_string(&dto).expect("serialize should succeed");
        let back: TaskQueryRequestDto =
            serde_json::from_str(&json).expect("deserialize should succeed");

        assert_eq!(back.team_id, team_id);
        assert_eq!(back.task_ids, Some(vec![task_id]));
        assert_eq!(back.task_types, Some(vec![TaskType::Crawl]));
        assert_eq!(back.statuses, Some(vec![TaskStatus::Queued]));
        assert_eq!(back.created_after, Some(after));
        assert_eq!(back.created_before, Some(before));
        assert_eq!(back.crawl_id, Some(crawl_id));
        assert_eq!(back.limit, Some(50));
        assert_eq!(back.offset, Some(10));
        assert_eq!(back.include_results, Some(true));
        assert_eq!(back.sync_wait_ms, Some(2000));
    }

    #[test]
    fn test_task_query_request_serde_minimal_only_team_id() {
        let team_id = Uuid::new_v4();
        let json = format!("{{\"team_id\":\"{}\"}}", team_id);
        let dto: TaskQueryRequestDto =
            serde_json::from_str(&json).expect("deserialize minimal should succeed");
        assert_eq!(dto.team_id, team_id);
        assert!(dto.task_ids.is_none());
        assert!(dto.limit.is_none());
    }

    #[test]
    fn test_task_query_request_validation_valid() {
        let dto = TaskQueryRequestDto {
            limit: Some(1),
            sync_wait_ms: Some(0),
            ..Default::default()
        };
        assert!(dto.validate().is_ok(), "limit=1 sync_wait_ms=0 should pass");

        let dto = TaskQueryRequestDto {
            limit: Some(1000),
            sync_wait_ms: Some(30000),
            ..Default::default()
        };
        assert!(
            dto.validate().is_ok(),
            "limit=1000 sync_wait_ms=30000 should pass"
        );
    }

    #[test]
    fn test_task_query_request_validation_limit_out_of_range() {
        let dto = TaskQueryRequestDto {
            limit: Some(0),
            ..Default::default()
        };
        assert!(dto.validate().is_err(), "limit=0 should fail");

        let dto = TaskQueryRequestDto {
            limit: Some(1001),
            ..Default::default()
        };
        assert!(dto.validate().is_err(), "limit=1001 should fail");
    }

    #[test]
    fn test_task_query_request_validation_sync_wait_ms_out_of_range() {
        let dto = TaskQueryRequestDto {
            sync_wait_ms: Some(30001),
            ..Default::default()
        };
        assert!(dto.validate().is_err(), "sync_wait_ms=30001 should fail");
    }

    #[test]
    fn test_task_query_request_validation_none_values_pass() {
        let dto = TaskQueryRequestDto {
            limit: None,
            sync_wait_ms: None,
            ..Default::default()
        };
        assert!(dto.validate().is_ok(), "None values should skip validation");
    }

    // ============ TaskCancelRequestDto ============

    #[test]
    fn test_task_cancel_request_serde_and_validation() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let dto = TaskCancelRequestDto {
            task_ids: vec![task_id],
            team_id,
            force: Some(true),
            sync_wait_ms: Some(5000),
        };

        let json = serde_json::to_string(&dto).expect("serialize should succeed");
        let back: TaskCancelRequestDto =
            serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(back.task_ids, vec![task_id]);
        assert_eq!(back.team_id, team_id);
        assert_eq!(back.force, Some(true));
        assert_eq!(back.sync_wait_ms, Some(5000));

        assert!(dto.validate().is_ok(), "valid cancel dto should pass");
    }

    #[test]
    fn test_task_cancel_request_validation_sync_wait_ms_out_of_range() {
        let dto = TaskCancelRequestDto {
            task_ids: vec![Uuid::new_v4()],
            team_id: Uuid::new_v4(),
            force: None,
            sync_wait_ms: Some(30001),
        };
        assert!(dto.validate().is_err(), "sync_wait_ms=30001 should fail");
    }

    // ============ Response DTOs serialization ============

    #[test]
    fn test_task_query_data_dto_serialization() {
        let task_id = Uuid::new_v4();
        let created = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2025, 1, 1, 12, 0, 0)
            .unwrap();
        let started = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2025, 1, 1, 12, 1, 0)
            .unwrap();

        let info = TaskInfoDto {
            id: task_id,
            task_type: TaskType::Scrape,
            status: TaskStatus::Completed,
            priority: 50,
            url: "https://example.com".to_string(),
            attempt_count: 1,
            max_retries: 3,
            created_at: created,
            started_at: Some(started),
            completed_at: None,
            crawl_id: None,
            result: None,
        };

        let data = TaskQueryDataDto {
            tasks: vec![info],
            total: 1,
            has_more: false,
        };

        let json = serde_json::to_string(&data).expect("serialize should succeed");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse should succeed");
        assert_eq!(value["total"], 1);
        assert_eq!(value["has_more"], false);
        assert_eq!(value["tasks"][0]["id"], task_id.to_string());
        assert_eq!(value["tasks"][0]["task_type"], "scrape");
        assert_eq!(value["tasks"][0]["status"], "completed");
        assert_eq!(value["tasks"][0]["priority"], 50);
        assert_eq!(value["tasks"][0]["url"], "https://example.com");
        assert_eq!(value["tasks"][0]["attempt_count"], 1);
        assert_eq!(value["tasks"][0]["max_retries"], 3);
        assert_eq!(value["tasks"][0]["crawl_id"], serde_json::Value::Null);
        assert_eq!(value["tasks"][0]["result"], serde_json::Value::Null);
    }

    #[test]
    fn test_task_info_dto_with_result_serialization() {
        let result_id = Uuid::new_v4();
        let created = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2025, 1, 1, 12, 0, 0)
            .unwrap();

        let result = ScrapeResultInfoDto {
            id: result_id,
            status_code: 200,
            content: "<html>ok</html>".to_string(),
            metadata: Some(serde_json::json!({"key": "value"})),
        };

        let info = TaskInfoDto {
            id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            status: TaskStatus::Active,
            priority: 10,
            url: "https://test.com".to_string(),
            attempt_count: 0,
            max_retries: 5,
            created_at: created,
            started_at: None,
            completed_at: None,
            crawl_id: Some(Uuid::new_v4()),
            result: Some(result),
        };

        let json = serde_json::to_string(&info).expect("serialize should succeed");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse should succeed");
        assert_eq!(value["result"]["status_code"], 200);
        assert_eq!(value["result"]["content"], "<html>ok</html>");
        assert_eq!(value["result"]["metadata"]["key"], "value");
    }

    // ============ ScrapeResultInfoDto ============

    #[test]
    fn test_scrape_result_info_dto_roundtrip() {
        let dto = ScrapeResultInfoDto {
            id: Uuid::new_v4(),
            status_code: 404,
            content: "not found".to_string(),
            metadata: None,
        };

        let json = serde_json::to_string(&dto).expect("serialize should succeed");
        let back: ScrapeResultInfoDto =
            serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(back.id, dto.id);
        assert_eq!(back.status_code, 404);
        assert_eq!(back.content, "not found");
        assert!(back.metadata.is_none());
    }

    #[test]
    fn test_scrape_result_info_dto_clone() {
        let dto = ScrapeResultInfoDto {
            id: Uuid::new_v4(),
            status_code: 500,
            content: "error".to_string(),
            metadata: Some(serde_json::json!({"err": true})),
        };
        let cloned = dto.clone();
        assert_eq!(dto.id, cloned.id);
        assert_eq!(dto.status_code, cloned.status_code);
        assert_eq!(dto.content, cloned.content);
    }

    // ============ Cancel response DTOs ============

    #[test]
    fn test_task_cancel_data_dto_serialization() {
        let task_id = Uuid::new_v4();
        let failed_id = Uuid::new_v4();
        let cancelled_at = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2025, 7, 9, 10, 30, 0)
            .unwrap();

        let data = TaskCancelDataDto {
            cancelled_tasks: vec![CancelledTaskInfoDto {
                task_id,
                status: "cancelled".to_string(),
                cancelled_at,
            }],
            failed_tasks: vec![FailedTaskInfoDto {
                task_id: failed_id,
                reason: "Already completed".to_string(),
            }],
            total_cancelled: 1,
            total_failed: 1,
        };

        let json = serde_json::to_string(&data).expect("serialize should succeed");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse should succeed");
        assert_eq!(value["total_cancelled"], 1);
        assert_eq!(value["total_failed"], 1);
        assert_eq!(value["cancelled_tasks"][0]["task_id"], task_id.to_string());
        assert_eq!(value["cancelled_tasks"][0]["status"], "cancelled");
        assert_eq!(value["failed_tasks"][0]["task_id"], failed_id.to_string());
        assert_eq!(value["failed_tasks"][0]["reason"], "Already completed");
    }

    #[test]
    fn test_task_cancel_data_dto_empty() {
        let data = TaskCancelDataDto {
            cancelled_tasks: vec![],
            failed_tasks: vec![],
            total_cancelled: 0,
            total_failed: 0,
        };
        let json = serde_json::to_string(&data).expect("serialize should succeed");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse should succeed");
        assert_eq!(value["total_cancelled"], 0);
        assert_eq!(value["cancelled_tasks"].as_array().unwrap().len(), 0);
    }
}
