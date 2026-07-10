// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 团队数据库实体模型
///
/// 对应数据库中的 teams 表，存储团队的基本信息和地理限制配置
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "teams")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub name: String,
    pub allowed_countries: Option<Json>,
    pub blocked_countries: Option<Json>,
    pub ip_whitelist: Option<Json>,
    pub domain_blacklist: Option<Json>,
    pub enable_geo_restrictions: bool,
    pub created_at: ChronoDateTimeWithTimeZone,
    pub updated_at: ChronoDateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        has_many = "super::api_key::Entity",
        from = "Column::Id",
        to = "super::api_key::Column::TeamId"
    )]
    ApiKeys,
    #[sea_orm(
        has_many = "super::task::Entity",
        from = "Column::Id",
        to = "super::task::Column::TeamId"
    )]
    Tasks,
    #[sea_orm(
        has_many = "super::crawl::Entity",
        from = "Column::Id",
        to = "super::crawl::Column::TeamId"
    )]
    Crawls,
    #[sea_orm(
        has_many = "super::webhook::Entity",
        from = "Column::Id",
        to = "super::webhook::Column::TeamId"
    )]
    Webhooks,
    #[sea_orm(
        has_many = "super::credits::Entity",
        from = "Column::Id",
        to = "super::credits::Column::TeamId"
    )]
    Credits,
}

impl Related<super::api_key::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ApiKeys.def()
    }
}

impl Related<super::task::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tasks.def()
    }
}

impl Related<super::crawl::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Crawls.def()
    }
}

impl Related<super::webhook::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Webhooks.def()
    }
}

impl Related<super::credits::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Credits.def()
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
            name: "Test Team".to_string(),
            allowed_countries: Some(serde_json::json!(["US", "CN"])),
            blocked_countries: Some(serde_json::json!(["RU"])),
            ip_whitelist: Some(serde_json::json!(["127.0.0.1"])),
            domain_blacklist: Some(serde_json::json!(["spam.com"])),
            enable_geo_restrictions: true,
            created_at: chrono::Utc::now().fixed_offset(),
            updated_at: chrono::Utc::now().fixed_offset(),
        }
    }

    #[test]
    fn test_model_construction() {
        let id = Uuid::new_v4();
        let model = Model {
            id,
            name: "My Team".to_string(),
            allowed_countries: None,
            blocked_countries: None,
            ip_whitelist: None,
            domain_blacklist: None,
            enable_geo_restrictions: false,
            created_at: chrono::Utc::now().fixed_offset(),
            updated_at: chrono::Utc::now().fixed_offset(),
        };
        assert_eq!(model.id, id);
        assert_eq!(model.name, "My Team");
        assert!(model.allowed_countries.is_none());
        assert!(model.blocked_countries.is_none());
        assert!(!model.enable_geo_restrictions);
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
        assert!(debug.contains("Test Team"));
        assert!(debug.contains("true"));
    }

    #[test]
    fn test_serde_round_trip() {
        let model = make_model();
        let json = serde_json::to_string(&model).expect("serialize");
        let deserialized: Model = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(model, deserialized);
    }

    #[test]
    fn test_model_with_geo_restrictions_disabled() {
        let model = Model {
            enable_geo_restrictions: false,
            allowed_countries: None,
            blocked_countries: None,
            ..make_model()
        };
        assert!(!model.enable_geo_restrictions);
        assert!(model.allowed_countries.is_none());
        assert!(model.blocked_countries.is_none());
    }

    #[test]
    fn test_active_model_with_set_values() {
        let id = Uuid::new_v4();
        let active = ActiveModel {
            id: ActiveValue::Set(id),
            name: ActiveValue::Set("New Team".to_string()),
            allowed_countries: ActiveValue::Set(None),
            blocked_countries: ActiveValue::Set(None),
            ip_whitelist: ActiveValue::Set(None),
            domain_blacklist: ActiveValue::Set(None),
            enable_geo_restrictions: ActiveValue::Set(false),
            created_at: ActiveValue::Set(chrono::Utc::now().fixed_offset()),
            updated_at: ActiveValue::Set(chrono::Utc::now().fixed_offset()),
        };
        assert_eq!(active.id.as_ref(), &id);
        assert_eq!(active.name.as_ref(), &"New Team".to_string());
    }

    #[test]
    fn test_relations_exist() {
        let _api_keys = Relation::ApiKeys;
        let _tasks = Relation::Tasks;
        let _crawls = Relation::Crawls;
        let _webhooks = Relation::Webhooks;
        let _credits = Relation::Credits;
    }

    #[test]
    fn test_relation_defs() {
        let api_keys_def = Relation::ApiKeys.def();
        assert_eq!(api_keys_def.rel_type, sea_orm::RelationType::HasMany);

        let tasks_def = Relation::Tasks.def();
        assert_eq!(tasks_def.rel_type, sea_orm::RelationType::HasMany);

        let crawls_def = Relation::Crawls.def();
        assert_eq!(crawls_def.rel_type, sea_orm::RelationType::HasMany);

        let webhooks_def = Relation::Webhooks.def();
        assert_eq!(webhooks_def.rel_type, sea_orm::RelationType::HasMany);

        let credits_def = Relation::Credits.def();
        assert_eq!(credits_def.rel_type, sea_orm::RelationType::HasMany);
    }
}
