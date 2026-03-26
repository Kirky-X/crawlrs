// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use dbnexus::{db_cache, db_permission, DbEntity};
use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, DbEntity, DeriveEntityModel)]
#[sea_orm(table_name = "scrape_results")]
#[db_permission(roles = ["admin", "api_user", "system", "scraper"], operations = ["select", "insert"])]
#[db_cache(ttl = 300, max_capacity = 5000)]
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
