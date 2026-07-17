// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task Mapper - converts between Task domain model and database entity

use crate::common::time_utils::{
    from_db_datetime, from_db_datetime_opt, to_db_datetime, to_db_datetime_opt,
};
use crate::domain::models::{Task, TaskStatus, TaskType};
use crate::infrastructure::database::entities::task;
use sea_orm::ActiveValue::{Set, Unchanged};

/// Mapper for converting between Task domain model and database entity
pub struct TaskMapper;

impl TaskMapper {
    /// Convert database entity to domain model
    pub fn to_domain(entity: task::Model) -> Task {
        Task {
            id: entity.id,
            task_type: Self::parse_task_type(&entity.task_type),
            status: Self::parse_task_status(&entity.status),
            priority: entity.priority,
            team_id: entity.team_id,
            api_key_id: entity.api_key_id,
            url: entity.url,
            payload: entity.payload,
            retry_count: entity.retry_count,
            attempt_count: entity.attempt_count,
            max_retries: entity.max_retries,
            scheduled_at: from_db_datetime_opt(entity.scheduled_at),
            expires_at: from_db_datetime_opt(entity.expires_at),
            created_at: from_db_datetime(entity.created_at),
            started_at: from_db_datetime_opt(entity.started_at),
            completed_at: from_db_datetime_opt(entity.completed_at),
            crawl_id: entity.crawl_id,
            updated_at: from_db_datetime(entity.updated_at),
            lock_token: entity.lock_token,
            lock_expires_at: from_db_datetime_opt(entity.lock_expires_at),
        }
    }

    /// Convert domain model to database entity
    pub fn to_entity(domain: &Task) -> task::Model {
        task::Model {
            id: domain.id,
            task_type: domain.task_type.to_string(),
            status: domain.status.to_string(),
            priority: domain.priority,
            team_id: domain.team_id,
            api_key_id: domain.api_key_id,
            url: domain.url.clone(),
            payload: domain.payload.clone(),
            retry_count: domain.retry_count,
            attempt_count: domain.attempt_count,
            max_retries: domain.max_retries,
            scheduled_at: to_db_datetime_opt(domain.scheduled_at),
            expires_at: to_db_datetime_opt(domain.expires_at),
            created_at: to_db_datetime(domain.created_at),
            started_at: to_db_datetime_opt(domain.started_at),
            completed_at: to_db_datetime_opt(domain.completed_at),
            crawl_id: domain.crawl_id,
            updated_at: to_db_datetime(domain.updated_at),
            lock_token: domain.lock_token,
            lock_expires_at: to_db_datetime_opt(domain.lock_expires_at),
        }
    }

    /// Convert multiple entities to domain models
    pub fn to_domain_list(entities: Vec<task::Model>) -> Vec<Task> {
        entities.into_iter().map(Self::to_domain).collect()
    }

    /// Convert domain model to ActiveModel for update operations
    ///
    /// 所有字段为 Set 状态，确保 `update()` 实际更新数据。
    /// id 用 Unchanged（用于 WHERE 条件），created_at 用 Unchanged（不应更新）。
    pub fn to_active_model(domain: &Task) -> task::ActiveModel {
        let entity = Self::to_entity(domain);
        task::ActiveModel {
            id: Unchanged(entity.id),
            task_type: Set(entity.task_type),
            team_id: Set(entity.team_id),
            api_key_id: Set(entity.api_key_id),
            crawl_id: Set(entity.crawl_id),
            url: Set(entity.url),
            status: Set(entity.status),
            priority: Set(entity.priority),
            payload: Set(entity.payload),
            retry_count: Set(entity.retry_count),
            max_retries: Set(entity.max_retries),
            scheduled_at: Set(entity.scheduled_at),
            expires_at: Set(entity.expires_at),
            completed_at: Set(entity.completed_at),
            lock_token: Set(entity.lock_token),
            lock_expires_at: Set(entity.lock_expires_at),
            started_at: Set(entity.started_at),
            attempt_count: Set(entity.attempt_count),
            created_at: Unchanged(entity.created_at),
            updated_at: Set(entity.updated_at),
        }
    }

    /// Parse task type from string
    fn parse_task_type(s: &str) -> TaskType {
        s.parse().unwrap_or_default()
    }

