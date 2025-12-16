// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

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
