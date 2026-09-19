// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use dbnexus::sea_orm;
use dbnexus::sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "scrape_results")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub task_id: Uuid,
    pub url: String,
    pub status_code: i32,
    pub content: String,
    pub content_type: String,
    pub response_time_ms: i64,
    pub created_at: ChronoDateTimeWithTimeZone,
    pub headers: Option<Json>,
    pub meta_data: Option<Json>,
    pub screenshot: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
