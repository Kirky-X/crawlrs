// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use crate::config::SearchSettings;
use crate::engines::engine_client::EngineClient;
use std::sync::Arc;

use super::{
    engine_trait::{SearchEngine, SearchRequest},
    error::SearchError,
    response::{Response, ResponseItem},
    types::SearchEngineType,
};

pub mod baidu;
pub mod bing;
pub mod exa;
pub mod html_parser;
pub mod shared_utils;
pub mod sogou;
pub mod tavily;

pub use baidu::BaiduSearchEngine;
pub use bing::BingSearchEngine;
pub use exa::{ExaConfig, ExaSearchEngine};
pub use shared_utils::{parse_first_selector, safe_parse_selector};
pub use sogou::SogouSearchEngine;
pub use tavily::{TavilyConfig, TavilySearchEngine};

/// Trait for SearchClient - enables dependency injection
#[async_trait::async_trait]
pub trait SearchClientTrait: Send + Sync {
    /// Search with default engine
    async fn search(&self, query: &str) -> SearchCommand;

    /// Search with specific engine
    async fn search_with_engine(
        &self,
        query: &str,
        engine: SearchEngineType,
    ) -> Result<Response<ResponseItem>, SearchError>;

    /// Get the default engine type
    fn default_engine(&self) -> SearchEngineType;
}

#[derive(Clone)]
struct SearchClientInner {
    engines: Vec<Arc<dyn SearchEngine>>,
    default_engine: SearchEngineType,
}

fn default_engine_type() -> SearchEngineType {
    SearchEngineType::Bing
}

/// 搜索客户端
#[derive(Clone)]
pub struct SearchClient {
    inner: Arc<SearchClientInner>,
}

impl SearchClient {
    /// Create a new SearchClient
    pub fn new(engine_client: Arc<EngineClient>) -> Self {
        let mut inner = SearchClientInner {
            engines: Vec::new(),
            default_engine: default_engine_type(),
        };

        Self::register_default_engines(&mut inner, &engine_client);

        SearchClient {
            inner: Arc::new(inner),
        }
    }

    /// Register the built-in scraping-backed engines (bing/baidu/sogou).
    fn register_default_engines(inner: &mut SearchClientInner, engine_client: &Arc<EngineClient>) {
        inner
            .engines
            .push(Arc::new(BingSearchEngine::new(engine_client.clone())) as Arc<dyn SearchEngine>);
        inner
            .engines
            .push(Arc::new(BaiduSearchEngine::new(engine_client.clone())) as Arc<dyn SearchEngine>);
        inner
            .engines
            .push(Arc::new(SogouSearchEngine::new(engine_client.clone())) as Arc<dyn SearchEngine>);
    }

