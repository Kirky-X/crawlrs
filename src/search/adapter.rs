// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::search::engine::{
    SearchEngine as DomainSearchEngine, SearchError as DomainSearchError,
};

use super::client::SearchClient;
use super::engine_trait::{SearchEngine as TraitSearchEngine, SearchRequest};

/// 域适配器 - 将新的 SearchEngine trait 适配到现有的 domain::search::engine::SearchEngine
pub struct SearchEngineAdapter {
    client: SearchClient,
}

impl SearchEngineAdapter {
    pub fn new(client: SearchClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl DomainSearchEngine for SearchEngineAdapter {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
    ) -> Result<Vec<crate::domain::models::search_result::SearchResult>, DomainSearchError> {
        let result = self
            .client
            .search(query)
            .limit(limit)
            .execute()
            .await
            .map_err(|e| DomainSearchError::EngineError(e.to_string()))?;

        let search_results = result
            .items
            .into_iter()
            .map(|item| crate::domain::models::search_result::SearchResult {
                title: item.title,
                url: item.url,
                description: Some(item.description),
                engine: format!("{:?}", item.engine),
                score: 0.0,
                published_time: None,
            })
            .collect();

        Ok(search_results)
    }

    fn name(&self) -> &'static str {
        "SearchEngineAdapter"
    }
}

/// 通用适配器 - 将 SearchEngine trait (src/search) 适配到 domain::search::engine::SearchEngine
pub struct GenericSearchEngineAdapter {
    engine: Arc<dyn TraitSearchEngine>,
}

impl GenericSearchEngineAdapter {
    pub fn new(engine: Arc<dyn TraitSearchEngine>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl DomainSearchEngine for GenericSearchEngineAdapter {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
    ) -> Result<Vec<crate::domain::models::search_result::SearchResult>, DomainSearchError> {
        let request = SearchRequest::new(query).with_limit(limit);
        let result = self
            .engine
            .search(&request)
            .await
            .map_err(|e| DomainSearchError::EngineError(e.to_string()))?;

        let search_results = result
            .items
            .into_iter()
            .map(|item| crate::domain::models::search_result::SearchResult {
                title: item.title,
                url: item.url,
                description: Some(item.description),
                engine: format!("{:?}", item.engine),
                score: 0.0,
                published_time: None,
            })
            .collect();

        Ok(search_results)
    }

    fn name(&self) -> &'static str {
        self.engine.name()
    }
}

#[cfg(test)]
mod tests {
    // Copyright (c) 2025 Kirky.X
    use super::*;
    use crate::search::engine_trait::SearchRequest;
    use crate::search::response::{Response, ResponseItem};
    use crate::search::types::SearchEngineType;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    // ========== Mock TraitSearchEngine (search::engine_trait::SearchEngine) ==========

    /// Mock implementation of `search::engine_trait::SearchEngine` for testing
    /// `GenericSearchEngineAdapter`. Returns a preconfigured `Response` or an error.
    struct MockTraitSearchEngine {
        engine_name: &'static str,
        engine_type: SearchEngineType,
        items: Vec<ResponseItem>,
        fail: bool,
        last_request: Mutex<Option<SearchRequest>>,
    }

    impl MockTraitSearchEngine {
        fn success(
            engine_name: &'static str,
            engine_type: SearchEngineType,
            items: Vec<ResponseItem>,
        ) -> Self {
            Self {
                engine_name,
                engine_type,
                items,
                fail: false,
                last_request: Mutex::new(None),
            }
        }

        fn failing(engine_name: &'static str, engine_type: SearchEngineType) -> Self {
            Self {
                engine_name,
                engine_type,
                items: Vec::new(),
                fail: true,
                last_request: Mutex::new(None),
            }
        }

