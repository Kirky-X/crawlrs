// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "api_keys")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_id: Uuid,
    #[sea_orm(unique)]
    pub key: String,
    /// Hash of the API key for secure storage (SHA-256 hex encoded)
    pub key_hash: Option<String>,
    pub created_at: ChronoDateTimeWithTimeZone,
    pub updated_at: Option<ChronoDateTimeWithTimeZone>,
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
            key: "ak_test_12345".to_string(),
            key_hash: Some("sha256hash".to_string()),
            created_at: chrono::Utc::now().fixed_offset(),
            updated_at: None,
        }
    }

    #[test]
    fn test_model_construction() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let model = Model {
            id,
            team_id,
            key: "ak_test_key".to_string(),
            key_hash: Some("hash123".to_string()),
            created_at: chrono::Utc::now().fixed_offset(),
            updated_at: None,
        };
        assert_eq!(model.id, id);
        assert_eq!(model.team_id, team_id);
        assert_eq!(model.key, "ak_test_key");
        assert_eq!(model.key_hash, Some("hash123".to_string()));
        assert!(model.updated_at.is_none());
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
        assert!(debug.contains("ak_test_12345"));
    }

    #[test]
    fn test_model_partial_eq() {
        let model1 = make_model();
        let model2 = model1.clone();
        assert_eq!(model1, model2);

        let model3 = Model {
            key: "different_key".to_string(),
            ..make_model()
        };
        assert_ne!(model1, model3);
    }

    #[test]
    fn test_model_with_none_key_hash() {
        let model = Model {
            key_hash: None,
            ..make_model()
        };
        assert!(model.key_hash.is_none());
    }

    #[test]
    fn test_active_model_with_set_values() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let active = ActiveModel {
            id: ActiveValue::Set(id),
            team_id: ActiveValue::Set(team_id),
            key: ActiveValue::Set("ak_new".to_string()),
            key_hash: ActiveValue::Set(None),
            created_at: ActiveValue::Set(chrono::Utc::now().fixed_offset()),
            updated_at: ActiveValue::Set(None),
        };
        assert_eq!(active.id.as_ref(), &id);
        assert_eq!(active.team_id.as_ref(), &team_id);
    }

    #[test]
    fn test_relation_team_exists() {
        // Verify Relation enum has Team variant
        let _relation = Relation::Team;
    }

    #[test]
    fn test_relation_def() {
        let def = Relation::Team.def();
        // RelationDef should be constructible
        assert_eq!(def.rel_type, sea_orm::RelationType::HasOne);
    }
}