    /// Create a SearchClient with engines assembled from [`SearchSettings`].
    ///
    /// 始终注册内置引擎（bing/baidu/sogou）；API 引擎（exa/tavily）仅在配置了
    /// 非空 `api_key` 时注册：
    ///
    /// - Exa: `search.exa.api_key`（env: `CRAWLRS__SEARCH__EXA__API_KEY`）
    /// - Tavily: `search.tavily.api_key`（env: `CRAWLRS__SEARCH__TAVILY__API_KEY`）
    ///
    /// 未配置凭据不是错误——跳过注册并输出 info 日志；此后显式请求
    /// `engine="exa"` / `engine="tavily"` 会得到 `SearchError::NoEngineAvailable`。
    pub fn with_configured_engines(
        engine_client: Arc<EngineClient>,
        http_client: Arc<reqwest::Client>,
        settings: &SearchSettings,
    ) -> Self {
        let mut inner = SearchClientInner {
            engines: Vec::new(),
            default_engine: default_engine_type(),
        };

        Self::register_default_engines(&mut inner, &engine_client);

        match settings
            .exa
            .api_key()
            .filter(|key| !key.trim().is_empty())
        {
            Some(api_key) => {
                let mut config = ExaConfig {
                    api_key: api_key.to_string(),
                    ..ExaConfig::default()
                };
                if let Some(base_url) = settings
                    .exa
                    .base_url()
                    .filter(|url| !url.trim().is_empty())
                {
                    config.endpoint = base_url.to_string();
                }
                inner.engines.push(Arc::new(ExaSearchEngine::new(
                    http_client.as_ref().clone(),
                    config,
                )) as Arc<dyn SearchEngine>);
                log::info!("Exa engine registered (engine=\"exa\")");
            }
            None => log::info!(
                "Exa engine skipped: api_key not configured (set CRAWLRS__SEARCH__EXA__API_KEY to enable engine=\"exa\")"
            ),
        }

        match settings
            .tavily
            .api_key()
            .filter(|key| !key.trim().is_empty())
        {
            Some(api_key) => {
                let mut config = TavilyConfig {
                    api_key: api_key.to_string(),
                    ..TavilyConfig::default()
                };
                if let Some(base_url) = settings
                    .tavily
                    .base_url()
                    .filter(|url| !url.trim().is_empty())
                {
                    config.endpoint = base_url.to_string();
                }
                inner.engines.push(Arc::new(TavilySearchEngine::new(
                    http_client.as_ref().clone(),
                    config,
                )) as Arc<dyn SearchEngine>);
                log::info!("Tavily engine registered (engine=\"tavily\")");
            }
            None => log::info!(
                "Tavily engine skipped: api_key not configured (set CRAWLRS__SEARCH__TAVILY__API_KEY to enable engine=\"tavily\")"
            ),
        }

        SearchClient {
            inner: Arc::new(inner),
        }
    }

    pub fn register_engine(&mut self, engine: Arc<dyn SearchEngine>) {
        Arc::make_mut(&mut self.inner).engines.push(engine);
    }

    /// Create a SearchClient with a custom set of engines.
    ///
    /// Enables test injection of mock engines and production scenarios that
    /// need a curated engine subset. The caller is responsible for providing
    /// at least one engine; an empty slice yields a client whose `search()`
    /// always fails with `SearchError::NoEngineAvailable`.
    pub fn new_with_engines(
        engines: Vec<Arc<dyn SearchEngine>>,
        default_engine: SearchEngineType,
    ) -> Self {
        let inner = SearchClientInner {
            engines,
            default_engine,
        };
        SearchClient {
            inner: Arc::new(inner),
        }
    }

    pub fn search(&self, query: &str) -> SearchCommand {
        SearchCommand::new(self.clone(), query)
    }

    pub async fn search_with_engine(
        &self,
        query: &str,
        engine: SearchEngineType,
    ) -> Result<Response<ResponseItem>, SearchError> {
        let request = SearchRequest::new(query).with_engine(engine);
        let eng = self
            .inner
            .engines
            .iter()
            .find(|e| e.engine_type() == engine)
            .ok_or_else(|| SearchError::NoEngineAvailable)?;

        eng.search(&request).await
    }

    pub fn default_engine(&self) -> SearchEngineType {
        self.inner.default_engine
    }
}

#[async_trait::async_trait]
impl SearchClientTrait for SearchClient {
    async fn search(&self, query: &str) -> SearchCommand {
        SearchCommand::new(self.clone(), query)
    }

    async fn search_with_engine(
        &self,
        query: &str,
        engine: SearchEngineType,
    ) -> Result<Response<ResponseItem>, SearchError> {
        let request = SearchRequest::new(query).with_engine(engine);
        let eng = self
            .inner
            .engines
            .iter()
            .find(|e| e.engine_type() == engine)
            .ok_or_else(|| SearchError::NoEngineAvailable)?;

        eng.search(&request).await
    }

    fn default_engine(&self) -> SearchEngineType {
        self.inner.default_engine
    }
}

/// 搜索命令构建器
#[must_use]
pub struct SearchCommand {
    client: SearchClient,
    query: String,
    engine: Option<SearchEngineType>,
    limit: u32,
    offset: u32,
}

impl SearchCommand {
    pub fn new(client: SearchClient, query: &str) -> Self {
        Self {
            client,
            query: query.to_string(),
            engine: None,
            limit: 10,
            offset: 0,
        }
    }

