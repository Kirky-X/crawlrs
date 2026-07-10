// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 爬取任务数据库实体模型
///
/// 对应数据库中的 crawls 表，存储爬取任务的基本信息和状态
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "crawls")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    pub root_url: String,
    pub url: String,
    pub status: String,
    pub config: Json,
    pub total_tasks: i32,
    pub completed_tasks: i32,
    pub failed_tasks: i32,
    pub created_at: ChronoDateTime,
    pub updated_at: ChronoDateTime,
    pub completed_at: Option<ChronoDateTime>,
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
            team_id: Uuid::new_v4(),
            name: "Test Crawl".to_string(),
            root_url: "https://example.com".to_string(),
            url: "https://example.com".to_string(),
            status: "running".to_string(),
            config: serde_json::json!({"depth": 3}),
            total_tasks: 10,
            completed_tasks: 3,
            failed_tasks: 1,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
            completed_at: None,
        }
    }

    #[test]
    fn test_model_construction() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let model = Model {
            id,
            team_id,
            name: "My Crawl".to_string(),
            root_url: "https://root.com".to_string(),
            url: "https://root.com/page".to_string(),
            status: "pending".to_string(),
            config: serde_json::json!({"max_pages": 100}),
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
            completed_at: None,
        };
        assert_eq!(model.id, id);
        assert_eq!(model.team_id, team_id);
        assert_eq!(model.name, "My Crawl");
        assert_eq!(model.root_url, "https://root.com");
        assert_eq!(model.url, "https://root.com/page");
        assert_eq!(model.status, "pending");
        assert_eq!(model.total_tasks, 0);
        assert_eq!(model.completed_tasks, 0);
        assert_eq!(model.failed_tasks, 0);
        assert!(model.completed_at.is_none());
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
        assert!(debug.contains("Test Crawl"));
        assert!(debug.contains("running"));
    }

    #[test]
    fn test_serde_round_trip() {
        let model = make_model();
        let json = serde_json::to_string(&model).expect("serialize");
        let deserialized: Model = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(model, deserialized);
    }

    #[test]
    fn test_model_with_completed_at() {
        let now = chrono::Utc::now().naive_utc();
        let model = Model {
            completed_at: Some(now),
            status: "completed".to_string(),
            completed_tasks: 10,
            ..make_model()
        };
        assert!(model.completed_at.is_some());
        assert_eq!(model.status, "completed");
        assert_eq!(model.completed_tasks, 10);
    }

    #[test]
    fn test_active_model_with_set_values() {
        let id = Uuid::new_v4();
        let active = ActiveModel {
            id: ActiveValue::Set(id),
            team_id: ActiveValue::Set(Uuid::new_v4()),
            name: ActiveValue::Set("New Crawl".to_string()),
            root_url: ActiveValue::Set("https://new.com".to_string()),
            url: ActiveValue::Set("https://new.com".to_string()),
            status: ActiveValue::Set("pending".to_string()),
            config: ActiveValue::Set(serde_json::json!({})),
            total_tasks: ActiveValue::Set(0),
            completed_tasks: ActiveValue::Set(0),
            failed_tasks: ActiveValue::Set(0),
            created_at: ActiveValue::Set(chrono::Utc::now().naive_utc()),
            updated_at: ActiveValue::Set(chrono::Utc::now().naive_utc()),
            completed_at: ActiveValue::Set(None),
        };
        assert_eq!(active.id.as_ref(), &id);
        assert_eq!(active.name.as_ref(), &"New Crawl".to_string());
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
