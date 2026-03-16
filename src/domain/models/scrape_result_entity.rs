// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! ScrapeResult entity definition using dbnexus

use dbnexus::{db_crud, db_permission};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// ScrapeResult entity
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "scrape_results")]
#[db_crud(table_name = "scrape_results")]
#[db_permission(roles = ["admin", "scraper", "viewer"], operations = ["SELECT", "INSERT", "UPDATE", "DELETE"])]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    #[sea_orm(column_name = "task_id", column_type = "Uuid")]
    pub task_id: Uuid,
    pub url: String,
    #[sea_orm(column_name = "status_code")]
    pub status_code: i32,
    pub content: String,
    #[sea_orm(column_name = "content_type")]
    pub content_type: String,
    pub headers: Json,
    #[sea_orm(column_name = "meta_data")]
    pub meta_data: Json,
    pub screenshot: Option<String>,
    #[sea_orm(column_name = "response_time_ms")]
    pub response_time_ms: i64,
    #[sea_orm(column_name = "created_at")]
    pub created_at: ChronoDateTime,
}

impl ActiveModelBehavior for ActiveModel {}

/// Relation enum for ScrapeResult entity
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
