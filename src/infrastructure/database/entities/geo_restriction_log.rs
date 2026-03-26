// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use dbnexus::{db_cache, db_permission, DbEntity};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 地理限制日志数据库实体模型
///
/// 对应数据库中的 geo_restriction_logs 表，存储地理限制相关的审计日志
#[derive(Clone, Debug, PartialEq, DbEntity, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "geo_restriction_logs")]
#[db_permission(roles = ["admin", "system"], operations = ["select", "insert"])]
#[db_cache(ttl = 300, max_capacity = 1000)]
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
