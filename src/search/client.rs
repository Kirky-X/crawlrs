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

mod baidu;
mod bing;
mod google;
mod sogou;

pub use baidu::BaiduSearchEngine;
pub use bing::BingSearchEngine;
pub use google::GoogleSearchEngine;
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
            inner
                .engines
                .push(Arc::new(GoogleSearchEngine::new()) as Arc<dyn SearchEngine>);
            inner
                .engines
                .push(Arc::new(BingSearchEngine::new()) as Arc<dyn SearchEngine>);
            inner
                .engines
                .push(Arc::new(BaiduSearchEngine::new()) as Arc<dyn SearchEngine>);
            inner
                .engines
                .push(Arc::new(SogouSearchEngine::new()) as Arc<dyn SearchEngine>);

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
