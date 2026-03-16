// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task Mapper - converts between Task domain model and database entity

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
            scheduled_at: entity.scheduled_at.map(|dt| dt.with_timezone(&chrono::Utc)),
            expires_at: entity.expires_at.map(|dt| dt.with_timezone(&chrono::Utc)),
            created_at: entity.created_at.with_timezone(&chrono::Utc),
            started_at: entity.started_at.map(|dt| dt.with_timezone(&chrono::Utc)),
            completed_at: entity.completed_at.map(|dt| dt.with_timezone(&chrono::Utc)),
            crawl_id: entity.crawl_id,
            updated_at: entity.updated_at.with_timezone(&chrono::Utc),
            lock_token: entity.lock_token,
            lock_expires_at: entity.lock_expires_at.map(|dt| dt.with_timezone(&chrono::Utc)),
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
            scheduled_at: domain.scheduled_at.map(|dt| dt.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())),
            expires_at: domain.expires_at.map(|dt| dt.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())),
            created_at: domain.created_at.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap()),
            started_at: domain.started_at.map(|dt| dt.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())),
            completed_at: domain.completed_at.map(|dt| dt.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())),
            crawl_id: domain.crawl_id,
            updated_at: domain.updated_at.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap()),
            lock_token: domain.lock_token,
            lock_expires_at: domain.lock_expires_at.map(|dt| dt.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())),
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
    use chrono::{TimeZone, Utc};
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
