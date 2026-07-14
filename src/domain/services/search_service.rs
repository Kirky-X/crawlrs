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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{CrawlStatus, CreditsTransaction, CreditsTransactionType};
    use crate::domain::repositories::crawl_repository::CrawlRepository;
    use crate::domain::repositories::credits_repository::CreditsRepositoryError;
    use crate::domain::repositories::task_repository::{
        RepositoryError, TaskQueryParams, TaskRepository,
    };
    use crate::search::client::{SearchClient, SearchClientTrait, SearchCommand};
    use crate::search::engine_trait::{SearchEngine, SearchRequest};
    use crate::search::error::SearchError;
    use crate::search::response::{Response, ResponseItem};
    use crate::search::types::SearchEngineType;
    use async_trait::async_trait;
    use std::collections::HashSet;
    use std::sync::Mutex;

    // ========== Mocks ==========

    /// Configurable mock for CreditsRepository.
    struct MockCreditsRepo {
        balance: i64,
        get_balance_fails: bool,
        deduct_fails: bool,
        deduct_calls: Mutex<Vec<(Uuid, i64)>>,
    }

    impl MockCreditsRepo {
        fn with_balance(balance: i64) -> Self {
            Self {
                balance,
                get_balance_fails: false,
                deduct_fails: false,
                deduct_calls: Mutex::new(Vec::new()),
            }
        }

        fn failing() -> Self {
            Self {
                balance: 0,
                get_balance_fails: true,
                deduct_fails: false,
                deduct_calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl CreditsRepository for MockCreditsRepo {
        async fn get_balance(&self, _team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
            if self.get_balance_fails {
                return Err(CreditsRepositoryError::DatabaseError(
                    "mock get_balance failure".to_string(),
                ));
            }
            Ok(self.balance)
        }

        async fn deduct_credits(
            &self,
            team_id: Uuid,
            amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), CreditsRepositoryError> {
            if self.deduct_fails {
                return Err(CreditsRepositoryError::DatabaseError(
                    "mock deduct failure".to_string(),
                ));
            }
            self.deduct_calls.lock().unwrap().push((team_id, amount));
            Ok(())
        }

        async fn add_credits(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<i64, CreditsRepositoryError> {
            Ok(0)
        }

        async fn get_transaction_history(
            &self,
            _team_id: Uuid,
            _limit: Option<u32>,
        ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
            Ok(vec![])
        }

        async fn initialize_team_credits(
            &self,
            _team_id: Uuid,
            _initial_balance: i64,
        ) -> Result<i64, CreditsRepositoryError> {
            Ok(0)
        }
    }

    /// Minimal mock for TaskRepository (only create is used in search_service).
    struct MockTaskRepo {
        create_fails: bool,
        created: Mutex<Vec<Task>>,
    }

    impl MockTaskRepo {
        fn new() -> Self {
            Self {
                create_fails: false,
                created: Mutex::new(Vec::new()),
            }
        }

        fn failing() -> Self {
            Self {
                create_fails: true,
                created: Mutex::new(Vec::new()),
            }
        }

        fn created_count(&self) -> usize {
            self.created.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepo {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            if self.create_fails {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock task create failure"
                )));
            }
            self.created.lock().unwrap().push(task.clone());
            Ok(task.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }

        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn mark_completed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn mark_failed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn mark_cancelled(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }

        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            Ok(HashSet::new())
        }

        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(vec![])
        }

        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }

        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }
    }

    /// Minimal mock for CrawlRepository (only create is used in search_service).
    struct MockCrawlRepo {
        create_fails: bool,
        created: Mutex<Vec<Crawl>>,
    }

    impl MockCrawlRepo {
        fn new() -> Self {
            Self {
                create_fails: false,
                created: Mutex::new(Vec::new()),
            }
        }

        fn failing() -> Self {
            Self {
                create_fails: true,
                created: Mutex::new(Vec::new()),
            }
        }

        fn created_count(&self) -> usize {
            self.created.lock().unwrap().len()
        }

        fn last_created_config(&self) -> Option<serde_json::Value> {
            self.created
                .lock()
                .unwrap()
                .last()
                .map(|c| c.config().clone())
        }
    }

    #[async_trait]
    impl CrawlRepository for MockCrawlRepo {
        async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            if self.create_fails {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock crawl create failure"
                )));
            }
            self.created.lock().unwrap().push(crawl.clone());
            Ok(crawl.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
            Ok(None)
        }

        async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            Ok(crawl.clone())
        }

        async fn increment_completed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn increment_failed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn update_status(
            &self,
            _id: Uuid,
            _status: CrawlStatus,
        ) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn increment_total_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn find_by_team_id_paginated(
            &self,
            _team_id: Uuid,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<Crawl>, RepositoryError> {
            Ok(vec![])
        }

        async fn count_by_team_id(&self, _team_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
    }

    /// Mock SearchEngine that returns a preconfigured Response or error.
    ///
    /// Enables testing `perform_search` success paths by injecting this engine
    /// into a `SearchClient` via `SearchClient::new_with_engines`. The engine
    /// records the last request it saw so tests can assert forwarding behavior.
    struct MockSearchEngine {
        engine_type: SearchEngineType,
        items: Vec<ResponseItem>,
        fail: bool,
        last_request: Mutex<Option<SearchRequest>>,
    }

    impl MockSearchEngine {
        /// Build an engine that returns a successful response with the given items.
        fn success_with_items(engine_type: SearchEngineType, items: Vec<ResponseItem>) -> Self {
            Self {
                engine_type,
                items,
                fail: false,
                last_request: Mutex::new(None),
            }
        }

        /// Build an engine that always fails with `NoEngineAvailable`.
        fn failing(engine_type: SearchEngineType) -> Self {
            Self {
                engine_type,
                items: Vec::new(),
                fail: true,
                last_request: Mutex::new(None),
            }
        }

        /// Return the last SearchRequest seen by `search`, if any.
        #[allow(dead_code)]
        fn last_request(&self) -> Option<SearchRequest> {
            self.last_request.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl SearchEngine for MockSearchEngine {
        fn name(&self) -> &'static str {
            match self.engine_type {
                SearchEngineType::Google => "MockGoogle",
                SearchEngineType::Bing => "MockBing",
                SearchEngineType::Baidu => "MockBaidu",
                SearchEngineType::Sogou => "MockSogou",
                _ => "MockEngine",
            }
        }

        fn engine_type(&self) -> SearchEngineType {
            self.engine_type
        }

        fn health(&self) -> crate::search::types::EngineHealth {
            crate::search::types::EngineHealth::Healthy
        }

        async fn search(
            &self,
            request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            *self.last_request.lock().unwrap() = Some(request.clone());
            if self.fail {
                return Err(SearchError::NoEngineAvailable);
            }
            let total = self.items.len() as u64;
            Ok(Response {
                items: self.items.clone(),
                total_results: Some(total),
                engine: self.engine_type,
            })
        }
    }

    /// Build a sample ResponseItem for tests.
    fn make_response_item(title: &str, url: &str, engine: SearchEngineType) -> ResponseItem {
        ResponseItem {
            title: title.to_string(),
            url: url.to_string(),
            description: format!("desc for {}", title),
            engine,
        }
    }

    /// Mock for SearchClientTrait backed by a `SearchClient` whose engines are
    /// `MockSearchEngine` instances. This lets `perform_search` succeed or fail
    /// deterministically without touching the network.
    struct MockSearchClient {
        inner: SearchClient,
    }

    impl MockSearchClient {
        /// Build a client whose default (Google) engine returns `items`.
        fn new_with_items(items: Vec<ResponseItem>) -> Self {
            let engine: Arc<dyn SearchEngine> = Arc::new(MockSearchEngine::success_with_items(
                SearchEngineType::Google,
                items,
            ));
            Self {
                inner: SearchClient::new_with_engines(vec![engine], SearchEngineType::Google),
            }
        }

        /// Build a client whose default engine always fails.
        fn failing() -> Self {
            let engine: Arc<dyn SearchEngine> =
                Arc::new(MockSearchEngine::failing(SearchEngineType::Google));
            Self {
                inner: SearchClient::new_with_engines(vec![engine], SearchEngineType::Google),
            }
        }

        /// Default success client with two generic results (kept for
        /// existing `make_service` callers that don't assert result shape).
        fn new() -> Self {
            Self::new_with_items(vec![
                make_response_item(
                    "Result 1",
                    "https://example.com/1",
                    SearchEngineType::Google,
                ),
                make_response_item(
                    "Result 2",
                    "https://example.com/2",
                    SearchEngineType::Google,
                ),
            ])
        }
    }

    #[async_trait]
    impl SearchClientTrait for MockSearchClient {
        async fn search(&self, query: &str) -> SearchCommand {
            self.inner.search(query)
        }

        async fn search_with_engine(
            &self,
            _query: &str,
            _engine: SearchEngineType,
        ) -> Result<Response<ResponseItem>, SearchError> {
            Err(SearchError::NoEngineAvailable)
        }

        fn default_engine(&self) -> SearchEngineType {
            SearchEngineType::Google
        }
    }

    // ========== Helpers ==========

    fn make_service(credits: Arc<dyn CreditsRepository>) -> SearchService {
        SearchService {
            crawl_repo: Arc::new(MockCrawlRepo::new()),
            task_repo: Arc::new(MockTaskRepo::new()),
            credits_repo: credits,
            search_client: Arc::new(MockSearchClient::new()),
        }
    }

    fn make_query(text: &str) -> SearchQuery {
        SearchQuery {
            query: text.to_string(),
            limit: None,
            lang: None,
            country: None,
            engine: None,
            sources: None,
            crawl_results: None,
            crawl_config: None,
        }
    }

    /// Build a service with recording mock repos, returning the mock handles
    /// so tests can inspect side effects (task/crawl creation, credit calls).
    fn make_service_with_recording(
        credits: Arc<dyn CreditsRepository>,
        search_client: Arc<dyn SearchClientTrait>,
    ) -> (SearchService, Arc<MockCrawlRepo>, Arc<MockTaskRepo>) {
        let crawl_repo = Arc::new(MockCrawlRepo::new());
        let task_repo = Arc::new(MockTaskRepo::new());
        let service = SearchService {
            crawl_repo: crawl_repo.clone(),
            task_repo: task_repo.clone(),
            credits_repo: credits,
            search_client,
        };
        (service, crawl_repo, task_repo)
    }

    /// Build a service whose search client always fails.
    fn make_failing_search_service(credits: Arc<dyn CreditsRepository>) -> SearchService {
        SearchService {
            crawl_repo: Arc::new(MockCrawlRepo::new()),
            task_repo: Arc::new(MockTaskRepo::new()),
            credits_repo: credits,
            search_client: Arc::new(MockSearchClient::failing()),
        }
    }

    // ========== SearchQuery tests ==========

    #[test]
    fn test_search_query_construction_with_all_fields() {
        let query = SearchQuery {
            query: "rust web scraping".to_string(),
            limit: Some(20),
            lang: Some("en".to_string()),
            country: Some("us".to_string()),
            engine: Some("google".to_string()),
            sources: Some(vec!["google".to_string(), "bing".to_string()]),
            crawl_results: Some(true),
            crawl_config: Some(SearchCrawlConfig::default()),
        };
        assert_eq!(query.query, "rust web scraping");
        assert_eq!(query.limit, Some(20));
        assert_eq!(query.lang.as_deref(), Some("en"));
        assert_eq!(query.country.as_deref(), Some("us"));
        assert_eq!(query.engine.as_deref(), Some("google"));
        assert_eq!(query.sources.as_ref().unwrap().len(), 2);
        assert_eq!(query.crawl_results, Some(true));
        assert!(query.crawl_config.is_some());
    }

    #[test]
    fn test_search_query_with_minimal_fields() {
        let query = make_query("test");
        assert_eq!(query.query, "test");
        assert!(query.limit.is_none());
        assert!(query.lang.is_none());
        assert!(query.crawl_results.is_none());
    }

    #[test]
    fn test_search_query_empty_string() {
        let query = make_query("");
        assert!(query.query.is_empty());
    }

    // ========== SearchCrawlConfig tests ==========

    #[test]
    fn test_search_crawl_config_default_values() {
        let config = SearchCrawlConfig::default();
        assert_eq!(config.max_depth, 0, "default max_depth should be 0");
        assert_eq!(
            config.strategy, "",
            "default strategy should be empty string"
        );
        assert_eq!(
            config.max_concurrency, 0,
            "default max_concurrency should be 0"
        );
        assert!(config.include_patterns.is_none());
        assert!(config.exclude_patterns.is_none());
        assert!(config.crawl_delay_ms.is_none());
        assert!(config.headers.is_none());
        assert!(config.proxy.is_none());
        assert!(config.extraction_rules.is_none());
    }

    #[test]
    fn test_search_crawl_config_serde_roundtrip() {
        let config = SearchCrawlConfig {
            max_depth: 3,
            include_patterns: Some(vec!["*/blog/*".to_string()]),
            exclude_patterns: Some(vec!["*/admin/*".to_string()]),
            strategy: "bfs".to_string(),
            crawl_delay_ms: Some(500),
            max_concurrency: 10,
            headers: Some(serde_json::json!({"User-Agent": "test"})),
            proxy: Some("http://proxy:8080".to_string()),
            extraction_rules: None,
        };
        let json = serde_json::to_string(&config).expect("should serialize");
        let back: SearchCrawlConfig = serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(back.max_depth, 3);
        assert_eq!(back.strategy, "bfs");
        assert_eq!(back.max_concurrency, 10);
        assert_eq!(back.crawl_delay_ms, Some(500));
        assert_eq!(
            back.include_patterns.as_ref().unwrap(),
            &vec!["*/blog/*".to_string()]
        );
    }

    #[test]
    fn test_search_crawl_config_serde_minimal() {
        let config = SearchCrawlConfig::default();
        let json = serde_json::to_string(&config).expect("should serialize");
        let back: SearchCrawlConfig = serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(config.max_depth, back.max_depth);
        assert_eq!(config.strategy, back.strategy);
    }

    // ========== SearchResult tests ==========

    #[test]
    fn test_search_result_construction() {
        let result = SearchResult {
            title: "Rust Programming".to_string(),
            url: "https://www.rust-lang.org".to_string(),
            description: Some(
                "A language empowering everyone to build reliable software.".to_string(),
            ),
            engine: "Google".to_string(),
        };
        assert_eq!(result.title, "Rust Programming");
        assert_eq!(result.url, "https://www.rust-lang.org");
        assert!(result.description.is_some());
        assert_eq!(result.engine, "Google");
    }

    #[test]
    fn test_search_result_with_none_description() {
        let result = SearchResult {
            title: "Test".to_string(),
            url: "https://example.com".to_string(),
            description: None,
            engine: "Bing".to_string(),
        };
        assert!(result.description.is_none());
    }

    // ========== SearchResponse tests ==========

    #[test]
    fn test_search_response_construction_with_crawl() {
        let response = SearchResponse {
            query: "rust scraping".to_string(),
            results: vec![SearchResult {
                title: "Result 1".to_string(),
                url: "https://example.com".to_string(),
                description: None,
                engine: "Google".to_string(),
            }],
            crawl_id: Some(Uuid::new_v4()),
            credits_used: 1,
        };
        assert_eq!(response.query, "rust scraping");
        assert_eq!(response.results.len(), 1);
        assert!(response.crawl_id.is_some());
        assert_eq!(response.credits_used, 1);
    }

    #[test]
    fn test_search_response_construction_without_crawl() {
        let response = SearchResponse {
            query: "test".to_string(),
            results: vec![],
            crawl_id: None,
            credits_used: 1,
        };
        assert!(response.results.is_empty());
        assert!(response.crawl_id.is_none());
    }

    // ========== SearchServiceError From conversions ==========

    #[test]
    fn test_search_service_error_from_string() {
        let err: SearchServiceError = "bad query".to_string().into();
        match err {
            SearchServiceError::ValidationError(msg) => {
                assert_eq!(msg, "bad query");
            }
            other => panic!("expected ValidationError, got {:?}", other),
        }
    }

    #[test]
    fn test_search_service_error_from_str_ref() {
        let err: SearchServiceError = "invalid input".into();
        match err {
            SearchServiceError::ValidationError(msg) => {
                assert_eq!(msg, "invalid input");
            }
            other => panic!("expected ValidationError, got {:?}", other),
        }
    }

    #[test]
    fn test_search_service_error_from_anyhow_error() {
        let err: SearchServiceError = anyhow::anyhow!("engine crashed").into();
        match err {
            SearchServiceError::SearchEngine(msg) => {
                assert!(msg.contains("engine crashed"), "msg: {}", msg);
            }
            other => panic!("expected SearchEngine, got {:?}", other),
        }
    }

    #[test]
    fn test_search_service_error_display_validation() {
        let err = SearchServiceError::ValidationError("missing field".to_string());
        let msg = err.to_string();
        assert!(msg.contains("Validation failed"));
        assert!(msg.contains("missing field"));
    }

    #[test]
    fn test_search_service_error_display_insufficient_credits() {
        let err = SearchServiceError::InsufficientCredits {
            available: 0,
            required: 1,
        };
        let msg = err.to_string();
        assert!(msg.contains("Insufficient credits"));
        assert!(msg.contains("available 0"));
        assert!(msg.contains("required 1"));
    }

    #[test]
    fn test_search_service_error_display_search_engine() {
        let err = SearchServiceError::SearchEngine("timeout".to_string());
        let msg = err.to_string();
        assert!(msg.contains("Search engine error"));
        assert!(msg.contains("timeout"));
    }

    // ========== search() pre-validation tests ==========

    #[tokio::test]
    async fn test_search_empty_query_returns_validation_error() {
        let service = make_service(Arc::new(MockCreditsRepo::with_balance(100)));
        let query = make_query("");
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchServiceError::ValidationError(msg) => {
                assert!(msg.contains("empty"), "error should mention empty: {}", msg);
            }
            other => panic!("expected ValidationError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_search_whitespace_only_query_returns_validation_error() {
        let service = make_service(Arc::new(MockCreditsRepo::with_balance(100)));
        let query = make_query("   ");
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_err(), "whitespace-only query should fail");
        match result.unwrap_err() {
            SearchServiceError::ValidationError(_) => {}
            other => panic!("expected ValidationError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_search_insufficient_credits_returns_error() {
        let service = make_service(Arc::new(MockCreditsRepo::with_balance(0)));
        let query = make_query("rust scraping");
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchServiceError::InsufficientCredits {
                available,
                required,
            } => {
                assert_eq!(available, 0);
                assert_eq!(required, 1);
            }
            other => panic!("expected InsufficientCredits, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_search_exact_balance_succeeds_validation() {
        // Balance exactly equals search_cost (1) → passes credit check.
        // perform_search succeeds with the mock engine, so we expect Ok with
        // credits_used == 1. The key assertion is that InsufficientCredits is
        // NOT returned.
        let service = make_service(Arc::new(MockCreditsRepo::with_balance(1)));
        let query = make_query("rust scraping");
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(
            result.is_ok(),
            "should pass credit check and perform_search, got {:?}",
            result.err()
        );
        let resp = result.unwrap();
        assert_eq!(resp.credits_used, 1, "search cost is 1 credit");
    }

    // ========== engine_param branch tests ==========
    // These tests exercise the sources/engine parameter handling logic in search().
    // perform_search succeeds with the injected MockSearchEngine (default Google),
    // so each test asserts Ok, proving the code reached perform_search and that the
    // engine_param branches did not short-circuit into ValidationError/InsufficientCredits.

    #[tokio::test]
    async fn test_search_with_single_source_passes_source_as_engine() {
        // sources.len() == 1 → engine_param = Some(sources[0]) = Some("google")
        // MockSearchEngine (Google) handles the request successfully.
        let service = make_service(Arc::new(MockCreditsRepo::with_balance(10)));
        let query = SearchQuery {
            sources: Some(vec!["google".to_string()]),
            ..make_query("rust scraping")
        };
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(
            result.is_ok(),
            "should reach perform_search and succeed, got {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_search_with_multiple_sources_uses_aggregator() {
        // sources.len() > 1 → engine_param = None (aggregator)
        // perform_search falls back to the default (Google) mock engine and succeeds.
        let service = make_service(Arc::new(MockCreditsRepo::with_balance(10)));
        let query = SearchQuery {
            sources: Some(vec!["google".to_string(), "bing".to_string()]),
            ..make_query("rust scraping")
        };
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(
            result.is_ok(),
            "aggregator path should reach perform_search and succeed, got {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_search_with_engine_specified_passes_engine() {
        // sources = None, engine = Some → engine_param = Some(engine)
        let service = make_service(Arc::new(MockCreditsRepo::with_balance(10)));
        let query = SearchQuery {
            engine: Some("bing".to_string()),
            ..make_query("rust scraping")
        };
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchServiceError::ValidationError(_) => {
                panic!("should pass validation");
            }
            SearchServiceError::InsufficientCredits { .. } => {
                panic!("should pass credit check");
            }
            _ => { /* reached perform_search — expected */ }
        }
    }

    #[tokio::test]
    async fn test_search_with_limit_specified_passes_to_perform_search() {
        // Exercise the query.limit.unwrap_or(10) branch with explicit limit.
        // MockSearchClient::new() returns 2 items, so limit=5 yields 2 results.
        let service = make_service(Arc::new(MockCreditsRepo::with_balance(10)));
        let query = SearchQuery {
            limit: Some(5),
            lang: Some("en".to_string()),
            country: Some("us".to_string()),
            ..make_query("rust scraping")
        };
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(
            result.is_ok(),
            "should reach perform_search and succeed, got {:?}",
            result.err()
        );
        let resp = result.unwrap();
        assert_eq!(
            resp.results.len(),
            2,
            "mock returns 2 items, limit=5 keeps both"
        );
    }

    #[tokio::test]
    async fn test_search_get_balance_failure_returns_credits_error() {
        let service = make_service(Arc::new(MockCreditsRepo::failing()));
        let query = make_query("rust scraping");
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchServiceError::CreditsRepository(err) => match err {
                CreditsRepositoryError::DatabaseError(msg) => {
                    assert!(msg.contains("mock get_balance failure"));
                }
                other => panic!("expected DatabaseError, got {:?}", other),
            },
            other => panic!("expected CreditsRepository, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_search_negative_balance_returns_insufficient_credits() {
        let service = make_service(Arc::new(MockCreditsRepo::with_balance(-5)));
        let query = make_query("rust scraping");
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchServiceError::InsufficientCredits {
                available,
                required,
            } => {
                assert_eq!(available, -5, "should report negative balance");
                assert_eq!(required, 1);
            }
            other => panic!("expected InsufficientCredits, got {:?}", other),
        }
    }

    // ========== SearchServiceTrait forwarding test ==========

    #[tokio::test]
    async fn test_search_service_trait_forwards_empty_query_error() {
        let service = make_service(Arc::new(MockCreditsRepo::with_balance(100)));
        let query = make_query("");
        // Use the trait method
        let result: Result<SearchResponse, SearchServiceError> =
            SearchServiceTrait::search(&service, Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchServiceError::ValidationError(_) => {}
            other => panic!("expected ValidationError, got {:?}", other),
        }
    }

    // ========== perform_search success path tests ==========
    //
    // These tests rely on MockSearchEngine (injected via SearchClient::new_with_engines)
    // to exercise the success branch of `perform_search` and the downstream crawl/credit
    // flows that were previously unreachable with a real SearchClient.

    #[tokio::test]
    async fn test_search_success_returns_results() {
        let credits = Arc::new(MockCreditsRepo::with_balance(100));
        let (service, _, _) =
            make_service_with_recording(credits, Arc::new(MockSearchClient::new()));
        let query = make_query("rust web scraping");
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_ok(), "expected ok, got {:?}", result.err());
        let resp = result.unwrap();
        assert_eq!(resp.query, "rust web scraping");
        assert_eq!(
            resp.results.len(),
            2,
            "MockSearchClient::new returns 2 items"
        );
        assert_eq!(resp.credits_used, 1, "search cost is 1 credit");
        assert!(resp.crawl_id.is_none(), "crawl_results not set");
    }

    #[tokio::test]
    async fn test_search_success_deducts_credits() {
        let credits = Arc::new(MockCreditsRepo::with_balance(100));
        let credits_handle: Arc<MockCreditsRepo> = credits.clone();
        let (service, _, _) =
            make_service_with_recording(credits, Arc::new(MockSearchClient::new()));
        let query = make_query("rust");
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_ok());
        let calls = credits_handle.deduct_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1, "deduct_credits should be called once");
        assert_eq!(calls[0].1, 1, "deducted amount should be 1");
    }

    #[tokio::test]
    async fn test_search_success_with_crawl_creates_tasks_and_crawl() {
        let (service, crawl_repo, task_repo) = make_service_with_recording(
            Arc::new(MockCreditsRepo::with_balance(100)),
            Arc::new(MockSearchClient::new()),
        );
        let query = SearchQuery {
            crawl_results: Some(true),
            ..make_query("rust scraping")
        };
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.crawl_id.is_some(), "crawl_id should be set");
        assert_eq!(
            crawl_repo.created_count(),
            1,
            "exactly one crawl record should be created"
        );
        assert_eq!(
            task_repo.created_count(),
            resp.results.len(),
            "one task per search result should be created"
        );
    }

    #[tokio::test]
    async fn test_search_success_with_crawl_skips_when_no_results() {
        let credits = Arc::new(MockCreditsRepo::with_balance(100));
        let (service, crawl_repo, task_repo) = make_service_with_recording(
            credits,
            Arc::new(MockSearchClient::new_with_items(vec![])),
        );
        let query = SearchQuery {
            crawl_results: Some(true),
            ..make_query("empty")
        };
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.crawl_id.is_none(), "no crawl when results empty");
        assert_eq!(crawl_repo.created_count(), 0);
        assert_eq!(task_repo.created_count(), 0);
    }

    #[tokio::test]
    async fn test_search_respects_limit_truncates_results() {
        let credits = Arc::new(MockCreditsRepo::with_balance(100));
        let (service, _, _) = make_service_with_recording(
            credits,
            Arc::new(MockSearchClient::new_with_items(vec![
                make_response_item("a", "https://a.example", SearchEngineType::Google),
                make_response_item("b", "https://b.example", SearchEngineType::Google),
                make_response_item("c", "https://c.example", SearchEngineType::Google),
                make_response_item("d", "https://d.example", SearchEngineType::Google),
            ])),
        );
        let query = SearchQuery {
            limit: Some(2),
            ..make_query("limited")
        };
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(
            resp.results.len(),
            2,
            "limit=2 should truncate to 2 results"
        );
    }

    #[tokio::test]
    async fn test_search_perform_search_failure_returns_search_engine_error() {
        let service = make_failing_search_service(Arc::new(MockCreditsRepo::with_balance(100)));
        let query = make_query("will fail");
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchServiceError::SearchEngine(msg) => {
                assert!(!msg.is_empty(), "error message should be non-empty");
            }
            other => panic!("expected SearchEngine error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_search_crawl_repo_failure_propagates() {
        let credits: Arc<dyn CreditsRepository> = Arc::new(MockCreditsRepo::with_balance(100));
        let service = SearchService {
            crawl_repo: Arc::new(MockCrawlRepo::failing()),
            task_repo: Arc::new(MockTaskRepo::new()),
            credits_repo: credits,
            search_client: Arc::new(MockSearchClient::new()),
        };
        let query = SearchQuery {
            crawl_results: Some(true),
            ..make_query("crawl fails")
        };
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchServiceError::Repository(_) => { /* expected */ }
            other => panic!("expected Repository error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_search_task_repo_failure_propagates() {
        let credits: Arc<dyn CreditsRepository> = Arc::new(MockCreditsRepo::with_balance(100));
        let service = SearchService {
            crawl_repo: Arc::new(MockCrawlRepo::new()),
            task_repo: Arc::new(MockTaskRepo::failing()),
            credits_repo: credits,
            search_client: Arc::new(MockSearchClient::new()),
        };
        let query = SearchQuery {
            crawl_results: Some(true),
            ..make_query("task fails")
        };
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchServiceError::Repository(_) => { /* expected */ }
            other => panic!("expected Repository error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_search_deduct_credits_failure_propagates() {
        let credits = Arc::new(MockCreditsRepo {
            balance: 100,
            get_balance_fails: false,
            deduct_fails: true,
            deduct_calls: Mutex::new(Vec::new()),
        });
        let (service, _, _) =
            make_service_with_recording(credits, Arc::new(MockSearchClient::new()));
        let query = make_query("deduct fails");
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchServiceError::CreditsRepository(_) => { /* expected */ }
            other => panic!("expected CreditsRepository error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_search_trait_delegates_to_success_path() {
        let service = make_service(Arc::new(MockCreditsRepo::with_balance(100)));
        let query = make_query("trait delegation");
        let result: Result<SearchResponse, SearchServiceError> =
            SearchServiceTrait::search(&service, Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_ok(), "trait should forward to success path");
        let resp = result.unwrap();
        assert_eq!(resp.results.len(), 2);
    }

    // ========== Mock stub contract tests ==========
    //
    // search() only exercises `create` on the task/crawl repos and
    // `get_balance`/`deduct_credits` on the credits repo. The remaining methods
    // are stubs that return safe defaults. Calling them here documents the stub
    // contract and keeps coverage accounting honest for the mock implementations.

    #[tokio::test]
    async fn test_mock_credits_repo_unused_methods_return_expected_defaults() {
        let credits = MockCreditsRepo::with_balance(10);
        let team_id = Uuid::new_v4();
        // add_credits returns 0 (stub does not track new balance)
        assert_eq!(
            credits
                .add_credits(
                    team_id,
                    5,
                    CreditsTransactionType::Search,
                    "add".to_string(),
                    None
                )
                .await
                .unwrap(),
            0
        );
        // get_transaction_history returns empty
        assert!(credits
            .get_transaction_history(team_id, None)
            .await
            .unwrap()
            .is_empty());
        // initialize_team_credits returns 0
        assert_eq!(
            credits.initialize_team_credits(team_id, 100).await.unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn test_mock_task_repo_unused_methods_return_expected_defaults() {
        let task_repo = MockTaskRepo::new();
        let task_id = Uuid::new_v4();
        // Read/query stubs return "not found" / empty
        assert!(task_repo.find_by_id(task_id).await.unwrap().is_none());
        assert!(task_repo
            .acquire_next(Uuid::new_v4())
            .await
            .unwrap()
            .is_none());
        assert!(!task_repo
            .exists_by_url("https://example.com")
            .await
            .unwrap());
        // State-transition stubs are no-ops returning Ok
        task_repo.mark_completed(task_id).await.unwrap();
        task_repo.mark_failed(task_id).await.unwrap();
        task_repo.mark_cancelled(task_id).await.unwrap();
        // Maintenance stubs return zero counts
        assert_eq!(task_repo.expire_tasks().await.unwrap(), 0);
        assert_eq!(
            task_repo
                .cancel_tasks_by_crawl_id(Uuid::new_v4())
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn test_mock_crawl_repo_unused_methods_return_expected_defaults() {
        let crawl_repo = MockCrawlRepo::new();
        let crawl_id = Uuid::new_v4();
        // find_by_id returns None (stub)
        assert!(crawl_repo.find_by_id(crawl_id).await.unwrap().is_none());
        // Counter/status stubs are no-ops returning Ok
        crawl_repo
            .increment_completed_tasks(crawl_id)
            .await
            .unwrap();
        crawl_repo.increment_failed_tasks(crawl_id).await.unwrap();
        crawl_repo.increment_total_tasks(crawl_id).await.unwrap();
        crawl_repo
            .update_status(crawl_id, CrawlStatus::Completed)
            .await
            .unwrap();
        // Query stubs return empty / zero
        assert!(crawl_repo
            .find_by_team_id_paginated(crawl_id, 10, 0)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(crawl_repo.count_by_team_id(crawl_id).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_mock_task_repo_remaining_methods_return_defaults() {
        let task_repo = MockTaskRepo::new();
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            json!({}),
        );
        // update echoes the task back
        let updated = task_repo.update(&task).await.unwrap();
        assert_eq!(updated.id, task.id);
        // find_existing_urls returns empty set
        let existing = task_repo
            .find_existing_urls(&["https://a.com".to_string()])
            .await
            .unwrap();
        assert!(existing.is_empty());
        // reset_stuck_tasks returns 0
        assert_eq!(
            task_repo
                .reset_stuck_tasks(chrono::Duration::minutes(5))
                .await
                .unwrap(),
            0
        );
        // find_by_crawl_id returns empty vec
        assert!(task_repo
            .find_by_crawl_id(Uuid::new_v4())
            .await
            .unwrap()
            .is_empty());
        // query_tasks returns empty vec + 0 count
        let (tasks, count) = task_repo
            .query_tasks(crate::domain::repositories::task_repository::TaskQueryParams::default())
            .await
            .unwrap();
        assert!(tasks.is_empty());
        assert_eq!(count, 0);
        // batch_cancel returns empty tuples
        let (cancelled, failed) = task_repo
            .batch_cancel(vec![Uuid::new_v4()], Uuid::new_v4(), false)
            .await
            .unwrap();
        assert!(cancelled.is_empty());
        assert!(failed.is_empty());
    }

    #[tokio::test]
    async fn test_mock_crawl_repo_update_echoes_back() {
        let crawl_repo = MockCrawlRepo::new();
        let crawl = Crawl::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "Test".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            json!({}),
        );
        let result = crawl_repo.update(&crawl).await.unwrap();
        assert_eq!(result.id, crawl.id);
    }

    #[test]
    fn test_mock_search_engine_name_engine_type_health_all_variants() {
        // Named branches in name(): Google, Bing, Baidu, Sogou
        for engine_type in [
            SearchEngineType::Google,
            SearchEngineType::Bing,
            SearchEngineType::Baidu,
            SearchEngineType::Sogou,
        ] {
            let engine = MockSearchEngine::success_with_items(engine_type, vec![]);
            assert_eq!(engine.engine_type(), engine_type);
            let _ = engine.name();
            assert!(matches!(
                engine.health(),
                crate::search::types::EngineHealth::Healthy
            ));
        }
        // Catch-all branch in name(): Auto, Smart, ABTest
        for engine_type in [
            SearchEngineType::Auto,
            SearchEngineType::Smart,
            SearchEngineType::ABTest,
        ] {
            let engine = MockSearchEngine::success_with_items(engine_type, vec![]);
            assert_eq!(engine.engine_type(), engine_type);
            assert_eq!(engine.name(), "MockEngine");
            assert!(matches!(
                engine.health(),
                crate::search::types::EngineHealth::Healthy
            ));
        }
    }

    #[test]
    fn test_mock_search_engine_last_request_is_none_initially() {
        let engine = MockSearchEngine::success_with_items(SearchEngineType::Google, vec![]);
        assert!(engine.last_request().is_none());
    }

    fn make_test_settings_for_search() -> Settings {
        use crate::config::settings::*;
        Settings {
            server: ServerSettings::default(),
            database: DatabaseSettings::default(),
            redis: RedisSettings::default(),
            cors: CorsSettings::default(),
            rate_limiting: RateLimitingSettings::default(),
            concurrency: ConcurrencySettings::default(),
            webhook: WebhookSettings::default(),
            bing_search: BingSearchSettings::default(),
            search: SearchSettings::default(),
            llm: LLMSettings::default(),
            proxy: ProxySettings::default(),
            engines: EngineSettings::default(),
            logging: LoggingSettings::default(),
            workers: WorkerSettings::default(),
            timeouts: TimeoutSettings::default(),
            cache: CacheSettings::default(),
            trusted_proxies: TrustedProxySettings::default(),
        }
    }

    #[test]
    fn test_search_service_new_constructor_assigns_dependencies() {
        let settings = make_test_settings_for_search();
        let service = SearchService::new(
            Arc::new(MockCrawlRepo::new()),
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockCreditsRepo::with_balance(10)),
            Arc::new(settings),
            Arc::new(MockSearchClient::new()),
        );
        // Verify the service is usable by performing a search that exercises
        // the injected mocks (empty query → ValidationError before touching repos)
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(service.search(Uuid::new_v4(), Uuid::new_v4(), make_query("")))
            .unwrap_err();
        assert!(matches!(
            err,
            SearchServiceError::ValidationError(ref msg) if msg.contains("empty")
        ));
    }

    // ========== crawl_config custom path tests ==========

    #[tokio::test]
    async fn test_search_with_crawl_results_and_custom_crawl_config() {
        // Exercises the `query.crawl_config.unwrap_or(default)` branch where
        // crawl_config is Some(custom). Verifies the custom config values are
        // serialized into the created Crawl record (not the defaults).
        let credits = Arc::new(MockCreditsRepo::with_balance(100));
        let (service, crawl_repo, task_repo) =
            make_service_with_recording(credits, Arc::new(MockSearchClient::new()));
        let custom_config = SearchCrawlConfig {
            max_depth: 5,
            include_patterns: Some(vec!["*/blog/*".to_string()]),
            exclude_patterns: Some(vec!["*/admin/*".to_string()]),
            strategy: "dfs".to_string(),
            crawl_delay_ms: Some(1000),
            max_concurrency: 20,
            headers: Some(json!({"User-Agent": "CrawlrsBot"})),
            proxy: Some("http://proxy:8080".to_string()),
            extraction_rules: None,
        };
        let query = SearchQuery {
            crawl_results: Some(true),
            crawl_config: Some(custom_config),
            ..make_query("custom crawl config")
        };
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_ok(), "expected ok, got {:?}", result.err());
        let resp = result.unwrap();
        assert!(resp.crawl_id.is_some(), "crawl_id should be set");
        assert_eq!(crawl_repo.created_count(), 1, "one crawl record created");
        assert_eq!(
            task_repo.created_count(),
            resp.results.len(),
            "one task per result"
        );
        // Verify the custom config (not defaults) was serialized into the crawl
        let config = crawl_repo
            .last_created_config()
            .expect("crawl record should exist");
        assert_eq!(config["max_depth"], 5, "custom max_depth should be 5");
        assert_eq!(config["strategy"], "dfs", "custom strategy should be dfs");
        assert_eq!(
            config["max_concurrency"], 20,
            "custom max_concurrency should be 20"
        );
        assert_eq!(
            config["crawl_delay_ms"], 1000,
            "custom crawl_delay_ms should be 1000"
        );
        assert_eq!(
            config["proxy"], "http://proxy:8080",
            "custom proxy should be set"
        );
        assert_eq!(
            config["include_patterns"][0], "*/blog/*",
            "custom include_patterns should be set"
        );
        assert_eq!(
            config["exclude_patterns"][0], "*/admin/*",
            "custom exclude_patterns should be set"
        );
    }

    #[tokio::test]
    async fn test_search_with_crawl_results_default_config_uses_bfs_strategy() {
        // Exercises the `query.crawl_config.unwrap_or(default)` branch where
        // crawl_config is None. Verifies the default config uses strategy="bfs"
        // and max_depth=1, confirming the default branch is distinct from the
        // custom config branch.
        let credits = Arc::new(MockCreditsRepo::with_balance(100));
        let (service, crawl_repo, _) =
            make_service_with_recording(credits, Arc::new(MockSearchClient::new()));
        let query = SearchQuery {
            crawl_results: Some(true),
            crawl_config: None,
            ..make_query("default crawl config")
        };
        let result = service.search(Uuid::new_v4(), Uuid::new_v4(), query).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.crawl_id.is_some());
        let config = crawl_repo
            .last_created_config()
            .expect("crawl record should exist");
        assert_eq!(config["max_depth"], 1, "default max_depth should be 1");
        assert_eq!(config["strategy"], "bfs", "default strategy should be bfs");
        assert_eq!(
            config["max_concurrency"], 10,
            "default max_concurrency should be 10"
        );
    }

    // ========== SearchServiceError display tests for remaining variants ==========

    #[test]
    fn test_search_service_error_display_repository() {
        let inner = RepositoryError::Database(anyhow::anyhow!("db connection lost"));
        let err = SearchServiceError::Repository(inner);
        let msg = err.to_string();
        assert!(msg.contains("Repository error"));
        assert!(msg.contains("db connection lost"));
    }

    #[test]
    fn test_search_service_error_display_credits_repository() {
        let inner = CreditsRepositoryError::DatabaseError("redis timeout".to_string());
        let err = SearchServiceError::CreditsRepository(inner);
        let msg = err.to_string();
        assert!(msg.contains("Credits repository error"));
        assert!(msg.contains("redis timeout"));
    }
}