    pub fn bing(mut self) -> Self {
        self.engine = Some(SearchEngineType::Bing);
        self
    }

    pub fn baidu(mut self) -> Self {
        self.engine = Some(SearchEngineType::Baidu);
        self
    }

    pub fn sogou(mut self) -> Self {
        self.engine = Some(SearchEngineType::Sogou);
        self
    }

    pub fn with_engine(mut self, engine: &str) -> Self {
        self.engine = SearchEngineType::from_name(engine);
        self
    }

    pub fn limit(mut self, n: u32) -> Self {
        self.limit = n;
        self
    }

    pub fn offset(mut self, n: u32) -> Self {
        self.offset = n;
        self
    }

    pub async fn execute(&self) -> Result<Response<ResponseItem>, SearchError> {
        let engine = self.engine.unwrap_or(self.client.default_engine());
        let request = SearchRequest {
            query: self.query.clone(),
            engine: Some(engine),
            limit: self.limit,
            offset: self.offset,
            lang: None,
            country: None,
        };

        let eng = self
            .client
            .inner
            .engines
            .iter()
            .find(|e| e.engine_type() == engine)
            .ok_or_else(|| SearchError::NoEngineAvailable)?;

        eng.search(&request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a SearchClient with all engines registered (for testing)
    fn create_test_search_client() -> SearchClient {
        use crate::engines::engine_client::EngineClient;
        let engine_client = Arc::new(EngineClient::new());
        SearchClient::new(engine_client)
    }

    #[tokio::test]
    async fn test_search_command_builder() {
        let client = create_test_search_client();
        let cmd = client.search("test query").bing().limit(5);
        assert_eq!(cmd.query, "test query");
        assert_eq!(cmd.engine, Some(SearchEngineType::Bing));
        assert_eq!(cmd.limit, 5);
    }

    #[tokio::test]
    async fn test_search_request_builder() {
        let req = SearchRequest::new("hello")
            .with_engine(SearchEngineType::Bing)
            .with_limit(20)
            .with_offset(10);

        assert_eq!(req.query, "hello");
        assert_eq!(req.engine, Some(SearchEngineType::Bing));
        assert_eq!(req.limit, 20);
        assert_eq!(req.offset, 10);
    }

    #[tokio::test]
    async fn test_all_engines_registered() {
        let client = create_test_search_client();
        assert_eq!(client.inner.engines.len(), 3);
    }

    // ========== Mock SearchEngine for client tests ==========

    use crate::search::types::EngineHealth;

    struct MockClientEngine {
        name: &'static str,
        engine_type: SearchEngineType,
        items: Vec<ResponseItem>,
    }

    impl MockClientEngine {
        fn new(
            name: &'static str,
            engine_type: SearchEngineType,
            items: Vec<ResponseItem>,
        ) -> Self {
            Self {
                name,
                engine_type,
                items,
            }
        }
    }

    #[async_trait::async_trait]
    impl SearchEngine for MockClientEngine {
        fn name(&self) -> &'static str {
            self.name
        }

        fn engine_type(&self) -> SearchEngineType {
            self.engine_type
        }

        fn health(&self) -> EngineHealth {
            EngineHealth::Healthy
        }

        async fn search(
            &self,
            _request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            Ok(Response {
                items: self.items.clone(),
                total_results: Some(self.items.len() as u64),
                engine: self.engine_type,
            })
        }
    }

    fn make_mock_items(engine_type: SearchEngineType, count: usize) -> Vec<ResponseItem> {
        (0..count)
            .map(|i| ResponseItem {
                title: format!("Result {}", i + 1),
                url: format!("https://example.com/{}", i),
                description: format!("Description {}", i + 1),
                engine: engine_type,
            })
            .collect()
    }

    // ========== new_with_engines tests ==========

    #[test]
    fn test_new_with_engines_creates_client_with_custom_engines() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![
            Arc::new(MockClientEngine::new(
                "bing",
                SearchEngineType::Bing,
                make_mock_items(SearchEngineType::Bing, 2),
            )),
            Arc::new(MockClientEngine::new(
                "baidu",
                SearchEngineType::Baidu,
                make_mock_items(SearchEngineType::Baidu, 1),
            )),
        ];
        let client = SearchClient::new_with_engines(engines, SearchEngineType::Bing);
        assert_eq!(client.inner.engines.len(), 2);
        assert_eq!(client.default_engine(), SearchEngineType::Bing);
    }

