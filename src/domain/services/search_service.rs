// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::config::settings::Settings;
use crate::domain::models::Crawl;
use crate::domain::models::CreditsTransactionType;
use crate::domain::models::{Task, TaskType};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::{CreditsRepository, CreditsRepositoryError};
use crate::domain::repositories::task_repository::TaskRepository;
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

/// Search query parameters (领域层参数对象)
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub limit: Option<u32>,
    pub lang: Option<String>,
    pub country: Option<String>,
    pub engine: Option<String>,
    pub sources: Option<Vec<String>>,
    pub crawl_results: Option<bool>,
    pub crawl_config: Option<SearchCrawlConfig>,
}

/// Search crawl configuration (领域层参数对象)
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SearchCrawlConfig {
    pub max_depth: u32,
    pub include_patterns: Option<Vec<String>>,
    pub exclude_patterns: Option<Vec<String>>,
    pub strategy: String,
    pub crawl_delay_ms: Option<u64>,
    pub max_concurrency: u32,
    pub headers: Option<serde_json::Value>,
    pub proxy: Option<String>,
    pub extraction_rules: Option<
        std::collections::HashMap<
            String,
            crate::domain::services::extraction_service::ExtractionRule,
        >,
    >,
}

/// Search result item (领域层返回对象)
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub engine: String,
}

/// Search response (领域层返回对象)
#[derive(Debug, Clone)]
pub struct SearchResponse {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub crawl_id: Option<Uuid>,
    pub credits_used: u32,
}

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

// From implementations for SearchServiceError
impl From<String> for SearchServiceError {
    fn from(msg: String) -> Self {
        SearchServiceError::ValidationError(msg)
    }
}

impl From<&str> for SearchServiceError {
    fn from(msg: &str) -> Self {
        SearchServiceError::ValidationError(msg.to_string())
    }
}

impl From<anyhow::Error> for SearchServiceError {
    fn from(err: anyhow::Error) -> Self {
        SearchServiceError::SearchEngine(err.to_string())
    }
}

use crate::search::client::SearchClientTrait;
use shaku::Interface;

/// Search service trait for trait object support.
#[async_trait::async_trait]
pub trait SearchServiceTrait: Interface + Send + Sync {
    /// Perform search operation.
    async fn search(
        &self,
        team_id: Uuid,
        api_key_id: Uuid,
        query: SearchQuery,
    ) -> Result<SearchResponse, SearchServiceError>;
}

pub struct SearchService {
    crawl_repo: Arc<dyn CrawlRepository>,
    task_repo: Arc<dyn TaskRepository>,
    credits_repo: Arc<dyn CreditsRepository>,
    search_client: Arc<dyn SearchClientTrait>,
}

impl SearchService {
    pub fn new(
        crawl_repo: Arc<dyn CrawlRepository>,
        task_repo: Arc<dyn TaskRepository>,
        credits_repo: Arc<dyn CreditsRepository>,
        _settings: Arc<Settings>,
        search_client: Arc<dyn SearchClientTrait>,
    ) -> Self {
        Self {
            crawl_repo,
            task_repo,
            credits_repo,
            search_client,
        }
    }

    pub async fn search(
        &self,
        team_id: Uuid,
        api_key_id: Uuid,
        query: SearchQuery,
    ) -> Result<SearchResponse, SearchServiceError> {
        // 简化验证：检查 query 是否为空
        if query.query.trim().is_empty() {
            return Err(SearchServiceError::ValidationError(
                "Query cannot be empty".to_string(),
            ));
        }

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
        // Use sources if provided, otherwise use engine
        let engine_param = if let Some(sources) = &query.sources {
            if sources.len() == 1 {
                Some(sources[0].as_str())
            } else {
                // Multiple sources - use aggregator (None means query all engines)
                None
            }
        } else {
            query.engine.as_deref()
        };

        let results = self
            .perform_search(
                &query.query,
                query.limit.unwrap_or(10),
                query.lang.as_deref(),
                query.country.as_deref(),
                engine_param,
            )
            .await?;

        let mut crawl_id = None;
        let credits_used = search_cost;

        // 2. If crawl_results is true, create a crawl task
        if query.crawl_results.unwrap_or(false) && !results.is_empty() {
            let cid = Uuid::new_v4();
            let _now = Utc::now();

            let config = query.crawl_config.unwrap_or(SearchCrawlConfig {
                max_depth: 1,
                include_patterns: None,
                exclude_patterns: None,
                strategy: "bfs".to_string(),
                crawl_delay_ms: None,
                max_concurrency: 10,
                headers: None,
                proxy: None,
                extraction_rules: None,
            });

            let _now = chrono::Utc::now();
            let crawl = Crawl::new(
                cid,
                team_id,
                format!("Search Crawl: {}", query.query),
                "search://".to_string() + &query.query,
                "search://".to_string() + &query.query,
                json!(config),
            );

            self.crawl_repo.create(&crawl).await?;

            for result in &results {
                let mut task = Task::new(
                    Uuid::new_v4(),
                    TaskType::Crawl,
                    team_id,
                    api_key_id,
                    result.url.clone(),
                    json!({ "crawl_id": cid, "depth": 0, "config": config }),
                );
                task.priority = 100;
                task.crawl_id = Some(cid);
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
                format!("Search query: {}", query.query),
                None,
            )
            .await?;

        Ok(SearchResponse {
            query: query.query,
            results,
            crawl_id,
            credits_used: credits_used as u32,
        })
    }

    async fn perform_search(
        &self,
        query: &str,
        limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
        engine: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchServiceError> {
        let mut command = self.search_client.search(query).await;

        command = command.limit(limit);

        if let Some(engine_name) = engine {
            command = command.with_engine(engine_name);
        }

        let response = command
            .execute()
            .await
            .map_err(|e| SearchServiceError::SearchEngine(e.to_string()))?;

        let filtered_results: Vec<SearchResult> = response
            .items
            .into_iter()
            .take(limit as usize)
            .map(|item| SearchResult {
                title: item.title,
                url: item.url,
                description: Some(item.description),
                engine: item.engine.name().to_string(),
            })
            .collect();

        Ok(filtered_results)
    }
}

/// Implement SearchServiceTrait for SearchService
#[async_trait::async_trait]
impl SearchServiceTrait for SearchService {
    async fn search(
        &self,
        team_id: Uuid,
        api_key_id: Uuid,
        query: SearchQuery,
    ) -> Result<SearchResponse, SearchServiceError> {
        Self::search(self, team_id, api_key_id, query).await
    }
}
