// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(deprecated)]

use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::EngineClient;
use crate::engines::traits::ScraperEngine;
use std::sync::Arc;

use once_cell::sync::Lazy;

use super::{
    engine_trait::{SearchEngine, SearchRequest},
    error::SearchError,
    response::{Response, ResponseItem},
    types::SearchEngineType,
};

pub mod baidu;
pub mod bing;
pub mod google;
pub mod html_parser;
pub mod http_client;
pub mod shared_utils;
pub mod sogou;

pub use baidu::BaiduSearchEngine;
pub use bing::BingSearchEngine;
pub use google::GoogleSearchEngine;
pub use http_client::SHARED_HTTP_CLIENT;
pub use shared_utils::{parse_first_selector, safe_parse_selector};
pub use sogou::SogouSearchEngine;

#[derive(Clone)]
struct SearchClientInner {
    engines: Vec<Arc<dyn SearchEngine>>,
    default_engine: SearchEngineType,
}

fn default_engine_type() -> SearchEngineType {
    SearchEngineType::Google
}

/// 搜索客户端单例
#[derive(Clone)]
pub struct SearchClient {
    inner: Arc<SearchClientInner>,
}

impl SearchClient {
    pub fn global() -> &'static Self {
        static INSTANCE: Lazy<SearchClient> = Lazy::new(|| {
            let mut inner = SearchClientInner {
                engines: Vec::new(),
                default_engine: default_engine_type(),
            };

            // 注册所有支持的搜索引擎（真实实现）
            // 默认注册所有引擎

            // Create a default EngineClient with ReqwestEngine for all engines
            let reqwest_engine = Arc::new(ReqwestEngine::new());
            let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine];
            let engine_client = Arc::new(EngineClient::with_engines(engines.clone()));

            inner
                .engines
                .push(Arc::new(GoogleSearchEngine::new(engine_client.clone())) as Arc<dyn SearchEngine>);
            inner
                .engines
                .push(Arc::new(BingSearchEngine::new(engine_client.clone())) as Arc<dyn SearchEngine>);
            inner
                .engines
                .push(Arc::new(BaiduSearchEngine::new(engine_client.clone())) as Arc<dyn SearchEngine>);
            inner
                .engines
                .push(Arc::new(SogouSearchEngine::new(engine_client)) as Arc<dyn SearchEngine>);

            SearchClient {
                inner: Arc::new(inner),
            }
        });
        &INSTANCE
    }

    pub fn register_engine(&mut self, engine: Arc<dyn SearchEngine>) {
        Arc::make_mut(&mut self.inner).engines.push(engine);
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
    fn new(client: SearchClient, query: &str) -> Self {
        Self {
            client,
            query: query.to_string(),
            engine: None,
            limit: 10,
            offset: 0,
        }
    }

    pub fn google(mut self) -> Self {
        self.engine = Some(SearchEngineType::Google);
        self
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

    #[tokio::test]
    async fn test_search_command_builder() {
        let client = SearchClient::global().clone();
        let cmd = client.search("test query").google().limit(5);
        assert_eq!(cmd.query, "test query");
        assert_eq!(cmd.engine, Some(SearchEngineType::Google));
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
        let client = SearchClient::global();
        assert_eq!(client.inner.engines.len(), 4);
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
}