    #[test]
    fn test_new_with_engines_empty_engines() {
        let client = SearchClient::new_with_engines(vec![], SearchEngineType::Bing);
        assert_eq!(client.inner.engines.len(), 0);
        assert_eq!(client.default_engine(), SearchEngineType::Bing);
    }

    // ========== register_engine tests ==========

    #[test]
    fn test_register_engine_adds_to_client() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![Arc::new(MockClientEngine::new(
            "bing",
            SearchEngineType::Bing,
            make_mock_items(SearchEngineType::Bing, 1),
        ))];
        let mut client = SearchClient::new_with_engines(engines, SearchEngineType::Bing);
        assert_eq!(client.inner.engines.len(), 1);

        client.register_engine(Arc::new(MockClientEngine::new(
            "bing",
            SearchEngineType::Bing,
            make_mock_items(SearchEngineType::Bing, 1),
        )));
        assert_eq!(client.inner.engines.len(), 2);
    }

    // ========== with_configured_engines tests ==========

    fn build_configured_client(settings: &SearchSettings) -> SearchClient {
        let engine_client = Arc::new(EngineClient::new());
        let http_client = Arc::new(reqwest::Client::new());
        SearchClient::with_configured_engines(engine_client, http_client, settings)
    }

    fn registered_engine_types(client: &SearchClient) -> Vec<SearchEngineType> {
        client
            .inner
            .engines
            .iter()
            .map(|e| e.engine_type())
            .collect()
    }

    #[test]
    fn test_with_configured_engines_without_api_keys_skips_exa_and_tavily() {
        let client = build_configured_client(&SearchSettings::default());
        let types = registered_engine_types(&client);

        assert_eq!(
            client.inner.engines.len(),
            3,
            "only bing/baidu/sogou should be registered without api keys"
        );
        assert!(
            !types.contains(&SearchEngineType::Exa),
            "exa should not be registered without api_key"
        );
        assert!(
            !types.contains(&SearchEngineType::Tavily),
            "tavily should not be registered without api_key"
        );
    }

    #[test]
    fn test_with_configured_engines_with_exa_api_key_registers_exa() {
        let mut settings = SearchSettings::default();
        settings.exa.api_key = Some("test-exa-key".to_string());

        let client = build_configured_client(&settings);
        let types = registered_engine_types(&client);

        assert_eq!(client.inner.engines.len(), 4);
        assert!(
            types.contains(&SearchEngineType::Exa),
            "exa should be registered when api_key is configured"
        );
        // SearchCommand::with_engine("exa") 依赖 from_name 解析路径
        assert_eq!(
            SearchEngineType::from_name("exa"),
            Some(SearchEngineType::Exa)
        );
    }

    #[test]
    fn test_with_configured_engines_with_tavily_api_key_registers_tavily() {
        let mut settings = SearchSettings::default();
        settings.tavily.api_key = Some("test-tavily-key".to_string());

        let client = build_configured_client(&settings);
        let types = registered_engine_types(&client);

        assert_eq!(client.inner.engines.len(), 4);
        assert!(
            types.contains(&SearchEngineType::Tavily),
            "tavily should be registered when api_key is configured"
        );
        assert_eq!(
            SearchEngineType::from_name("tavily"),
            Some(SearchEngineType::Tavily)
        );
    }

    #[test]
    fn test_with_configured_engines_with_both_api_keys_registers_all() {
        let mut settings = SearchSettings::default();
        settings.exa.api_key = Some("test-exa-key".to_string());
        settings.tavily.api_key = Some("test-tavily-key".to_string());

        let client = build_configured_client(&settings);
        let types = registered_engine_types(&client);

        assert_eq!(client.inner.engines.len(), 5);
        assert!(types.contains(&SearchEngineType::Exa));
        assert!(types.contains(&SearchEngineType::Tavily));
    }

    #[test]
    fn test_with_configured_engines_empty_api_key_string_skips_registration() {
        let mut settings = SearchSettings::default();
        settings.exa.api_key = Some(String::new());
        settings.tavily.api_key = Some("   ".to_string());

        let client = build_configured_client(&settings);

        assert_eq!(
            client.inner.engines.len(),
            3,
            "blank api_key should be treated as not configured"
        );
        assert!(!registered_engine_types(&client).contains(&SearchEngineType::Exa));
        assert!(!registered_engine_types(&client).contains(&SearchEngineType::Tavily));
    }

    #[tokio::test]
    async fn test_with_engine_exa_unregistered_returns_no_engine_available() {
        let client = build_configured_client(&SearchSettings::default());

        // with_engine("exa") 解析为 Some(Exa)，但引擎未注册 → 显性 NoEngineAvailable
        let result = client.search("test").with_engine("exa").execute().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchError::NoEngineAvailable => {}
            other => panic!("Expected NoEngineAvailable, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_search_with_engine_tavily_unregistered_returns_no_engine_available() {
        let client = build_configured_client(&SearchSettings::default());

        let result = client
            .search_with_engine("test", SearchEngineType::Tavily)
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchError::NoEngineAvailable => {}
            other => panic!("Expected NoEngineAvailable, got {:?}", other),
        }
    }

    // ========== search_with_engine tests ==========

    #[tokio::test]
    async fn test_search_with_engine_success() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![Arc::new(MockClientEngine::new(
            "bing",
            SearchEngineType::Bing,
            make_mock_items(SearchEngineType::Bing, 3),
        ))];
        let client = SearchClient::new_with_engines(engines, SearchEngineType::Bing);

        let response = client
            .search_with_engine("test", SearchEngineType::Bing)
            .await
            .expect("search_with_engine should succeed");
        assert_eq!(response.items.len(), 3);
        assert_eq!(response.engine, SearchEngineType::Bing);
        assert_eq!(response.total_results, Some(3));
    }

    #[tokio::test]
    async fn test_search_with_engine_no_match_returns_no_engine_available() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![Arc::new(MockClientEngine::new(
            "bing",
            SearchEngineType::Bing,
            make_mock_items(SearchEngineType::Bing, 1),
        ))];
        let client = SearchClient::new_with_engines(engines, SearchEngineType::Bing);

        let result = client
            .search_with_engine("test", SearchEngineType::Baidu)
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchError::NoEngineAvailable => {}
            other => panic!("Expected NoEngineAvailable, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_search_with_engine_empty_client_returns_no_engine_available() {
        let client = SearchClient::new_with_engines(vec![], SearchEngineType::Bing);
        let result = client
            .search_with_engine("test", SearchEngineType::Bing)
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchError::NoEngineAvailable => {}
            other => panic!("Expected NoEngineAvailable, got {:?}", other),
        }
    }

    // ========== default_engine tests ==========

    #[test]
    fn test_default_engine_returns_configured_type() {
        let client = SearchClient::new_with_engines(vec![], SearchEngineType::Baidu);
        assert_eq!(client.default_engine(), SearchEngineType::Baidu);
    }

    #[test]
    fn test_default_engine_from_new_client() {
        let client = create_test_search_client();
        assert_eq!(client.default_engine(), SearchEngineType::Bing);
    }

    // ========== SearchCommand builder tests ==========

    #[tokio::test]
    async fn test_search_command_bing_builder() {
        let client = create_test_search_client();
        let cmd = client.search("query").bing();
        assert_eq!(cmd.engine, Some(SearchEngineType::Bing));
    }

    #[tokio::test]
    async fn test_search_command_baidu_builder() {
        let client = create_test_search_client();
        let cmd = client.search("query").baidu();
        assert_eq!(cmd.engine, Some(SearchEngineType::Baidu));
    }

    #[tokio::test]
    async fn test_search_command_sogou_builder() {
        let client = create_test_search_client();
        let cmd = client.search("query").sogou();
        assert_eq!(cmd.engine, Some(SearchEngineType::Sogou));
    }

    #[tokio::test]
    async fn test_search_command_with_engine_string_valid() {
        let client = create_test_search_client();
        let cmd = client.search("query").with_engine("bing");
        assert_eq!(cmd.engine, Some(SearchEngineType::Bing));
    }

    #[tokio::test]
    async fn test_search_command_with_engine_string_invalid() {
        let client = create_test_search_client();
        let cmd = client.search("query").with_engine("yahoo");
        assert_eq!(cmd.engine, None);
    }

    #[tokio::test]
    async fn test_search_command_offset_builder() {
        let client = create_test_search_client();
        let cmd = client.search("query").offset(20);
        assert_eq!(cmd.offset, 20);
    }

    #[tokio::test]
    async fn test_search_command_chained_builders() {
        let client = create_test_search_client();
        let cmd = client.search("rust").bing().limit(15).offset(5);
        assert_eq!(cmd.query, "rust");
        assert_eq!(cmd.engine, Some(SearchEngineType::Bing));
        assert_eq!(cmd.limit, 15);
        assert_eq!(cmd.offset, 5);
    }

    // ========== SearchCommand::execute tests ==========

    #[tokio::test]
    async fn test_execute_success_with_matching_engine() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![Arc::new(MockClientEngine::new(
            "bing",
            SearchEngineType::Bing,
            make_mock_items(SearchEngineType::Bing, 2),
        ))];
        let client = SearchClient::new_with_engines(engines, SearchEngineType::Bing);

        let response = client
            .search("test")
            .bing()
            .execute()
            .await
            .expect("execute should succeed with matching engine");
        assert_eq!(response.items.len(), 2);
        assert_eq!(response.engine, SearchEngineType::Bing);
    }

    #[tokio::test]
    async fn test_execute_no_matching_engine_returns_error() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![Arc::new(MockClientEngine::new(
            "bing",
            SearchEngineType::Bing,
            make_mock_items(SearchEngineType::Bing, 1),
        ))];
        let client = SearchClient::new_with_engines(engines, SearchEngineType::Bing);

        let result = client.search("test").baidu().execute().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchError::NoEngineAvailable => {}
            other => panic!("Expected NoEngineAvailable, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_execute_uses_default_engine_when_none_specified() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![Arc::new(MockClientEngine::new(
            "bing",
            SearchEngineType::Bing,
            make_mock_items(SearchEngineType::Bing, 1),
        ))];
        let client = SearchClient::new_with_engines(engines, SearchEngineType::Bing);

        // No engine specified, should use default (Bing)
        let response = client
            .search("test")
            .execute()
            .await
            .expect("execute should use default engine");
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.engine, SearchEngineType::Bing);
    }

    #[tokio::test]
    async fn test_execute_with_empty_client_returns_no_engine_available() {
        let client = SearchClient::new_with_engines(vec![], SearchEngineType::Bing);
        let result = client.search("test").execute().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchError::NoEngineAvailable => {}
            other => panic!("Expected NoEngineAvailable, got {:?}", other),
        }
    }

    // ========== SearchClientTrait impl tests ==========

    #[tokio::test]
    async fn test_trait_impl_search_returns_command() {
        let client = create_test_search_client();
        let cmd = SearchClientTrait::search(&client, "query").await;
        assert_eq!(cmd.query, "query");
    }

    #[tokio::test]
    async fn test_trait_impl_search_with_engine_success() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![Arc::new(MockClientEngine::new(
            "bing",
            SearchEngineType::Bing,
            make_mock_items(SearchEngineType::Bing, 1),
        ))];
        let client = SearchClient::new_with_engines(engines, SearchEngineType::Bing);

        let response =
            SearchClientTrait::search_with_engine(&client, "test", SearchEngineType::Bing)
                .await
                .expect("trait search_with_engine should succeed");
        assert_eq!(response.items.len(), 1);
    }

    #[tokio::test]
    async fn test_trait_impl_search_with_engine_no_match() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![Arc::new(MockClientEngine::new(
            "bing",
            SearchEngineType::Bing,
            make_mock_items(SearchEngineType::Bing, 1),
        ))];
        let client = SearchClient::new_with_engines(engines, SearchEngineType::Bing);

        let result =
            SearchClientTrait::search_with_engine(&client, "test", SearchEngineType::Baidu).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_trait_impl_default_engine() {
        let client = SearchClient::new_with_engines(vec![], SearchEngineType::Sogou);
        assert_eq!(
            SearchClientTrait::default_engine(&client),
            SearchEngineType::Sogou
        );
    }
}

