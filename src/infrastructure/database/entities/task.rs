// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tasks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub task_type: String,
    pub team_id: Uuid,
    pub crawl_id: Option<Uuid>,
    pub url: String,
    pub status: String,
    pub priority: i32,
    pub payload: Json,
    pub max_retries: i32,
    pub scheduled_at: Option<ChronoDateTimeWithTimeZone>,
    pub expires_at: Option<ChronoDateTimeWithTimeZone>,
    pub completed_at: Option<ChronoDateTimeWithTimeZone>,
    pub lock_token: Option<Uuid>,
    pub lock_expires_at: Option<ChronoDateTimeWithTimeZone>,
    pub started_at: Option<ChronoDateTimeWithTimeZone>,
    pub attempt_count: i32,
    pub created_at: ChronoDateTimeWithTimeZone,
    pub updated_at: ChronoDateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
