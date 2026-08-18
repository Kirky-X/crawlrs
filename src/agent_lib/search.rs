// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! `search()`：按 provider 检索网页并返回标题/URL/摘要。
//!
//! 复用 `search::SearchClient` + `engines::EngineClient`（ReqwestEngine）构建真实搜索链路；
//! provider → 引擎名映射见 [`SearchProvider`]。不支持的 provider 返回
//! [`AgentLibError::UnsupportedProvider`]。

use std::sync::Arc;

use crate::engines::client::ReqwestEngine;
use crate::engines::engine_client::EngineClient;
use crate::engines::router::{EngineRouter, EngineRouterTrait};
use crate::search::client::SearchClient;
use crate::search::response::ResponseItem;
use crate::search::types::SearchEngineType;
use crate::utils::http_client::create_http_client;

use super::error::AgentLibError;

/// 支持的搜索 provider。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchProvider {
    /// Bing 搜索
    Bing,
    /// Google 搜索（当前 crawlrs 无 Google 引擎实现，映射为 `UnsupportedProvider`）
    Google,
    /// 百度搜索
    Baidu,
    /// 搜狗搜索
    Sogou,
}

/// 单条搜索结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    /// 结果标题
    pub title: String,
    /// 结果 URL
    pub url: String,
    /// 结果摘要
    pub snippet: String,
}

/// 使用默认引擎客户端执行搜索。
///
/// # 参数
///
/// - `provider`: 搜索提供方
/// - `query`: 搜索关键词
/// - `limit`: 返回结果条数上限
///
/// # 错误
///
/// - `UnsupportedProvider`: provider 无对应引擎实现
/// - `Search`: 底层搜索引擎失败
/// - `Network`: 网络请求失败
pub async fn search(
    provider: SearchProvider,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResult>, AgentLibError> {
    let engine_client = build_engine_client();
    let search_client = SearchClient::new(engine_client);
    search_with_client(&search_client, provider, query, limit).await
}

/// 使用注入的 `SearchClient` 执行搜索（供测试 mock 注入）。
///
/// `pub(crate)` 仅限本 crate 使用，测试可通过 `SearchClient::new_with_engines`
/// 注入 mock 引擎验证结果解析（A005）。
pub(crate) async fn search_with_client(
    client: &SearchClient,
    provider: SearchProvider,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResult>, AgentLibError> {
    let engine = provider_to_engine_type(provider)?;

    let request = client
        .search(query)
        .with_engine(engine.name())
        .limit(limit.clamp(0, u32::MAX as usize) as u32);

    let response = request
        .execute()
        .await
        .map_err(|e| AgentLibError::Search(format!("{}: {}", engine.name(), e)))?;

    Ok(response.items.into_iter().map(map_item).collect())
}

/// 将 [`SearchProvider`] 映射为 [`SearchEngineType`]。
fn provider_to_engine_type(provider: SearchProvider) -> Result<SearchEngineType, AgentLibError> {
    match provider {
        SearchProvider::Bing => Ok(SearchEngineType::Bing),
        SearchProvider::Baidu => Ok(SearchEngineType::Baidu),
        SearchProvider::Sogou => Ok(SearchEngineType::Sogou),
        SearchProvider::Google => Err(AgentLibError::UnsupportedProvider("Google".to_string())),
    }
}

/// 将引擎 [`ResponseItem`] 映射为库面 [`SearchResult`]。
fn map_item(item: ResponseItem) -> SearchResult {
    SearchResult {
        title: item.title,
        url: item.url,
        snippet: item.description,
    }
}

/// 构建带 ReqwestEngine 的最小 `EngineClient`（真实搜索链路）。
///
/// 与 `utils::search_test::build_test_engine_client` 同构，但复用 SSRF-safe
/// 的 `create_http_client()` 作为底层 HTTP 客户端。
fn build_engine_client() -> Arc<EngineClient> {
    let http_client = create_http_client();
    let reqwest_engine = ReqwestEngine::new(http_client);
    let router: Arc<dyn EngineRouterTrait> =
        Arc::new(EngineRouter::new(vec![Arc::new(reqwest_engine)]));
    Arc::new(EngineClient::with_router(router))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::engine_trait::{SearchEngine, SearchRequest};
    use crate::search::error::SearchError;
    use crate::search::response::Response;
    use crate::search::types::EngineHealth;
    use async_trait::async_trait;

    /// Mock 引擎：返回固定条数的结构化结果，供解析断言。
    struct MockEngine {
        engine_type: SearchEngineType,
        items: Vec<ResponseItem>,
    }

    #[async_trait]
    impl SearchEngine for MockEngine {
        fn name(&self) -> &'static str {
            "MockEngine"
        }

        fn engine_type(&self) -> SearchEngineType {
            self.engine_type
        }

        fn health(&self) -> EngineHealth {
            EngineHealth::Healthy
        }

        async fn search(
            &self,
            request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            let slice = self
                .items
                .iter()
                .take(request.limit as usize)
                .cloned()
                .collect::<Vec<_>>();
            Ok(Response {
                items: slice,
                total_results: Some(self.items.len() as u64),
                engine: self.engine_type,
            })
        }
    }

    fn mock_client(engine_type: SearchEngineType) -> SearchClient {
        let engine = MockEngine {
            engine_type,
            items: vec![
                ResponseItem {
                    title: "Rust Web Scraping".to_string(),
                    url: "https://example.com/rust".to_string(),
                    description: "A guide to Rust web scraping".to_string(),
                    engine: engine_type,
                },
                ResponseItem {
                    title: "Async in Rust".to_string(),
                    url: "https://example.com/async".to_string(),
                    description: "Understanding async/await".to_string(),
                    engine: engine_type,
                },
            ],
        };
        SearchClient::new_with_engines(vec![Arc::new(engine) as Arc<dyn SearchEngine>], engine_type)
    }

    #[tokio::test]
    async fn search_bing_parses_items() {
        let client = mock_client(SearchEngineType::Bing);
        let results = search_with_client(&client, SearchProvider::Bing, "rust", 2)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Web Scraping");
        assert_eq!(results[0].url, "https://example.com/rust");
        assert_eq!(results[0].snippet, "A guide to Rust web scraping");
    }

    #[tokio::test]
    async fn search_baidu_parses_items() {
        let client = mock_client(SearchEngineType::Baidu);
        let results = search_with_client(&client, SearchProvider::Baidu, "rust", 1)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust Web Scraping");
    }

    #[tokio::test]
    async fn search_sogou_parses_items() {
        let client = mock_client(SearchEngineType::Sogou);
        let results = search_with_client(&client, SearchProvider::Sogou, "rust", 2)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[1].url, "https://example.com/async");
    }

    #[tokio::test]
    async fn search_limit_zero_returns_empty() {
        let client = mock_client(SearchEngineType::Bing);
        let results = search_with_client(&client, SearchProvider::Bing, "rust", 0)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn search_google_unsupported_provider() {
        let client = mock_client(SearchEngineType::Bing);
        let err = search_with_client(&client, SearchProvider::Google, "rust", 2)
            .await
            .unwrap_err();
        assert!(matches!(err, AgentLibError::UnsupportedProvider(_)));
    }

    #[tokio::test]
    async fn search_no_engine_for_provider_errors() {
        // Sogou 引擎未注册，Baidu 请求应命中 NoEngineAvailable
        let client = mock_client(SearchEngineType::Sogou);
        let err = search_with_client(&client, SearchProvider::Baidu, "rust", 2)
            .await
            .unwrap_err();
        assert!(matches!(err, AgentLibError::Search(_)));
    }
}
