// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Tavily 搜索引擎 — REST API
//!
//! 通过 Tavily REST API (`https://api.tavily.com/search`) 执行 Web 搜索。
//! 支持 keyless 模式（`X-Tavily-Access-Mode: keyless`）和 Bearer token 认证。

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::types::{EngineHealth, SearchEngineType};

/// Tavily API 默认基础 URL
const DEFAULT_TAVILY_ENDPOINT: &str = "https://api.tavily.com";
/// 请求超时（秒）
const TAVILY_TIMEOUT_SECS: u64 = 25;
/// 响应体最大大小（256KB）
const MAX_RESPONSE_SIZE: usize = 256 * 1024;

/// Tavily 搜索引擎配置
#[derive(Debug, Clone)]
pub struct TavilyConfig {
    /// API Key（可选，空字符串表示使用 keyless 模式）
    pub api_key: String,
    /// API 基础 URL
    pub endpoint: String,
}

impl Default for TavilyConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            endpoint: DEFAULT_TAVILY_ENDPOINT.to_string(),
        }
    }
}

/// Tavily 搜索引擎（REST API）
pub struct TavilySearchEngine {
    client: Client,
    config: TavilyConfig,
}

impl TavilySearchEngine {
    pub fn new(client: Client, config: TavilyConfig) -> Self {
        Self { client, config }
    }

    /// 构造搜索请求体
    fn build_request(&self, query: &str, limit: u32) -> Value {
        json!({
            "query": query,
            "max_results": limit,
        })
    }

    /// 解析 Tavily 搜索响应
    fn parse_response(body: &str) -> Result<Vec<TavilySearchResult>, SearchError> {
        let response: TavilyResponse = serde_json::from_str(body).map_err(|e| {
            SearchError::Parse(format!("Failed to parse Tavily response: {}", e))
        })?;

        Ok(response
            .results
            .into_iter()
            .map(|r| TavilySearchResult {
                title: r.title.unwrap_or_default(),
                url: r.url,
                description: r.content.unwrap_or_default(),
            })
            .collect())
    }
}

#[async_trait]
impl SearchEngine for TavilySearchEngine {
    fn name(&self) -> &'static str {
        "Tavily"
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Tavily
    }

    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(
        &self,
        request: &SearchRequest,
    ) -> Result<Response<ResponseItem>, SearchError> {
        let body = self.build_request(&request.query, request.limit);
        let url = format!("{}/search", self.config.endpoint);

        let mut req_builder = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(TAVILY_TIMEOUT_SECS));

        // 根据是否有 API Key 选择认证方式
        if !self.config.api_key.is_empty() {
            req_builder = req_builder
                .header("Authorization", format!("Bearer {}", self.config.api_key));
        } else {
            req_builder = req_builder.header("X-Tavily-Access-Mode", "keyless");
        }

        let response = req_builder.send().await?;

        let status = response.status();
        if !status.is_success() {
            return Err(SearchError::BadHttpStatus(
                "Tavily".to_string(),
                status.as_u16(),
            ));
        }

        let body_text = response.text().await?;
        if body_text.len() > MAX_RESPONSE_SIZE {
            return Err(SearchError::Parse(format!(
                "Tavily response exceeds 256KB limit: {} bytes",
                body_text.len()
            )));
        }

        let results = Self::parse_response(&body_text)?;

        let items: Vec<ResponseItem> = results
            .into_iter()
            .map(|r| ResponseItem {
                title: r.title,
                url: r.url,
                description: r.description,
                engine: SearchEngineType::Tavily,
            })
            .collect();

        Ok(Response {
            total_results: Some(items.len() as u64),
            items,
            engine: SearchEngineType::Tavily,
        })
    }
}

// =============================================================================
// 内部数据结构
// =============================================================================

