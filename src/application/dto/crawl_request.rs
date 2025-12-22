// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct CrawlRequestDto {
    #[validate(url)]
    pub url: String,
    pub name: Option<String>,
    #[validate(nested)]
    pub config: CrawlConfigDto,
    /// 同步等待时长（毫秒，默认 5000，最大 30000）
    #[validate(range(min = 0, max = 30000))]
    pub sync_wait_ms: Option<u32>,
    /// 任务过期时间
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize, Serialize, Validate, Clone)]
pub struct CrawlConfigDto {
    #[validate(range(min = 0, max = 5))]
    pub max_depth: u32,
    pub include_patterns: Option<Vec<String>>,
    pub exclude_patterns: Option<Vec<String>>,
    pub strategy: Option<String>,
    #[validate(range(min = 0, max = 60000))]
    pub crawl_delay_ms: Option<u64>,
    #[validate(range(min = 1, max = 100))]
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
