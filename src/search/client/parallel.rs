// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Parallel 搜索引擎 — MCP JSON-RPC 2.0 协议
//!
//! 通过 Parallel MCP 端点 (`https://search.parallel.ai/mcp`) 执行 Web 搜索。
//! 支持可选 Bearer token 认证。

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::types::{EngineHealth, SearchEngineType};

/// Parallel MCP 端点默认 URL
const DEFAULT_PARALLEL_ENDPOINT: &str = "https://search.parallel.ai/mcp";
/// 请求超时（秒）
const PARALLEL_TIMEOUT_SECS: u64 = 25;
/// 响应体最大大小（256KB）
const MAX_RESPONSE_SIZE: usize = 256 * 1024;

/// Parallel 搜索引擎配置
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    /// API Key（可选，用于 Bearer token 认证）
    pub api_key: String,
    /// MCP 端点 URL
    pub endpoint: String,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            endpoint: DEFAULT_PARALLEL_ENDPOINT.to_string(),
        }
    }
}

/// Parallel 搜索引擎（MCP JSON-RPC 2.0 协议）
pub struct ParallelSearchEngine {
    client: Client,
    config: ParallelConfig,
}

impl ParallelSearchEngine {
    pub fn new(client: Client, config: ParallelConfig) -> Self {
        Self { client, config }
    }

    /// 构造 MCP JSON-RPC 2.0 请求体
    fn build_request(&self, query: &str, limit: u32) -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "web_search",
                "arguments": {
                    "objective": query,
                    "search_queries": [query],
                    "max_results": limit,
                }
            },
            "id": 1,
        })
    }

    /// 解析 SSE 流式响应
    fn parse_sse_response(body: &str) -> Result<JsonRpcResponse, SearchError> {
        let mut last_data = None;
        for line in body.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                last_data = Some(data);
            }
        }

        let data = last_data.ok_or_else(|| {
            SearchError::Parse("No SSE data lines found in Parallel response".to_string())
        })?;

        serde_json::from_str::<JsonRpcResponse>(data)
            .map_err(|e| SearchError::Parse(format!("Failed to parse SSE JSON-RPC: {}", e)))
    }

    /// 解析直接 JSON 响应
    fn parse_json_response(body: &str) -> Result<JsonRpcResponse, SearchError> {
        serde_json::from_str::<JsonRpcResponse>(body)
            .map_err(|e| SearchError::Parse(format!("Failed to parse JSON-RPC response: {}", e)))
    }

    /// 从 MCP 响应内容中提取搜索结果
    fn extract_results(rpc_response: &JsonRpcResponse) -> Vec<ParallelSearchResult> {
        let mut results = Vec::new();

        if let Some(result) = &rpc_response.result {
            for content in &result.content {
                if content.content_type == "text" {
                    // 尝试解析为结构化结果
                    if let Ok(parallel_result) =
                        serde_json::from_str::<ParallelResult>(&content.text)
                    {
                        for item in &parallel_result.results {
                            results.push(ParallelSearchResult {
                                title: item.title.clone(),
                                url: item.url.clone(),
                                description: item.snippet.clone().unwrap_or_default(),
                            });
                        }
                        continue;
                    }

                    // Fallback: 尝试解析为通用 JSON 数组
                    if let Ok(Value::Array(items)) =
                        serde_json::from_str::<Value>(&content.text)
                    {
                        for item in &items {
                            if let Some(result) = parse_generic_item(item) {
                                results.push(result);
                            }
                        }
                    }
                }
            }
        }

        results
    }
}

#[async_trait]
impl SearchEngine for ParallelSearchEngine {
    fn name(&self) -> &'static str {
        "Parallel"
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Parallel
    }

    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(
        &self,
        request: &SearchRequest,
    ) -> Result<Response<ResponseItem>, SearchError> {
        let body = self.build_request(&request.query, request.limit);

        let mut req_builder = self
            .client
            .post(&self.config.endpoint)
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(PARALLEL_TIMEOUT_SECS));

        // 有 API Key 时添加 Bearer token
        if !self.config.api_key.is_empty() {
            req_builder =
                req_builder.header("Authorization", format!("Bearer {}", self.config.api_key));
        }

        let response = req_builder.send().await?;

        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !status.is_success() {
            return Err(SearchError::BadHttpStatus(
                "Parallel".to_string(),
                status.as_u16(),
            ));
        }

        let body_text = response.text().await?;
        if body_text.len() > MAX_RESPONSE_SIZE {
            return Err(SearchError::Parse(format!(
                "Parallel response exceeds 256KB limit: {} bytes",
                body_text.len()
            )));
        }

        let rpc_response = if content_type.contains("text/event-stream") {
            Self::parse_sse_response(&body_text)?
        } else {
            Self::parse_json_response(&body_text)?
        };

        if let Some(error) = &rpc_response.error {
            return Err(SearchError::EngineFailed(format!(
                "Parallel JSON-RPC error: code={}, message={}",
                error.code, error.message
            )));
        }

        let results = Self::extract_results(&rpc_response);

        let items: Vec<ResponseItem> = results
            .into_iter()
            .map(|r| ResponseItem {
                title: r.title,
                url: r.url,
                description: r.description,
                engine: SearchEngineType::Parallel,
            })
            .collect();

        Ok(Response {
            total_results: Some(items.len() as u64),
            items,
            engine: SearchEngineType::Parallel,
        })
    }
}

// =============================================================================
// 内部数据结构
// =============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: Option<String>,
    result: Option<McpResponse>,
    error: Option<JsonRpcError>,
    id: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct McpResponse {
    content: Vec<McpContent>,
}

