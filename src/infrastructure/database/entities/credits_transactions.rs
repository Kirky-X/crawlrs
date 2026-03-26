// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use dbnexus::{db_cache, db_permission, DbEntity};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DbEntity, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "credits_transactions")]
#[db_permission(roles = ["admin", "system"], operations = ["select", "insert"])]
#[db_cache(ttl = 30, max_capacity = 500)]
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
