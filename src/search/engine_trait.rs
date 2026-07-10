// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;

use super::response::ResponseItem;
use super::types::EngineHealth;
use super::{error::SearchError, response::Response, types::SearchEngineType};

/// 搜索请求
#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub query: String,
    pub engine: Option<SearchEngineType>,
    pub limit: u32,
    pub offset: u32,
    pub lang: Option<String>,
    pub country: Option<String>,
}

impl SearchRequest {
    pub fn new(query: &str) -> Self {
        Self {
            query: query.to_string(),
            engine: None,
            limit: 10,
            offset: 0,
            lang: None,
            country: None,
        }
    }

    pub fn with_engine(mut self, engine: SearchEngineType) -> Self {
        self.engine = Some(engine);
        self
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_offset(mut self, offset: u32) -> Self {
        self.offset = offset;
        self
    }

    pub fn with_lang(mut self, lang: &str) -> Self {
        self.lang = Some(lang.to_string());
        self
    }

    pub fn with_country(mut self, country: &str) -> Self {
        self.country = Some(country.to_string());
        self
    }
}

impl Default for SearchRequest {
    fn default() -> Self {
        Self::new("")
    }
}

/// 搜索引擎 trait
#[async_trait]
pub trait SearchEngine: Send + Sync {
    fn name(&self) -> &'static str;
    fn engine_type(&self) -> SearchEngineType;
    fn health(&self) -> EngineHealth;
    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError>;

