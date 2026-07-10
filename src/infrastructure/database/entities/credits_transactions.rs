// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "credits_transactions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_id: Uuid,
    pub amount: i64,
    pub transaction_type: String,
    pub description: String,
    pub reference_id: Option<Uuid>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::api_key::Entity",
        from = "Column::TeamId",
        to = "super::api_key::Column::TeamId",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    ApiKeys,
}

impl Related<super::api_key::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ApiKeys.def()
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
            amount: 100,
            transaction_type: "credit".to_string(),
            description: "Test transaction".to_string(),
            reference_id: None,
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
            amount: 500,
            transaction_type: "debit".to_string(),
            description: "Charge for scrape".to_string(),
            reference_id: Some(Uuid::new_v4()),
            created_at: chrono::Utc::now().fixed_offset(),
        };
        assert_eq!(model.id, id);
        assert_eq!(model.team_id, team_id);
        assert_eq!(model.amount, 500);
        assert_eq!(model.transaction_type, "debit");
        assert_eq!(model.description, "Charge for scrape");
        assert!(model.reference_id.is_some());
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
        assert!(debug.contains("credit"));
        assert!(debug.contains("Test transaction"));
    }

    #[test]
    fn test_serde_round_trip() {
        let model = make_model();
        let json = serde_json::to_string(&model).expect("serialize");
        let deserialized: Model = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(model, deserialized);
    }

    #[test]
    fn test_model_negative_amount() {
        let model = Model {
            amount: -50,
            transaction_type: "debit".to_string(),
            ..make_model()
        };
        assert_eq!(model.amount, -50);
    }

    #[test]
    fn test_model_with_reference_id() {
        let ref_id = Uuid::new_v4();
        let model = Model {
            reference_id: Some(ref_id),
            ..make_model()
        };
        assert_eq!(model.reference_id, Some(ref_id));
    }

    #[test]
    fn test_active_model_with_set_values() {
        let id = Uuid::new_v4();
        let active = ActiveModel {
            id: ActiveValue::Set(id),
            team_id: ActiveValue::Set(Uuid::new_v4()),
            amount: ActiveValue::Set(250),
            transaction_type: ActiveValue::Set("credit".to_string()),
            description: ActiveValue::Set("New transaction".to_string()),
            reference_id: ActiveValue::Set(None),
            created_at: ActiveValue::Set(chrono::Utc::now().fixed_offset()),
        };
        assert_eq!(active.id.as_ref(), &id);
        assert_eq!(active.amount.as_ref(), &250);
    }

    #[test]
    fn test_relation_api_keys_exists() {
        let _relation = Relation::ApiKeys;
    }

    #[test]
    fn test_relation_def() {
        let def = Relation::ApiKeys.def();
        assert_eq!(def.rel_type, sea_orm::RelationType::HasOne);
    }
}
