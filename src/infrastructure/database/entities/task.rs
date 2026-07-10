// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use sea_orm::entity::prelude::*;
use uuid::Uuid;

/// 任务数据库实体模型
///
/// 对应数据库中的 tasks 表，存储任务的详细信息和执行状态
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tasks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub task_type: String,
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub crawl_id: Option<Uuid>,
    pub url: String,
    pub status: String,
    pub priority: i32,
    pub payload: Json,
    pub retry_count: i32,
    pub max_retries: i32,
    pub scheduled_at: Option<ChronoDateTimeWithTimeZone>,
    pub expires_at: Option<ChronoDateTimeWithTimeZone>,
    pub completed_at: Option<ChronoDateTimeWithTimeZone>,
    pub lock_token: Option<Uuid>,
    pub lock_expires_at: Option<ChronoDateTimeWithTimeZone>,
    pub started_at: Option<ChronoDateTimeWithTimeZone>,
    pub attempt_count: i32,
    pub created_at: ChronoDateTimeWithTimeZone,
    pub updated_at: ChronoDateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::team::Entity",
        from = "Column::TeamId",
        to = "super::team::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Team,
}

impl Related<super::team::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Team.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ActiveValue;

    fn make_model() -> Model {
        Model {
            id: Uuid::new_v4(),
            task_type: "scrape".to_string(),
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            crawl_id: None,
            url: "https://example.com".to_string(),
            status: "pending".to_string(),
            priority: 5,
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
            created_at: chrono::Utc::now().fixed_offset(),
            updated_at: chrono::Utc::now().fixed_offset(),
        }
    }

    #[test]
    fn test_model_construction() {
        let id = Uuid::new_v4();
        let model = make_model();
        let model = Model { id, ..model };
        assert_eq!(model.id, id);
        assert_eq!(model.task_type, "scrape");
        assert_eq!(model.url, "https://example.com");
        assert_eq!(model.status, "pending");
        assert_eq!(model.priority, 5);
        assert_eq!(model.retry_count, 0);
        assert_eq!(model.max_retries, 3);
        assert_eq!(model.attempt_count, 0);
        assert!(model.crawl_id.is_none());
        assert!(model.scheduled_at.is_none());
        assert!(model.expires_at.is_none());
        assert!(model.completed_at.is_none());
        assert!(model.lock_token.is_none());
        assert!(model.lock_expires_at.is_none());
        assert!(model.started_at.is_none());
    }

    #[test]
    fn test_model_clone() {
        let model = make_model();
        let cloned = model.clone();
        assert_eq!(model, cloned);
    }

    #[test]
    fn test_model_debug() {
        let model = make_model();
        let debug = format!("{:?}", model);
        assert!(debug.contains("Model"));
        assert!(debug.contains("scrape"));
        assert!(debug.contains("pending"));
    }

    #[test]
    fn test_model_with_optional_fields_set() {
        let crawl_id = Uuid::new_v4();
        let lock_token = Uuid::new_v4();
        let now = chrono::Utc::now().fixed_offset();
        let model = Model {
            crawl_id: Some(crawl_id),
            scheduled_at: Some(now),
            expires_at: Some(now),
            completed_at: Some(now),
            lock_token: Some(lock_token),
            lock_expires_at: Some(now),
            started_at: Some(now),
            ..make_model()
        };
        assert_eq!(model.crawl_id, Some(crawl_id));
        assert!(model.scheduled_at.is_some());
        assert!(model.expires_at.is_some());
        assert!(model.completed_at.is_some());
        assert_eq!(model.lock_token, Some(lock_token));
        assert!(model.lock_expires_at.is_some());
        assert!(model.started_at.is_some());
    }

    #[test]
    fn test_active_model_with_set_values() {
        let id = Uuid::new_v4();
        let active = ActiveModel {
            id: ActiveValue::Set(id),
            task_type: ActiveValue::Set("crawl".to_string()),
            team_id: ActiveValue::Set(Uuid::new_v4()),
            api_key_id: ActiveValue::Set(Uuid::new_v4()),
            crawl_id: ActiveValue::Set(None),
            url: ActiveValue::Set("https://test.com".to_string()),
            status: ActiveValue::Set("queued".to_string()),
            priority: ActiveValue::Set(10),
            payload: ActiveValue::Set(serde_json::json!({})),
            retry_count: ActiveValue::Set(0),
            max_retries: ActiveValue::Set(5),
            scheduled_at: ActiveValue::Set(None),
            expires_at: ActiveValue::Set(None),
            completed_at: ActiveValue::Set(None),
            lock_token: ActiveValue::Set(None),
            lock_expires_at: ActiveValue::Set(None),
            started_at: ActiveValue::Set(None),
            attempt_count: ActiveValue::Set(0),
            created_at: ActiveValue::Set(chrono::Utc::now().fixed_offset()),
            updated_at: ActiveValue::Set(chrono::Utc::now().fixed_offset()),
        };
        assert_eq!(active.id.as_ref(), &id);
        assert_eq!(active.task_type.as_ref(), &"crawl".to_string());
        assert_eq!(active.priority.as_ref(), &10);
    }

    #[test]
    fn test_relation_team_exists() {
        let _relation = Relation::Team;
    }

    #[test]
    fn test_relation_def() {
        let def = Relation::Team.def();
        assert_eq!(def.rel_type, sea_orm::RelationType::HasOne);
    }
}
