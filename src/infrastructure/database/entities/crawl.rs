// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use dbnexus::{db_cache, db_permission, DbEntity};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 爬取任务数据库实体模型
///
/// 对应数据库中的 crawls 表，存储爬取任务的基本信息和状态
#[derive(Clone, Debug, PartialEq, DbEntity, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "crawls")]
#[db_permission(roles = ["admin", "api_user", "system", "scraper"], operations = ["select", "insert", "update", "delete"])]
#[db_cache(ttl = 60, max_capacity = 500)]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    pub root_url: String,
    pub url: String,
    pub status: String,
    pub config: Json,
    pub total_tasks: i32,
    pub completed_tasks: i32,
    pub failed_tasks: i32,
    pub created_at: ChronoDateTime,
    pub updated_at: ChronoDateTime,
    pub completed_at: Option<ChronoDateTime>,
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
