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

use crate::application::dto::search_request::{
    SearchRequestDto, SearchResponseDto, SearchResultDto,
};
use crate::domain::models::crawl::{Crawl, CrawlStatus};
use crate::domain::models::task::{Task, TaskStatus, TaskType};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;
use validator::Validate;

#[derive(Error, Debug)]
pub enum SearchServiceError {
    #[error("Validation failed: {0}")]
    ValidationError(String),
    #[error("Repository error: {0}")]
    Repository(#[from] crate::domain::repositories::task_repository::RepositoryError),
    #[error("Search engine error: {0}")]
    SearchEngine(String),
}

pub struct SearchService<CR, TR> {
    crawl_repo: Arc<CR>,
    task_repo: Arc<TR>,
}

impl<CR, TR> SearchService<CR, TR>
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
{
    pub fn new(crawl_repo: Arc<CR>, task_repo: Arc<TR>) -> Self {
        Self {
            crawl_repo,
            task_repo,
        }
    }

    pub async fn search(
        &self,
        team_id: Uuid,
        dto: SearchRequestDto,
    ) -> Result<SearchResponseDto, SearchServiceError> {
        dto.validate()
            .map_err(|e| SearchServiceError::ValidationError(e.to_string()))?;

        // 1. Perform Search using configured engine
        let results = self
            .perform_search(&dto.query, dto.limit.unwrap_or(10))
            .await?;

        let mut crawl_id = None;

        // 2. If crawl_results is true, create a crawl task
        if dto.crawl_results.unwrap_or(false) && !results.is_empty() {
            let cid = Uuid::new_v4();
            let now = Utc::now();

            let config = dto.crawl_config.unwrap_or(
                crate::application::dto::crawl_request::CrawlConfigDto {
                    max_depth: 1,
                    include_patterns: None,
                    exclude_patterns: None,
                    strategy: Some("bfs".to_string()),
                    crawl_delay_ms: None,
                    headers: None,
                    proxy: None,
                    extraction_rules: None,
                },
            );

            let crawl = Crawl {
                id: cid,
                team_id,
                name: format!("Search Crawl: {}", dto.query),
                root_url: "search://".to_string() + &dto.query,
                url: "search://".to_string() + &dto.query,
                status: CrawlStatus::Queued,
                config: json!(config),
                total_tasks: results.len() as i32,
                completed_tasks: 0,
                failed_tasks: 0,
                created_at: now,
                updated_at: now,
                completed_at: None,
            };

            self.crawl_repo.create(&crawl).await?;

            for result in &results {
                let task = Task {
                    id: Uuid::new_v4(),
                    task_type: TaskType::Crawl,
                    status: TaskStatus::Queued,
                    priority: 100,
                    team_id,
                    url: result.url.clone(),
                    payload: json!({ "crawl_id": cid, "depth": 0, "config": config }),
                    attempt_count: 0,
                    max_retries: 3,
                    scheduled_at: None,
                    created_at: now.into(),
                    started_at: None,
                    completed_at: None,
                    crawl_id: Some(cid),
                    updated_at: now.into(),
                    lock_token: None,
                    lock_expires_at: None,
                    expires_at: None,
                };
                self.task_repo.create(&task).await?;
            }

            crawl_id = Some(cid);
        }

        Ok(SearchResponseDto {
            query: dto.query,
            results,
            crawl_id,
        })
    }

    async fn perform_search(
        &self,
        _query: &str,
        _limit: u32,
    ) -> Result<Vec<SearchResultDto>, SearchServiceError> {
        // Check for search engine configuration
        let google_key = std::env::var("GOOGLE_SEARCH_API_KEY").ok();
        let google_cx = std::env::var("GOOGLE_SEARCH_CX").ok();

        if let (Some(_key), Some(_cx)) = (google_key, google_cx) {
            // TODO: Implement actual Google Custom Search API call
            // For now, we return an error to indicate it's not fully implemented rather than returning mock data
            return Err(SearchServiceError::SearchEngine(
                "Google Search integration is not yet implemented".to_string(),
            ));
        }

        // If no search engine is configured, return error
        Err(SearchServiceError::SearchEngine(
            "No search engine configured. Please set GOOGLE_SEARCH_API_KEY and GOOGLE_SEARCH_CX."
                .to_string(),
        ))
    }
}
