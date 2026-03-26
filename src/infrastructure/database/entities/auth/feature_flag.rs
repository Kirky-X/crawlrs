// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use dbnexus::{db_cache, db_permission, DbEntity};
use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, DbEntity, DeriveEntityModel)]
#[sea_orm(table_name = "feature_flags")]
#[db_permission(roles = ["admin"], operations = ["select", "insert", "update", "delete"])]
#[db_cache(ttl = 60, max_capacity = 100)]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub rollout_percentage: i32,
    pub metadata: sea_orm::prelude::Json,
    pub started_at: Option<ChronoDateTimeWithTimeZone>,
    pub stopped_at: Option<ChronoDateTimeWithTimeZone>,
    pub created_at: ChronoDateTimeWithTimeZone,
    pub updated_at: ChronoDateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
