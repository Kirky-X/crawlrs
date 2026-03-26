// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use dbnexus::{db_cache, db_permission, DbEntity};
use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, DbEntity, DeriveEntityModel)]
#[sea_orm(table_name = "api_keys")]
#[db_permission(roles = ["admin"], operations = ["select", "insert", "update", "delete"])]
#[db_cache(ttl = 60, max_capacity = 1000)]
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
