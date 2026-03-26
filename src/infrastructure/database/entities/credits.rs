// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use dbnexus::{db_cache, db_permission, DbEntity};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DbEntity, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "credits")]
#[db_permission(roles = ["admin", "system"], operations = ["select", "insert", "update", "delete"])]
#[db_cache(ttl = 10, max_capacity = 500)]
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