        fn last_request(&self) -> Option<SearchRequest> {
            self.last_request.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl TraitSearchEngine for MockTraitSearchEngine {
        fn name(&self) -> &'static str {
            self.engine_name
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
        ) -> Result<Response<ResponseItem>, crate::search::error::SearchError> {
            *self.last_request.lock().unwrap() = Some(request.clone());
            if self.fail {
                return Err(crate::search::error::SearchError::NoEngineAvailable);
            }
            Ok(Response {
                items: self.items.clone(),
                total_results: Some(self.items.len() as u64),
                engine: self.engine_type,
            })
        }
    }

    fn make_response_item(title: &str, url: &str, engine: SearchEngineType) -> ResponseItem {
        ResponseItem {
            title: title.to_string(),
            url: url.to_string(),
            description: format!("desc for {}", title),
            engine,
        }
    }

    // ========== GenericSearchEngineAdapter tests ==========

    #[tokio::test]
    async fn test_generic_adapter_name_delegates_to_engine() {
        let engine = Arc::new(MockTraitSearchEngine::success(
            "MockGoogle",
            SearchEngineType::Google,
            vec![],
        ));
        let adapter = GenericSearchEngineAdapter::new(engine);
        assert_eq!(adapter.name(), "MockGoogle");
    }

    #[tokio::test]
    async fn test_generic_adapter_search_maps_results() {
        let items = vec![
            make_response_item("Result 1", "https://a.com", SearchEngineType::Google),
            make_response_item("Result 2", "https://b.com", SearchEngineType::Google),
        ];
        let engine = Arc::new(MockTraitSearchEngine::success(
            "MockGoogle",
            SearchEngineType::Google,
            items,
        ));
        let adapter = GenericSearchEngineAdapter::new(engine);

        let results = adapter
            .search("rust", 10, None, None)
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 2, "should return both mapped results");
        assert_eq!(results[0].title, "Result 1");
        assert_eq!(results[0].url, "https://a.com");
        assert_eq!(
            results[0].description,
            Some("desc for Result 1".to_string()),
            "description should be mapped from ResponseItem"
        );
        assert_eq!(results[0].score, 0.0, "adapter should set score to 0.0");
        assert_eq!(
            results[0].published_time, None,
            "adapter should set published_time to None"
        );
    }

    #[tokio::test]
    async fn test_generic_adapter_search_engine_field_is_debug_format() {
        let items = vec![make_response_item(
            "R",
            "https://a.com",
            SearchEngineType::Bing,
        )];
        let engine = Arc::new(MockTraitSearchEngine::success(
            "MockBing",
            SearchEngineType::Bing,
            items,
        ));
        let adapter = GenericSearchEngineAdapter::new(engine);

        let results = adapter
            .search("q", 5, None, None)
            .await
            .expect("search should succeed");

        assert_eq!(
            results[0].engine, "Bing",
            "engine field is Debug format of the SearchEngineType"
        );
    }

