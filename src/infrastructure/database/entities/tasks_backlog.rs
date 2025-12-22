// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use sea_orm::entity::prelude::*;
use uuid::Uuid;

/// 任务积压数据库实体模型
///
/// 对应数据库中的 tasks_backlog 表，存储当团队并发限制达到时的积压任务
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tasks_backlog")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub task_id: Uuid,
    pub team_id: Uuid,
    pub task_type: String,
    pub priority: i32,
    pub payload: Json,
    pub max_retries: i32,
    pub retry_count: i32,
    pub status: String,
    pub created_at: ChronoDateTimeWithTimeZone,
    pub updated_at: ChronoDateTimeWithTimeZone,
    pub scheduled_at: Option<ChronoDateTimeWithTimeZone>,
    pub expires_at: Option<ChronoDateTimeWithTimeZone>,
    pub processed_at: Option<ChronoDateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