struct TavilySearchResult {
    title: String,
    url: String,
    description: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TavilyResponse {
    #[serde(default)]
    results: Vec<TavilyRawResult>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TavilyRawResult {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    content: Option<String>,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> TavilyConfig {
        TavilyConfig::default()
    }

    fn test_engine() -> TavilySearchEngine {
        TavilySearchEngine::new(Client::new(), test_config())
    }

    // ========== 请求构造 ==========

    #[test]
    fn test_build_request_structure() {
        let engine = test_engine();
        let req = engine.build_request("rust search", 10);

        assert_eq!(req["query"], "rust search");
        assert_eq!(req["max_results"], 10);
    }

    // ========== 响应解析 ==========

    #[test]
    fn test_parse_response_success() {
        let body = r#"{
            "results": [
                {"title": "Result 1", "url": "https://example.com/1", "content": "Description 1"},
                {"title": "Result 2", "url": "https://example.com/2", "content": "Description 2"}
            ]
        }"#;

        let results = TavilySearchEngine::parse_response(body).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Result 1");
        assert_eq!(results[0].url, "https://example.com/1");
        assert_eq!(results[0].description, "Description 1");
    }

    #[test]
    fn test_parse_response_missing_optional_fields() {
        let body = r#"{
            "results": [
                {"url": "https://example.com"}
            ]
        }"#;

        let results = TavilySearchEngine::parse_response(body).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "");
        assert_eq!(results[0].description, "");
    }

    #[test]
    fn test_parse_response_empty_results() {
        let body = r#"{"results": []}"#;
        let results = TavilySearchEngine::parse_response(body).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_response_invalid_json() {
        let body = "not json";
        assert!(TavilySearchEngine::parse_response(body).is_err());
    }

    // ========== trait 方法 ==========

    #[test]
    fn test_engine_name() {
        assert_eq!(test_engine().name(), "Tavily");
    }

    #[test]
    fn test_engine_type() {
        assert_eq!(test_engine().engine_type(), SearchEngineType::Tavily);
    }

    // ========== 集成测试（wiremock） ==========

    #[tokio::test]
    async fn test_search_keyless_mode() {
        let mock_server = wiremock::MockServer::start().await;

        let response_body = r#"{
            "results": [
                {"title": "Keyless Result", "url": "https://keyless.com", "content": "keyless desc"}
            ]
        }"#;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/search"))
            .and(wiremock::matchers::header("X-Tavily-Access-Mode", "keyless"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(response_body))
            .mount(&mock_server)
            .await;

        let config = TavilyConfig {
            api_key: String::new(),
            endpoint: mock_server.uri(),
        };
        let engine = TavilySearchEngine::new(Client::new(), config);
        let request = SearchRequest::new("test").with_limit(5);
        let response = engine.search(&request).await.unwrap();

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].title, "Keyless Result");
        assert_eq!(response.items[0].engine, SearchEngineType::Tavily);
    }

    #[tokio::test]
    async fn test_search_keyed_mode() {
        let mock_server = wiremock::MockServer::start().await;

        let response_body = r#"{
            "results": [
                {"title": "Keyed Result", "url": "https://keyed.com", "content": "keyed desc"}
            ]
        }"#;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/search"))
            .and(wiremock::matchers::header("Authorization", "Bearer tavily-key"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(response_body))
            .mount(&mock_server)
            .await;

        let config = TavilyConfig {
            api_key: "tavily-key".to_string(),
            endpoint: mock_server.uri(),
        };
        let engine = TavilySearchEngine::new(Client::new(), config);
        let request = SearchRequest::new("test").with_limit(5);
        let response = engine.search(&request).await.unwrap();

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].title, "Keyed Result");
    }

    #[tokio::test]
    async fn test_search_http_error() {
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(403))
            .mount(&mock_server)
            .await;

        let config = TavilyConfig {
            api_key: String::new(),
            endpoint: mock_server.uri(),
        };
        let engine = TavilySearchEngine::new(Client::new(), config);
        let request = SearchRequest::new("test").with_limit(5);
        let result = engine.search(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            SearchError::BadHttpStatus(name, code) => {
                assert_eq!(name, "Tavily");
                assert_eq!(code, 403);
            }
            other => panic!("expected BadHttpStatus, got {:?}", other),
        }
    }
}
