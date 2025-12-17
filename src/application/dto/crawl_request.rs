// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CrawlRequestDto {
    #[validate(url)]
    pub url: String,
    pub name: Option<String>,
    #[validate(nested)]
    pub config: CrawlConfigDto,
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
    pub proxy: Option<String>,
    pub headers: Option<serde_json::Value>,
    pub extraction_rules: Option<
        std::collections::HashMap<
            String,
            crate::domain::services::extraction_service::ExtractionRule,
        >,
    >,
}