// ========== Shared Utilities ==========

// Shared utilities for search engine clients
// Provides common utilities to eliminate code duplication across search engine implementations.

use url::Url;

/// Encode a search query for use in URLs
pub fn encode_search_query(query: &str) -> String {
    urlencoding::encode(query).to_string()
}

/// Build a simple paginated search URL
pub fn build_search_url(
    base_url: &str,
    query_param: &str,
    query: &str,
    page: u32,
    results_per_page: u32,
) -> String {
    let encoded_query = encode_search_query(query);
    let offset = (page - 1) * results_per_page;
    format!(
        "{}?{}={}&start={}",
        base_url, query_param, encoded_query, offset
    )
}

/// Validate and normalize a URL from search results
pub fn validate_result_url(url: &str, allowed_schemes: &[&str]) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    match Url::parse(url) {
        Ok(parsed) => {
            if allowed_schemes.contains(&parsed.scheme()) {
                Some(url.to_string())
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod shared_tests {
    use super::*;

    #[test]
    fn test_encode_search_query() {
        let encoded = encode_search_query("hello world");
        assert!(encoded.contains("hello%20world"));
    }

    #[test]
    fn test_build_search_url() {
        let url = build_search_url("https://example.com/search", "q", "test", 2, 10);
        assert!(url.contains("q=test"));
        assert!(url.contains("start=10"));
    }

    #[test]
    fn test_validate_result_url_valid() {
        let result = validate_result_url("https://example.com", &["http", "https"]);
        assert_eq!(result, Some("https://example.com".to_string()));
    }

    #[test]
    fn test_validate_result_url_invalid_scheme() {
        let result = validate_result_url("ftp://example.com", &["http", "https"]);
        assert!(result.is_none());
    }

    #[test]
    fn test_validate_result_url_empty_returns_none() {
        let result = validate_result_url("", &["http", "https"]);
        assert!(result.is_none(), "empty URL should return None");
    }

    #[test]
    fn test_validate_result_url_no_scheme_returns_none() {
        let result = validate_result_url("example.com", &["http", "https"]);
        assert!(
            result.is_none(),
            "URL without http/https prefix should return None"
        );
    }

    #[test]
    fn test_validate_result_url_plain_string_returns_none() {
        let result = validate_result_url("not-a-url", &["http", "https"]);
        assert!(result.is_none(), "plain string should return None");
    }

    #[test]
    fn test_validate_result_url_http_with_only_https_allowed() {
        let result = validate_result_url("http://example.com", &["https"]);
        assert!(
            result.is_none(),
            "http URL with only https allowed should return None"
        );
    }

    #[test]
    fn test_validate_result_url_https_with_only_http_allowed() {
        let result = validate_result_url("https://example.com", &["http"]);
        assert!(
            result.is_none(),
            "https URL with only http allowed should return None"
        );
    }

    #[test]
    fn test_validate_result_url_with_path() {
        let result = validate_result_url("https://example.com/path/to/page", &["http", "https"]);
        assert_eq!(result, Some("https://example.com/path/to/page".to_string()));
    }

    #[test]
    fn test_validate_result_url_http_no_host_returns_none() {
        // Cover line 673: "http://" passes the starts_with check but
        // Url::parse fails with EmptyHost, triggering the Err(_) => None branch.
        let result = validate_result_url("http://", &["http", "https"]);
        assert_eq!(result, None);
    }
}
