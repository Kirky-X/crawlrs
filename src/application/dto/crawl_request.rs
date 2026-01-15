// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CrawlRequestDto {
    pub url: String,
    pub name: Option<String>,
    pub config: CrawlConfigDto,
    /// 同步等待时长（毫秒，默认 5000，最大 30000）
    pub sync_wait_ms: Option<u32>,
    /// 任务过期时间
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CrawlConfigDto {
    pub max_depth: u32,
    pub include_patterns: Option<Vec<String>>,
    pub exclude_patterns: Option<Vec<String>>,
    pub strategy: Option<String>,
    pub crawl_delay_ms: Option<u64>,
    pub max_concurrency: Option<u32>,
    pub proxy: Option<String>,
    pub headers: Option<serde_json::Value>,
    pub extraction_rules: Option<
        std::collections::HashMap<
            String,
            crate::domain::services::extraction_service::ExtractionRule,
        >,
    >,
}
