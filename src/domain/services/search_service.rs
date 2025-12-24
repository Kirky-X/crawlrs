// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::application::dto::search_request::{
    SearchRequestDto, SearchResponseDto, SearchResultDto,
};
use crate::config::settings::Settings;
use crate::domain::models::crawl::{Crawl, CrawlStatus};
use crate::domain::models::credits::CreditsTransactionType;
use crate::domain::models::task::{Task, TaskStatus, TaskType};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::{CreditsRepository, CreditsRepositoryError};
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
    #[error("Credits repository error: {0}")]
    CreditsRepository(#[from] CreditsRepositoryError),
    #[error("Search engine error: {0}")]
    SearchEngine(String),
    #[error("Insufficient credits: available {available}, required {required}")]
    InsufficientCredits { available: i64, required: i64 },
}

use crate::domain::search::engine::SearchEngine;

pub struct SearchService<CR, TR, CRR> {
    crawl_repo: Arc<CR>,
    task_repo: Arc<TR>,
    credits_repo: Arc<CRR>,
    search_engine: Arc<dyn SearchEngine>,
}

impl<CR, TR, CRR> SearchService<CR, TR, CRR>
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
    CRR: CreditsRepository + 'static,
{
    pub fn new(
        crawl_repo: Arc<CR>,
        task_repo: Arc<TR>,
        credits_repo: Arc<CRR>,
        _settings: Arc<Settings>,
        search_engine: Arc<dyn SearchEngine>,
    ) -> Self {
        Self {
            crawl_repo,
            task_repo,
            credits_repo,
            search_engine,
        }
    }

    pub async fn search(
        &self,
        team_id: Uuid,
        dto: SearchRequestDto,
    ) -> Result<SearchResponseDto, SearchServiceError> {
        dto.validate()
            .map_err(|e| SearchServiceError::ValidationError(e.to_string()))?;

        // Check team credits balance before performing search
        let current_balance = self.credits_repo.get_balance(team_id).await?;
        let search_cost = 1i64; // 1 credit per search as per PRD

        if current_balance < search_cost {
            return Err(SearchServiceError::InsufficientCredits {
                available: current_balance,
                required: search_cost,
            });
        }

        // 1. Perform Search using configured engine
        let results = self
            .perform_search(
                &dto.query,
                dto.limit.unwrap_or(10),
                dto.lang.as_deref(),
                dto.country.as_deref(),
            )
            .await?;

        let mut crawl_id = None;
        let credits_used = search_cost;

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
                    max_concurrency: Some(10),
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

        // Deduct credits for the search operation
        self.credits_repo
            .deduct_credits(
                team_id,
                search_cost,
                CreditsTransactionType::Search,
                format!("Search query: {}", dto.query),
                None,
            )
            .await?;

        Ok(SearchResponseDto {
            query: dto.query,
            results,
            crawl_id,
            credits_used: credits_used as u32,
        })
    }

    async fn perform_search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResultDto>, SearchServiceError> {
        let results = self
            .search_engine
            .search(query, limit, lang, country)
            .await
            .map_err(|e| SearchServiceError::SearchEngine(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(|item| SearchResultDto {
                title: item.title,
                url: item.url,
                description: item.description,
                engine: Some(item.engine),
            })
            .collect())
    }
}