    #[tokio::test]
    async fn test_generic_adapter_search_propagates_error() {
        let engine = Arc::new(MockTraitSearchEngine::failing(
            "MockFailing",
            SearchEngineType::Google,
        ));
        let adapter = GenericSearchEngineAdapter::new(engine);

        let result = adapter.search("rust", 10, None, None).await;
        assert!(
            result.is_err(),
            "adapter should propagate the underlying search error"
        );
        match result.unwrap_err() {
            DomainSearchError::EngineError(msg) => {
                assert!(
                    msg.contains("没有可用的搜索引擎") || msg.contains("NoEngineAvailable"),
                    "error message should describe the failure, got: {}",
                    msg
                );
            }
            other => panic!("expected EngineError variant, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_generic_adapter_search_forwards_query_and_limit() {
        let engine = Arc::new(MockTraitSearchEngine::success(
            "MockGoogle",
            SearchEngineType::Google,
            vec![make_response_item(
                "T",
                "https://x.com",
                SearchEngineType::Google,
            )],
        ));
        // Keep a typed handle for inspection before the Arc is coerced to a trait object.
        let engine_for_inspection = Arc::clone(&engine);
        let adapter = GenericSearchEngineAdapter::new(engine);

        adapter
            .search("hello world", 7, None, None)
            .await
            .expect("search should succeed");

        let last_req = engine_for_inspection
            .last_request()
            .expect("adapter should have forwarded the request");
        assert_eq!(last_req.query, "hello world");
        assert_eq!(last_req.limit, 7);
    }

    #[tokio::test]
    async fn test_generic_adapter_search_empty_results() {
        let engine = Arc::new(MockTraitSearchEngine::success(
            "MockEmpty",
            SearchEngineType::Google,
            vec![],
        ));
        let adapter = GenericSearchEngineAdapter::new(engine);

        let results = adapter
            .search("nothing", 10, None, None)
            .await
            .expect("search should succeed");

        assert!(
            results.is_empty(),
            "empty response should yield empty results"
        );
    }

    // ========== SearchEngineAdapter tests ==========

    fn make_search_client_with_mock(
        engine_name: &'static str,
        engine_type: SearchEngineType,
        items: Vec<ResponseItem>,
    ) -> SearchClient {
        let mock = MockTraitSearchEngine::success(engine_name, engine_type, items);
        SearchClient::new_with_engines(
            vec![Arc::new(mock) as Arc<dyn TraitSearchEngine>],
            engine_type,
        )
    }

    fn make_failing_search_client(
        engine_name: &'static str,
        engine_type: SearchEngineType,
    ) -> SearchClient {
        let mock = MockTraitSearchEngine::failing(engine_name, engine_type);
        SearchClient::new_with_engines(
            vec![Arc::new(mock) as Arc<dyn TraitSearchEngine>],
            engine_type,
        )
    }

    #[tokio::test]
    async fn test_adapter_name_is_fixed() {
        let client = make_search_client_with_mock("MockGoogle", SearchEngineType::Google, vec![]);
        let adapter = SearchEngineAdapter::new(client);
        assert_eq!(adapter.name(), "SearchEngineAdapter");
    }

    #[tokio::test]
    async fn test_adapter_search_maps_results() {
        let items = vec![
            make_response_item("A", "https://a.com", SearchEngineType::Google),
            make_response_item("B", "https://b.com", SearchEngineType::Google),
        ];
        let client = make_search_client_with_mock("MockGoogle", SearchEngineType::Google, items);
        let adapter = SearchEngineAdapter::new(client);

        let results = adapter
            .search("rust", 5, None, None)
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "A");
        assert_eq!(results[0].url, "https://a.com");
        assert_eq!(results[0].score, 0.0);
        assert_eq!(results[0].published_time, None);
    }

    #[tokio::test]
    async fn test_adapter_search_propagates_error() {
        let client = make_failing_search_client("MockFailing", SearchEngineType::Google);
        let adapter = SearchEngineAdapter::new(client);

        let result = adapter.search("rust", 5, None, None).await;
        assert!(result.is_err(), "adapter should propagate search errors");
        match result.unwrap_err() {
            DomainSearchError::EngineError(_) => {}
            other => panic!("expected EngineError variant, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_adapter_search_empty_results() {
        let client = make_search_client_with_mock("MockGoogle", SearchEngineType::Google, vec![]);
        let adapter = SearchEngineAdapter::new(client);

        let results = adapter
            .search("empty", 10, None, None)
            .await
            .expect("search should succeed");

        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_adapter_search_description_defaults_when_empty() {
        let items = vec![ResponseItem {
            title: "NoDesc".to_string(),
            url: "https://nodesc.com".to_string(),
            description: String::new(),
            engine: SearchEngineType::Google,
        }];
        let client = make_search_client_with_mock("MockGoogle", SearchEngineType::Google, items);
        let adapter = SearchEngineAdapter::new(client);

        let results = adapter
            .search("q", 5, None, None)
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 1);
        // The adapter wraps description in Some(...), even if the source was empty.
        assert_eq!(results[0].description, Some(String::new()));
    }
}
