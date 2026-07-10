// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 地理限制日志数据库实体模型
///
/// 对应数据库中的 geo_restriction_logs 表，存储地理限制相关的审计日志
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "geo_restriction_logs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_id: Uuid,
    pub ip_address: String,
    pub country_code: Option<String>,
    pub restriction_type: String,
    pub url: Option<String>,
    pub reason: String,
    pub created_at: ChronoDateTimeWithTimeZone,
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
            ip_address: "192.168.1.1".to_string(),
            country_code: Some("US".to_string()),
            restriction_type: "country_block".to_string(),
            url: Some("https://example.com".to_string()),
            reason: "Blocked by geo restriction policy".to_string(),
            created_at: chrono::Utc::now().fixed_offset(),
        }
    }

    #[test]
    fn test_model_construction() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let model = Model {
            id,
            team_id,
            ip_address: "10.0.0.1".to_string(),
            country_code: Some("CN".to_string()),
            restriction_type: "ip_block".to_string(),
            url: None,
            reason: "IP not in whitelist".to_string(),
            created_at: chrono::Utc::now().fixed_offset(),
        };
        assert_eq!(model.id, id);
        assert_eq!(model.team_id, team_id);
        assert_eq!(model.ip_address, "10.0.0.1");
        assert_eq!(model.country_code, Some("CN".to_string()));
        assert_eq!(model.restriction_type, "ip_block");
        assert!(model.url.is_none());
        assert_eq!(model.reason, "IP not in whitelist");
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
        assert!(debug.contains("192.168.1.1"));
        assert!(debug.contains("country_block"));
    }

    #[test]
    fn test_serde_round_trip() {
        let model = make_model();
        let json = serde_json::to_string(&model).expect("serialize");
        let deserialized: Model = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(model, deserialized);
    }

    #[test]
    fn test_model_with_none_optionals() {
        let model = Model {
            country_code: None,
            url: None,
            ..make_model()
        };
        assert!(model.country_code.is_none());
        assert!(model.url.is_none());
    }

    #[test]
    fn test_active_model_with_set_values() {
        let id = Uuid::new_v4();
        let active = ActiveModel {
            id: ActiveValue::Set(id),
            team_id: ActiveValue::Set(Uuid::new_v4()),
            ip_address: ActiveValue::Set("172.16.0.1".to_string()),
            country_code: ActiveValue::Set(Some("UK".to_string())),
            restriction_type: ActiveValue::Set("country_block".to_string()),
            url: ActiveValue::Set(Some("https://test.com".to_string())),
            reason: ActiveValue::Set("Blocked".to_string()),
            created_at: ActiveValue::Set(chrono::Utc::now().fixed_offset()),
        };
        assert_eq!(active.id.as_ref(), &id);
        assert_eq!(active.ip_address.as_ref(), &"172.16.0.1".to_string());
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
