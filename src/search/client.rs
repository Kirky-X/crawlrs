// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use std::sync::Arc;

use once_cell::sync::Lazy;

use super::{
    engine_trait::{SearchEngine, SearchRequest},
    error::SearchError,
    response::{Response, ResponseItem},
    types::{EngineHealth, SearchEngineType},
};

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

            // 注册所有支持的搜索引擎（使用 mock 实现）
            // 实际使用时，应该通过 feature flags 条件编译注册真实引擎
            #[cfg(feature = "search-google")]
            inner
                .engines
                .push(Arc::new(MockSearchEngine::new(SearchEngineType::Google))
                    as Arc<dyn SearchEngine>);

            #[cfg(feature = "search-bing")]
            inner
                .engines
                .push(Arc::new(MockSearchEngine::new(SearchEngineType::Bing))
                    as Arc<dyn SearchEngine>);

            #[cfg(feature = "search-baidu")]
            inner
                .engines
                .push(Arc::new(MockSearchEngine::new(SearchEngineType::Baidu))
                    as Arc<dyn SearchEngine>);

            #[cfg(feature = "search-sogou")]
            inner
                .engines
                .push(Arc::new(MockSearchEngine::new(SearchEngineType::Sogou))
                    as Arc<dyn SearchEngine>);

            // 如果没有配置任何搜索引擎，添加默认的 mock 引擎
            if inner.engines.is_empty() {
                inner
                    .engines
                    .push(Arc::new(MockSearchEngine::new(SearchEngineType::Google))
                        as Arc<dyn SearchEngine>);
            }

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

/// Mock engine for development/testing
struct MockSearchEngine {
    engine_type: SearchEngineType,
}

impl MockSearchEngine {
    fn new(engine_type: SearchEngineType) -> Self {
        Self { engine_type }
    }
}

#[async_trait::async_trait]
impl SearchEngine for MockSearchEngine {
    fn get_name(&self) -> &'static str {
        self.engine_type.name()
    }

    fn engine_type(&self) -> SearchEngineType {
        self.engine_type
    }

    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        Ok(Response {
            items: vec![ResponseItem {
                title: format!("Mock result for {}", request.query),
                url: "https://example.com".to_string(),
                description: "This is a mock search result".to_string(),
                engine: self.engine_type,
            }],
            total_results: Some(1),
            engine: self.engine_type,
        })
    }
}

#[allow(dead_code)]
struct MockEngine;

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
}