    /// Search with a specific engine (if engine is None, search all engines)
    /// Default implementation: searches without specific engine
    async fn search_with_engine(
        &self,
        query: &str,
        limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
        engine: Option<&str>,
    ) -> Result<Vec<ResponseItem>, SearchError> {
        let mut request = SearchRequest::new(query).with_limit(limit);
        if let Some(engine_name) = engine {
            if let Some(engine_type) = SearchEngineType::from_name(engine_name) {
                request = request.with_engine(engine_type);
            }
        }
        let response = self.search(&request).await?;
        Ok(response.items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== SearchRequest::new tests ==========

    #[test]
    fn test_search_request_new_sets_query() {
        let req = SearchRequest::new("rust web scraping");
        assert_eq!(req.query, "rust web scraping");
    }

    #[test]
    fn test_search_request_new_defaults() {
        let req = SearchRequest::new("test");
        assert_eq!(req.limit, 10);
        assert_eq!(req.offset, 0);
        assert!(req.engine.is_none());
        assert!(req.lang.is_none());
        assert!(req.country.is_none());
    }

    #[test]
    fn test_search_request_new_empty_query() {
        let req = SearchRequest::new("");
        assert_eq!(req.query, "");
    }

    // ========== SearchRequest::default tests ==========

    #[test]
    fn test_search_request_default_empty_query() {
        let req = SearchRequest::default();
        assert_eq!(req.query, "");
    }

    #[test]
    fn test_search_request_default_limit_10() {
        let req = SearchRequest::default();
        assert_eq!(req.limit, 10);
    }

    #[test]
    fn test_search_request_default_offset_0() {
        let req = SearchRequest::default();
        assert_eq!(req.offset, 0);
    }

    // ========== SearchRequest builder tests ==========

    #[test]
    fn test_search_request_with_engine() {
        let req = SearchRequest::new("test").with_engine(SearchEngineType::Google);
        assert_eq!(req.engine, Some(SearchEngineType::Google));
    }

    #[test]
    fn test_search_request_with_engine_bing() {
        let req = SearchRequest::new("test").with_engine(SearchEngineType::Bing);
        assert_eq!(req.engine, Some(SearchEngineType::Bing));
    }

    #[test]
    fn test_search_request_with_limit() {
        let req = SearchRequest::new("test").with_limit(50);
        assert_eq!(req.limit, 50);
    }

    #[test]
    fn test_search_request_with_limit_zero() {
        let req = SearchRequest::new("test").with_limit(0);
        assert_eq!(req.limit, 0);
    }

    #[test]
    fn test_search_request_with_offset() {
        let req = SearchRequest::new("test").with_offset(20);
        assert_eq!(req.offset, 20);
    }

    #[test]
    fn test_search_request_with_offset_zero() {
        let req = SearchRequest::new("test").with_offset(0);
        assert_eq!(req.offset, 0);
    }

    #[test]
    fn test_search_request_with_lang() {
        let req = SearchRequest::new("test").with_lang("en");
        assert_eq!(req.lang, Some("en".to_string()));
    }

    #[test]
    fn test_search_request_with_lang_zh() {
        let req = SearchRequest::new("test").with_lang("zh-CN");
        assert_eq!(req.lang, Some("zh-CN".to_string()));
    }

    #[test]
    fn test_search_request_with_country() {
        let req = SearchRequest::new("test").with_country("US");
        assert_eq!(req.country, Some("US".to_string()));
    }

    #[test]
    fn test_search_request_with_country_cn() {
        let req = SearchRequest::new("test").with_country("CN");
        assert_eq!(req.country, Some("CN".to_string()));
    }

    // ========== SearchRequest chained builder tests ==========

    #[test]
    fn test_search_request_chained_builders() {
        let req = SearchRequest::new("rust")
            .with_engine(SearchEngineType::Baidu)
            .with_limit(20)
            .with_offset(5)
            .with_lang("zh")
            .with_country("CN");

        assert_eq!(req.query, "rust");
        assert_eq!(req.engine, Some(SearchEngineType::Baidu));
        assert_eq!(req.limit, 20);
        assert_eq!(req.offset, 5);
        assert_eq!(req.lang, Some("zh".to_string()));
        assert_eq!(req.country, Some("CN".to_string()));
    }

    #[test]
    fn test_search_request_clone() {
        let req = SearchRequest::new("test").with_limit(5);
        let cloned = req.clone();
        assert_eq!(cloned.query, req.query);
        assert_eq!(cloned.limit, req.limit);
    }

    #[test]
    fn test_search_request_debug() {
        let req = SearchRequest::new("test");
        let dbg = format!("{:?}", req);
        assert!(dbg.contains("SearchRequest"));
        assert!(dbg.contains("test"));
    }

    // ========== Mock SearchEngine implementation tests ==========

    struct MockSearchEngine;

    #[async_trait]
    impl SearchEngine for MockSearchEngine {
        fn name(&self) -> &'static str {
            "MockEngine"
        }

        fn engine_type(&self) -> SearchEngineType {
            SearchEngineType::Google
        }

        fn health(&self) -> EngineHealth {
            EngineHealth::Healthy
        }

        async fn search(
            &self,
            request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            let items = (0..request.limit as usize)
                .map(|i| ResponseItem {
                    title: format!("Result {}", i + 1),
                    url: format!("https://example.com/{}", i + 1),
                    description: format!("Description for result {}", i + 1),
                    engine: SearchEngineType::Google,
                })
                .collect();
            Ok(Response {
                items,
                total_results: Some(request.limit as u64),
                engine: SearchEngineType::Google,
            })
        }
    }

    #[test]
    fn test_mock_engine_name() {
        let engine = MockSearchEngine;
        assert_eq!(engine.name(), "MockEngine");
    }

    #[test]
    fn test_mock_engine_type() {
        let engine = MockSearchEngine;
        assert_eq!(engine.engine_type(), SearchEngineType::Google);
    }

    #[test]
    fn test_mock_engine_health() {
        let engine = MockSearchEngine;
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    #[tokio::test]
    async fn test_mock_engine_search_returns_results() {
        let engine = MockSearchEngine;
        let req = SearchRequest::new("test query").with_limit(5);
        let response = engine.search(&req).await.unwrap();
        assert_eq!(response.items.len(), 5);
        assert_eq!(response.engine, SearchEngineType::Google);
        assert_eq!(response.total_results, Some(5));
    }

    #[tokio::test]
    async fn test_mock_engine_search_default_limit() {
        let engine = MockSearchEngine;
        let req = SearchRequest::new("test");
        let response = engine.search(&req).await.unwrap();
        assert_eq!(response.items.len(), 10);
    }

    #[tokio::test]
    async fn test_search_with_engine_no_engine_specified() {
        let engine = MockSearchEngine;
        let items = engine
            .search_with_engine("test", 3, None, None, None)
            .await
            .unwrap();
        assert_eq!(items.len(), 3);
    }

    #[tokio::test]
    async fn test_search_with_engine_named_engine() {
        let engine = MockSearchEngine;
        let items = engine
            .search_with_engine("test", 2, None, None, Some("Google"))
            .await
            .unwrap();
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn test_search_with_engine_invalid_name_still_searches() {
        let engine = MockSearchEngine;
        // Invalid engine name should not cause an error - it just won't set the engine
        let items = engine
            .search_with_engine("test", 4, None, None, Some("invalid_engine"))
            .await
            .unwrap();
        assert_eq!(items.len(), 4);
    }

    #[tokio::test]
    async fn test_search_with_engine_with_lang_and_country() {
        let engine = MockSearchEngine;
        let items = engine
            .search_with_engine("test", 5, Some("en"), Some("US"), Some("google"))
            .await
            .unwrap();
        assert_eq!(items.len(), 5);
    }

    #[tokio::test]
    async fn test_search_with_engine_zero_limit() {
        let engine = MockSearchEngine;
        let items = engine
            .search_with_engine("test", 0, None, None, None)
            .await
            .unwrap();
        assert!(items.is_empty());
    }
}