    /// Parse task status from string
    fn parse_task_status(s: &str) -> TaskStatus {
        s.parse().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use sea_orm::ActiveValue;
    use uuid::Uuid;

    /// 构造一个填充了典型值的 task::Model，供各测试按需覆盖字段
    fn make_entity() -> task::Model {
        let now = Utc::now().fixed_offset();
        task::Model {
            id: Uuid::new_v4(),
            task_type: "scrape".to_string(),
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            crawl_id: None,
            url: "https://example.com".to_string(),
            status: "queued".to_string(),
            priority: 1,
            payload: serde_json::json!({"key": "value"}),
            retry_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            completed_at: None,
            lock_token: None,
            lock_expires_at: None,
            started_at: None,
            attempt_count: 0,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn test_task_mapper_roundtrip() {
        let now = Utc::now();
        let domain = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            status: TaskStatus::Queued,
            priority: 1,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::json!({"key": "value"}),
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
        };

        let entity = TaskMapper::to_entity(&domain);
        let back_to_domain = TaskMapper::to_domain(entity);

        assert_eq!(domain.id, back_to_domain.id);
        assert_eq!(domain.task_type, back_to_domain.task_type);
        assert_eq!(domain.status, back_to_domain.status);
        assert_eq!(domain.url, back_to_domain.url);
    }

    // ========== to_domain: TaskType 映射 ==========

    #[test]
    fn test_to_domain_scrape_type() {
        let mut entity = make_entity();
        entity.task_type = "scrape".to_string();
        let domain = TaskMapper::to_domain(entity);
        assert_eq!(domain.task_type, TaskType::Scrape);
    }

    #[test]
    fn test_to_domain_crawl_type() {
        let mut entity = make_entity();
        entity.task_type = "crawl".to_string();
        let domain = TaskMapper::to_domain(entity);
        assert_eq!(domain.task_type, TaskType::Crawl);
    }

    #[test]
    fn test_to_domain_extract_type() {
        let mut entity = make_entity();
        entity.task_type = "extract".to_string();
        let domain = TaskMapper::to_domain(entity);
        assert_eq!(domain.task_type, TaskType::Extract);
    }

    #[test]
    fn test_to_domain_invalid_task_type_falls_back_to_default() {
        // 解析失败应 fallback 到 TaskType::default() = Scrape
        for bad in ["unknown", "", "SCRAPE", "search", "scrape "].iter() {
            let mut entity = make_entity();
            entity.task_type = bad.to_string();
            let domain = TaskMapper::to_domain(entity);
            assert_eq!(
                domain.task_type,
                TaskType::Scrape,
                "invalid task_type {:?} should fall back to Scrape",
                bad
            );
        }
    }

    // ========== to_domain: TaskStatus 映射 ==========

    #[test]
    fn test_to_domain_queued_status() {
        let mut entity = make_entity();
        entity.status = "queued".to_string();
        assert_eq!(TaskMapper::to_domain(entity).status, TaskStatus::Queued);
    }

    #[test]
    fn test_to_domain_active_status() {
        let mut entity = make_entity();
        entity.status = "active".to_string();
        assert_eq!(TaskMapper::to_domain(entity).status, TaskStatus::Active);
    }

    #[test]
    fn test_to_domain_completed_status() {
        let mut entity = make_entity();
        entity.status = "completed".to_string();
        assert_eq!(TaskMapper::to_domain(entity).status, TaskStatus::Completed);
    }

    #[test]
    fn test_to_domain_failed_status() {
        let mut entity = make_entity();
        entity.status = "failed".to_string();
        assert_eq!(TaskMapper::to_domain(entity).status, TaskStatus::Failed);
    }

    #[test]
    fn test_to_domain_cancelled_status() {
        let mut entity = make_entity();
        entity.status = "cancelled".to_string();
        assert_eq!(TaskMapper::to_domain(entity).status, TaskStatus::Cancelled);
    }

    #[test]
    fn test_to_domain_invalid_status_falls_back_to_default() {
        // 解析失败应 fallback 到 TaskStatus::default() = Queued
        for bad in ["unknown", "", "QUEUED", "running", "expired"].iter() {
            let mut entity = make_entity();
            entity.status = bad.to_string();
            let domain = TaskMapper::to_domain(entity);
            assert_eq!(
                domain.status,
                TaskStatus::Queued,
                "invalid status {:?} should fall back to Queued",
                bad
            );
        }
    }

    // ========== to_entity: 字段映射 ==========

    #[test]
    fn test_to_entity_maps_all_fields() {
        let now = Utc::now();
        let domain = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            status: TaskStatus::Active,
            priority: 7,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://test.example.com/path".to_string(),
            payload: serde_json::json!({"k": "v", "n": 42}),
            retry_count: 2,
            attempt_count: 3,
            max_retries: 5,
            scheduled_at: None,
            expires_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
        };

        let entity = TaskMapper::to_entity(&domain);
        assert_eq!(entity.id, domain.id);
        assert_eq!(entity.task_type, "crawl");
        assert_eq!(entity.status, "active");
        assert_eq!(entity.priority, 7);
        assert_eq!(entity.team_id, domain.team_id);
        assert_eq!(entity.api_key_id, domain.api_key_id);
        assert_eq!(entity.url, domain.url);
        assert_eq!(entity.payload, domain.payload);
        assert_eq!(entity.retry_count, 2);
        assert_eq!(entity.attempt_count, 3);
        assert_eq!(entity.max_retries, 5);
        assert_eq!(entity.created_at.timestamp(), now.timestamp());
        assert_eq!(entity.updated_at.timestamp(), now.timestamp());
        assert!(entity.scheduled_at.is_none());
        assert!(entity.expires_at.is_none());
        assert!(entity.started_at.is_none());
        assert!(entity.completed_at.is_none());
        assert!(entity.crawl_id.is_none());
        assert!(entity.lock_token.is_none());
        assert!(entity.lock_expires_at.is_none());
    }

    // ========== to_domain_list ==========

    #[test]
    fn test_to_domain_list_empty() {
        let result = TaskMapper::to_domain_list(Vec::new());
        assert!(result.is_empty());
    }

    #[test]
    fn test_to_domain_list_single_element() {
        let entity = make_entity();
        let result = TaskMapper::to_domain_list(vec![entity.clone()]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, entity.id);
        assert_eq!(result[0].url, entity.url);
    }

    #[test]
    fn test_to_domain_list_multiple_preserves_order() {
        let e1 = make_entity();
        let mut e2 = make_entity();
        e2.url = "https://second.example.com".to_string();
        let mut e3 = make_entity();
        e3.url = "https://third.example.com".to_string();

        let input = vec![e1.clone(), e2.clone(), e3.clone()];
        let result = TaskMapper::to_domain_list(input);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].id, e1.id);
        assert_eq!(result[1].id, e2.id);
        assert_eq!(result[2].id, e3.id);
        assert_eq!(result[0].url, e1.url);
        assert_eq!(result[1].url, e2.url);
        assert_eq!(result[2].url, e3.url);
    }

    // ========== to_active_model: ActiveValue 状态 ==========

    #[test]
    fn test_to_active_model_id_is_unchanged() {
        let now = Utc::now();
        let id = Uuid::new_v4();
        let domain = Task {
            id,
            task_type: TaskType::Scrape,
            status: TaskStatus::Queued,
            priority: 1,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::json!({}),
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
        };

        let active = TaskMapper::to_active_model(&domain);
        // id 必须是 Unchanged（作为 WHERE 条件）
        assert!(matches!(active.id, ActiveValue::Unchanged(_)));
        assert_eq!(active.id.as_ref(), &id);
    }

    #[test]
    fn test_to_active_model_created_at_is_unchanged() {
        let now = Utc::now();
        let domain = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            status: TaskStatus::Queued,
            priority: 1,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::json!({}),
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
        };

        let active = TaskMapper::to_active_model(&domain);
        // created_at 必须是 Unchanged（不应更新）
        assert!(matches!(active.created_at, ActiveValue::Unchanged(_)));
    }

    #[test]
    fn test_to_active_model_other_fields_are_set() {
        let now = Utc::now();
        let domain = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            status: TaskStatus::Active,
            priority: 9,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::json!({"k": "v"}),
            retry_count: 1,
            attempt_count: 2,
            max_retries: 4,
            scheduled_at: None,
            expires_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
        };

        let active = TaskMapper::to_active_model(&domain);
        // 除 id 和 created_at 外，其他字段必须是 Set
        assert!(matches!(active.task_type, ActiveValue::Set(_)));
        assert!(matches!(active.team_id, ActiveValue::Set(_)));
        assert!(matches!(active.api_key_id, ActiveValue::Set(_)));
        assert!(matches!(active.crawl_id, ActiveValue::Set(_)));
        assert!(matches!(active.url, ActiveValue::Set(_)));
        assert!(matches!(active.status, ActiveValue::Set(_)));
        assert!(matches!(active.priority, ActiveValue::Set(_)));
        assert!(matches!(active.payload, ActiveValue::Set(_)));
        assert!(matches!(active.retry_count, ActiveValue::Set(_)));
        assert!(matches!(active.max_retries, ActiveValue::Set(_)));
        assert!(matches!(active.scheduled_at, ActiveValue::Set(_)));
        assert!(matches!(active.expires_at, ActiveValue::Set(_)));
        assert!(matches!(active.completed_at, ActiveValue::Set(_)));
        assert!(matches!(active.lock_token, ActiveValue::Set(_)));
        assert!(matches!(active.lock_expires_at, ActiveValue::Set(_)));
        assert!(matches!(active.started_at, ActiveValue::Set(_)));
        assert!(matches!(active.attempt_count, ActiveValue::Set(_)));
        assert!(matches!(active.updated_at, ActiveValue::Set(_)));

        // 值也应正确映射
        assert_eq!(active.task_type.as_ref(), "crawl");
        assert_eq!(active.status.as_ref(), "active");
        assert_eq!(active.priority.as_ref(), &9);
        assert_eq!(active.retry_count.as_ref(), &1);
        assert_eq!(active.attempt_count.as_ref(), &2);
        assert_eq!(active.max_retries.as_ref(), &4);
    }

    // ========== null/optional 字段 ==========

    #[test]
    fn test_to_domain_all_optional_fields_none() {
        let entity = make_entity(); // 默认所有 Option 字段为 None
        let domain = TaskMapper::to_domain(entity);
        assert!(domain.scheduled_at.is_none());
        assert!(domain.expires_at.is_none());
        assert!(domain.started_at.is_none());
        assert!(domain.completed_at.is_none());
        assert!(domain.crawl_id.is_none());
        assert!(domain.lock_token.is_none());
        assert!(domain.lock_expires_at.is_none());
    }

    #[test]
    fn test_to_domain_all_optional_fields_some() {
        let now = Utc::now().fixed_offset();
        let crawl_id = Uuid::new_v4();
        let lock_token = Uuid::new_v4();
        let mut entity = make_entity();
        entity.scheduled_at = Some(now);
        entity.expires_at = Some(now);
        entity.started_at = Some(now);
        entity.completed_at = Some(now);
        entity.crawl_id = Some(crawl_id);
        entity.lock_token = Some(lock_token);
        entity.lock_expires_at = Some(now);

        let domain = TaskMapper::to_domain(entity);
        assert!(domain.scheduled_at.is_some());
        assert!(domain.expires_at.is_some());
        assert!(domain.started_at.is_some());
        assert!(domain.completed_at.is_some());
        assert_eq!(domain.crawl_id, Some(crawl_id));
        assert_eq!(domain.lock_token, Some(lock_token));
        assert!(domain.lock_expires_at.is_some());
    }

    // ========== datetime 转换 ==========

    #[test]
    fn test_to_entity_with_scheduled_at_preserves_timestamp() {
        let scheduled = Utc::now() + chrono::Duration::seconds(3600);
        let now = Utc::now();
        let domain = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            status: TaskStatus::Queued,
            priority: 1,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::json!({}),
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: Some(scheduled),
            expires_at: Some(scheduled),
            created_at: now,
            started_at: Some(scheduled),
            completed_at: Some(scheduled),
            crawl_id: None,
            updated_at: now,
            lock_token: None,
            lock_expires_at: Some(scheduled),
        };

        let entity = TaskMapper::to_entity(&domain);
        assert!(entity.scheduled_at.is_some());
        assert_eq!(
            entity.scheduled_at.unwrap().timestamp(),
            scheduled.timestamp()
        );
        assert_eq!(
            entity.expires_at.unwrap().timestamp(),
            scheduled.timestamp()
        );
        assert_eq!(
            entity.started_at.unwrap().timestamp(),
            scheduled.timestamp()
        );
        assert_eq!(
            entity.completed_at.unwrap().timestamp(),
            scheduled.timestamp()
        );
        assert_eq!(
            entity.lock_expires_at.unwrap().timestamp(),
            scheduled.timestamp()
        );
    }

    #[test]
    fn test_roundtrip_preserves_all_optional_datetimes() {
        let ts = Utc::now();
        let domain = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Extract,
            status: TaskStatus::Failed,
            priority: 3,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::json!({"a": 1}),
            retry_count: 2,
            attempt_count: 3,
            max_retries: 5,
            scheduled_at: Some(ts),
            expires_at: Some(ts),
            created_at: ts,
            started_at: Some(ts),
            completed_at: Some(ts),
            crawl_id: Some(Uuid::new_v4()),
            updated_at: ts,
            lock_token: Some(Uuid::new_v4()),
            lock_expires_at: Some(ts),
        };

        let entity = TaskMapper::to_entity(&domain);
        let back = TaskMapper::to_domain(entity);

        assert_eq!(
            domain.scheduled_at.map(|t| t.timestamp()),
            back.scheduled_at.map(|t| t.timestamp())
        );
        assert_eq!(
            domain.expires_at.map(|t| t.timestamp()),
            back.expires_at.map(|t| t.timestamp())
        );
        assert_eq!(
            domain.started_at.map(|t| t.timestamp()),
            back.started_at.map(|t| t.timestamp())
        );
        assert_eq!(
            domain.completed_at.map(|t| t.timestamp()),
            back.completed_at.map(|t| t.timestamp())
        );
        assert_eq!(
            domain.lock_expires_at.map(|t| t.timestamp()),
            back.lock_expires_at.map(|t| t.timestamp())
        );
        assert_eq!(domain.crawl_id, back.crawl_id);
        assert_eq!(domain.lock_token, back.lock_token);
    }
}
