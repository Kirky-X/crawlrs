// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "credits")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_id: Uuid,
    pub balance: i64,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
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
            balance: 1000,
            created_at: chrono::Utc::now().fixed_offset(),
            updated_at: chrono::Utc::now().fixed_offset(),
        }
    }

    #[test]
    fn test_model_construction() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let model = Model {
            id,
            team_id,
            balance: 500,
            created_at: chrono::Utc::now().fixed_offset(),
            updated_at: chrono::Utc::now().fixed_offset(),
        };
        assert_eq!(model.id, id);
        assert_eq!(model.team_id, team_id);
        assert_eq!(model.balance, 500);
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
        assert!(debug.contains("1000"));
    }

    #[test]
    fn test_serde_round_trip() {
        let model = make_model();
        let json = serde_json::to_string(&model).expect("serialize");
        let deserialized: Model = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(model, deserialized);
    }

    #[test]
    fn test_model_zero_balance() {
        let model = Model {
            balance: 0,
            ..make_model()
        };
        assert_eq!(model.balance, 0);
    }

    #[test]
    fn test_model_negative_balance() {
        let model = Model {
            balance: -100,
            ..make_model()
        };
        assert_eq!(model.balance, -100);
    }

    #[test]
    fn test_active_model_with_set_values() {
        let id = Uuid::new_v4();
        let active = ActiveModel {
            id: ActiveValue::Set(id),
            team_id: ActiveValue::Set(Uuid::new_v4()),
            balance: ActiveValue::Set(2000),
            created_at: ActiveValue::Set(chrono::Utc::now().fixed_offset()),
            updated_at: ActiveValue::Set(chrono::Utc::now().fixed_offset()),
        };
        assert_eq!(active.id.as_ref(), &id);
        assert_eq!(active.balance.as_ref(), &2000);
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
