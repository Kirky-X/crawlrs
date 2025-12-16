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
