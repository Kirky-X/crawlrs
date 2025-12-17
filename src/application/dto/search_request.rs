// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::application::dto::crawl_request::CrawlConfigDto;
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct SearchRequestDto {
    #[validate(length(min = 1, message = "Query cannot be empty"))]
    pub query: String,
    pub engine: Option<String>, // e.g., "google", "bing"
    #[serde(alias = "sources")]
    pub sources: Option<Vec<String>>, // Alias for "engine" or multi-source support
    #[validate(range(min = 1, max = 100))]
    pub limit: Option<u32>,
    pub lang: Option<String>,

    // Optional crawl configuration for async crawling of search results
    #[validate(nested)]
    pub crawl_config: Option<CrawlConfigDto>,

    // If true, will create a crawl task for each search result
    pub crawl_results: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponseDto {
    pub query: String,
    pub results: Vec<SearchResultDto>,
    pub crawl_id: Option<uuid::Uuid>, // If async crawling was triggered
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResultDto {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
}
