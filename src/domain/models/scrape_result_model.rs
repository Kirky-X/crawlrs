// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 纯领域模型：ScrapeResult（无 ORM 注解）
//!
//! 数据库实体定义见 `infrastructure/database/entities/scrape_result.rs`，
//! 两者通过 `ScrapeResultRepositoryImpl::to_domain/to_active_model` 转换。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrapeResult {
    pub id: crate::domain::id::Id,
    pub task_id: crate::domain::id::Id,
    pub url: String,
    pub status_code: i32,
    pub content: String,
    pub content_type: String,
    pub headers: Value,
    pub meta_data: Value,
    pub screenshot: Option<String>,
    pub response_time_ms: i64,
    pub created_at: DateTime<Utc>,
}
