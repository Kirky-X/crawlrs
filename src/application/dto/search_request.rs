// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::application::dto::crawl_request::CrawlConfigDto;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct SearchRequestDto {
    pub query: String,
    pub engine: Option<String>, // e.g., "google", "bing"
    #[serde(alias = "sources")]
    pub sources: Option<Vec<String>>, // Alias for "engine" or multi-source support
    pub limit: Option<u32>,
    pub lang: Option<String>,
    pub country: Option<String>,

    // Optional crawl configuration for async crawling of search results
    pub crawl_config: Option<CrawlConfigDto>,

    // If true, will create a crawl task for each search result
    pub crawl_results: Option<bool>,

    /// 同步等待时长（毫秒，默认 5000，最大 30000）
    pub sync_wait_ms: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponseDto {
    pub query: String,
    pub results: Vec<SearchResultDto>,
    pub crawl_id: Option<uuid::Uuid>, // If async crawling was triggered
    pub credits_used: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResultDto {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub engine: Option<String>,
}
