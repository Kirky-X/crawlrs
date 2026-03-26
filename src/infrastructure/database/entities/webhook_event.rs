// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use dbnexus::{db_cache, db_permission, DbEntity};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum SeaWebhookStatus {
    #[sea_orm(string_value = "pending")]
    Pending,
    #[sea_orm(string_value = "delivered")]
    Delivered,
    #[sea_orm(string_value = "failed")]
    Failed,
    #[sea_orm(string_value = "dead")]
    Dead,
}

#[derive(Clone, Debug, PartialEq, DbEntity, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "webhook_events")]
#[db_permission(roles = ["admin", "system", "scraper"], operations = ["select", "insert", "update"])]
#[db_cache(ttl = 30, max_capacity = 1000)]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_id: Uuid,
    pub webhook_id: Option<Uuid>,
    pub event_type: String,
    pub status: SeaWebhookStatus,
    pub payload: JsonValue,
    pub webhook_url: String,
    pub response_status: Option<i32>,
    pub response_body: Option<String>,
    pub error_message: Option<String>,
    pub attempt_count: i32,
    pub max_retries: i32,
    pub next_retry_at: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    pub delivered_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