#[derive(Debug, Serialize, Deserialize)]
struct McpContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ParallelResult {
    #[serde(default)]
    results: Vec<ParallelRawResult>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ParallelRawResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    snippet: Option<String>,
    description: Option<String>,
}

struct ParallelSearchResult {
    title: String,
    url: String,
    description: String,
}

fn parse_generic_item(item: &Value) -> Option<ParallelSearchResult> {
    let url = item
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    if url.is_empty() {
        return None;
    }

    let title = item
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let description = item
        .get("snippet")
        .or_else(|| item.get("description"))
        .or_else(|| item.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    Some(ParallelSearchResult {
        title,
        url,
        description,
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ParallelConfig {
        ParallelConfig::default()
    }

    fn test_engine() -> ParallelSearchEngine {
        ParallelSearchEngine::new(Client::new(), test_config())
    }

    // ========== 请求构造 ==========

    #[test]
    fn test_build_request_structure() {
        let engine = test_engine();
        let req = engine.build_request("rust search", 10);

        assert_eq!(req["jsonrpc"], "2.0");
        assert_eq!(req["method"], "tools/call");
        assert_eq!(req["params"]["name"], "web_search");
        assert_eq!(req["params"]["arguments"]["objective"], "rust search");
        assert_eq!(req["params"]["arguments"]["search_queries"][0], "rust search");
        assert_eq!(req["params"]["arguments"]["max_results"], 10);
    }

    // ========== 响应解析 ==========

    #[test]
    fn test_parse_json_response_success() {
        let body = r#"{
            "jsonrpc": "2.0",
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": "{\"results\": [{\"title\": \"Parallel Result\", \"url\": \"https://parallel.com\", \"snippet\": \"A snippet\"}]}"
                    }
                ]
            },
            "id": 1
        }"#;

        let rpc = ParallelSearchEngine::parse_json_response(body).unwrap();
        assert!(rpc.error.is_none());
        let results = ParallelSearchEngine::extract_results(&rpc);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Parallel Result");
        assert_eq!(results[0].url, "https://parallel.com");
        assert_eq!(results[0].description, "A snippet");
    }

    #[test]
    fn test_parse_sse_response() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"{\\\"results\\\":[{\\\"title\\\":\\\"SSE\\\",\\\"url\\\":\\\"https://sse.com\\\",\\\"snippet\\\":\\\"desc\\\"}]}\"}]},\"id\":1}\n\n";

        let rpc = ParallelSearchEngine::parse_sse_response(body).unwrap();
        let results = ParallelSearchEngine::extract_results(&rpc);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "SSE");
    }

    #[test]
    fn test_parse_json_rpc_error() {
        let body = r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":1}"#;
        let rpc = ParallelSearchEngine::parse_json_response(body).unwrap();
        assert!(rpc.error.is_some());
    }

    #[test]
    fn test_extract_results_empty() {
        let body = r#"{"jsonrpc":"2.0","result":{"content":[]},"id":1}"#;
        let rpc = ParallelSearchEngine::parse_json_response(body).unwrap();
        assert!(ParallelSearchEngine::extract_results(&rpc).is_empty());
    }

    // ========== trait 方法 ==========

    #[test]
    fn test_engine_name() {
        assert_eq!(test_engine().name(), "Parallel");
    }

    #[test]
    fn test_engine_type() {
        assert_eq!(test_engine().engine_type(), SearchEngineType::Parallel);
    }

    // ========== 集成测试（wiremock） ==========

    #[tokio::test]
    async fn test_search_without_api_key() {
        let mock_server = wiremock::MockServer::start().await;

        let response_body = r#"{
            "jsonrpc": "2.0",
            "result": {
                "content": [{"type": "text", "text": "{\"results\": [{\"title\": \"Test\", \"url\": \"https://test.com\", \"snippet\": \"desc\"}]}"}]
            },
            "id": 1
        }"#;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(response_body))
            .mount(&mock_server)
            .await;

        let config = ParallelConfig {
            api_key: String::new(),
            endpoint: mock_server.uri(),
        };
        let engine = ParallelSearchEngine::new(Client::new(), config);
        let request = SearchRequest::new("test").with_limit(5);
        let response = engine.search(&request).await.unwrap();

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].engine, SearchEngineType::Parallel);
    }

    #[tokio::test]
    async fn test_search_with_api_key() {
        let mock_server = wiremock::MockServer::start().await;

        let response_body = r#"{
            "jsonrpc": "2.0",
            "result": {
                "content": [{"type": "text", "text": "{\"results\": [{\"title\": \"Auth\", \"url\": \"https://auth.com\", \"snippet\": \"auth desc\"}]}"}]
            },
            "id": 1
        }"#;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::header("Authorization", "Bearer my-key"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(response_body))
            .mount(&mock_server)
            .await;

        let config = ParallelConfig {
            api_key: "my-key".to_string(),
            endpoint: mock_server.uri(),
        };
        let engine = ParallelSearchEngine::new(Client::new(), config);
        let request = SearchRequest::new("test").with_limit(5);
        let response = engine.search(&request).await.unwrap();

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].title, "Auth");
    }

    #[tokio::test]
    async fn test_search_http_error() {
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(429))
            .mount(&mock_server)
            .await;

        let config = ParallelConfig {
            api_key: String::new(),
            endpoint: mock_server.uri(),
        };
        let engine = ParallelSearchEngine::new(Client::new(), config);
        let request = SearchRequest::new("test").with_limit(5);
        let result = engine.search(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            SearchError::BadHttpStatus(name, code) => {
                assert_eq!(name, "Parallel");
                assert_eq!(code, 429);
            }
            other => panic!("expected BadHttpStatus, got {:?}", other),
        }
    }
}
