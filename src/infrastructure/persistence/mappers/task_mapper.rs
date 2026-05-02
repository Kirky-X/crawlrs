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
    use uuid::Uuid;

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
}
