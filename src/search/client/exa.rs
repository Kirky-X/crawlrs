// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Exa 搜索引擎 — MCP JSON-RPC 2.0 协议
//!
//! 通过 Exa MCP 端点 (`https://mcp.exa.ai/mcp`) 执行 Web 搜索。
//! 支持匿名访问（无需 API Key）和带 API Key 的认证访问。

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::types::{EngineHealth, SearchEngineType};

/// Exa MCP 端点默认 URL
const DEFAULT_EXA_ENDPOINT: &str = "https://mcp.exa.ai/mcp";
/// 请求超时（秒）
const EXA_TIMEOUT_SECS: u64 = 25;
/// 响应体最大大小（256KB）
const MAX_RESPONSE_SIZE: usize = 256 * 1024;

/// Exa 搜索引擎配置
#[derive(Debug, Clone)]
pub struct ExaConfig {
    /// API Key（可选，Exa MCP 支持匿名访问）
    pub api_key: String,
    /// MCP 端点 URL
    pub endpoint: String,
}

impl Default for ExaConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            endpoint: DEFAULT_EXA_ENDPOINT.to_string(),
        }
    }
}

/// Exa 搜索引擎（MCP JSON-RPC 2.0 协议）
pub struct ExaSearchEngine {
    client: Client,
    config: ExaConfig,
}

impl ExaSearchEngine {
    pub fn new(client: Client, config: ExaConfig) -> Self {
        Self { client, config }
    }

    /// 构造 MCP JSON-RPC 2.0 请求体
    fn build_request(&self, query: &str, limit: u32) -> Value {
        let args = json!({
            "query": query,
            "numResults": limit,
        });

        json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "web_search_exa",
                "arguments": args,
            },
            "id": 1,
        })
    }

    /// 获取实际请求 URL（含可选 API Key）
    fn request_url(&self) -> String {
        if !self.config.api_key.is_empty() {
            format!("{}?apiKey={}", self.config.endpoint, self.config.api_key)
        } else {
            self.config.endpoint.clone()
        }
    }

    /// 解析 SSE 流式响应，提取最后一个 `data:` 行的 JSON-RPC 响应
    fn parse_sse_response(body: &str) -> Result<JsonRpcResponse, SearchError> {
        let mut last_data = None;
        for line in body.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                last_data = Some(data);
            }
        }

        let data = last_data.ok_or_else(|| {
            SearchError::Parse("No SSE data lines found in Exa response".to_string())
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
    fn extract_results(rpc_response: &JsonRpcResponse) -> Vec<ExaSearchResult> {
        let mut results = Vec::new();

        if let Some(result) = &rpc_response.result {
            for content in &result.content {
                if content.content_type == "text" {
                    // 尝试解析为结构化 MCP 结果
                    if let Ok(mcp_result) = serde_json::from_str::<McpResult>(&content.text) {
                        for item in &mcp_result.results {
                            results.push(ExaSearchResult {
                                title: item.title.clone(),
                                url: item.url.clone(),
                                description: item
                                    .text
                                    .as_deref()
                                    .or(item.description.as_deref())
                                    .or(item.snippet.as_deref())
                                    .unwrap_or("")
                                    .to_string(),
                            });
                        }
                        continue;
                    }

                    // Fallback: 尝试解析为通用 JSON 数组
                    if let Ok(Value::Array(items)) =
                        serde_json::from_str::<Value>(&content.text)
                    {
                        for item in &items {
                            if let Some(result) = parse_generic_result_item(item) {
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
impl SearchEngine for ExaSearchEngine {
    fn name(&self) -> &'static str {
        "Exa"
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Exa
    }

    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(
        &self,
        request: &SearchRequest,
    ) -> Result<Response<ResponseItem>, SearchError> {
        let body = self.build_request(&request.query, request.limit);
        let url = self.request_url();

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(EXA_TIMEOUT_SECS))
            .send()
            .await?;

        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !status.is_success() {
            return Err(SearchError::BadHttpStatus(
                "Exa".to_string(),
                status.as_u16(),
            ));
        }

        let body_text = response.text().await?;
        if body_text.len() > MAX_RESPONSE_SIZE {
            return Err(SearchError::Parse(format!(
                "Exa response exceeds 256KB limit: {} bytes",
                body_text.len()
            )));
        }

        // 根据 Content-Type 选择解析方式
        let rpc_response = if content_type.contains("text/event-stream") {
            Self::parse_sse_response(&body_text)?
        } else {
            Self::parse_json_response(&body_text)?
        };

        // 检查 JSON-RPC 错误
        if let Some(error) = &rpc_response.error {
            return Err(SearchError::EngineFailed(format!(
                "Exa JSON-RPC error: code={}, message={}",
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
                engine: SearchEngineType::Exa,
            })
            .collect();

        Ok(Response {
            total_results: Some(items.len() as u64),
            items,
            engine: SearchEngineType::Exa,
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
struct McpResult {
    #[serde(default)]
    results: Vec<ExaRawResult>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExaRawResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    text: Option<String>,
    /// Fallback description field
    description: Option<String>,
    /// Another fallback field
    snippet: Option<String>,
}

struct ExaSearchResult {
    title: String,
    url: String,
    description: String,
}

/// 从通用 JSON 对象中提取搜索结果
fn parse_generic_result_item(item: &Value) -> Option<ExaSearchResult> {
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
        .get("description")
        .or_else(|| item.get("text"))
        .or_else(|| item.get("snippet"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    Some(ExaSearchResult {
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

    fn test_config() -> ExaConfig {
        ExaConfig::default()
    }

    fn test_engine() -> ExaSearchEngine {
        ExaSearchEngine::new(Client::new(), test_config())
    }

    // ========== 请求构造 ==========

    #[test]
    fn test_build_request_structure() {
        let engine = test_engine();
        let req = engine.build_request("rust web scraping", 5);

        assert_eq!(req["jsonrpc"], "2.0");
        assert_eq!(req["method"], "tools/call");
        assert_eq!(req["params"]["name"], "web_search_exa");
        assert_eq!(req["params"]["arguments"]["query"], "rust web scraping");
        assert_eq!(req["params"]["arguments"]["numResults"], 5);
        assert_eq!(req["id"], 1);
    }

    #[test]
    fn test_request_url_without_api_key() {
        let engine = test_engine();
        assert_eq!(engine.request_url(), "https://mcp.exa.ai/mcp");
    }

    #[test]
    fn test_request_url_with_api_key() {
        let config = ExaConfig {
            api_key: "test-key-123".to_string(),
            endpoint: "https://mcp.exa.ai/mcp".to_string(),
        };
        let engine = ExaSearchEngine::new(Client::new(), config);
        assert_eq!(
            engine.request_url(),
            "https://mcp.exa.ai/mcp?apiKey=test-key-123"
        );
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
                        "text": "{\"results\": [{\"title\": \"Rust Lang\", \"url\": \"https://rust-lang.org\", \"text\": \"A language for everyone\"}]}"
                    }
                ]
            },
            "id": 1
        }"#;

        let rpc = ExaSearchEngine::parse_json_response(body).unwrap();
        assert!(rpc.error.is_none());
        let results = ExaSearchEngine::extract_results(&rpc);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust Lang");
        assert_eq!(results[0].url, "https://rust-lang.org");
        assert_eq!(results[0].description, "A language for everyone");
    }

    #[test]
    fn test_parse_json_response_with_description_fallback() {
        let body = r#"{
            "jsonrpc": "2.0",
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": "{\"results\": [{\"title\": \"Test\", \"url\": \"https://example.com\", \"description\": \"desc field\"}]}"
                    }
                ]
            },
            "id": 1
        }"#;

        let rpc = ExaSearchEngine::parse_json_response(body).unwrap();
        let results = ExaSearchEngine::extract_results(&rpc);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "desc field");
    }

    #[test]
    fn test_parse_json_response_with_snippet_fallback() {
        let body = r#"{
            "jsonrpc": "2.0",
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": "{\"results\": [{\"title\": \"Test\", \"url\": \"https://example.com\", \"snippet\": \"snippet field\"}]}"
                    }
                ]
            },
            "id": 1
        }"#;

        let rpc = ExaSearchEngine::parse_json_response(body).unwrap();
        let results = ExaSearchEngine::extract_results(&rpc);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "snippet field");
    }

    #[test]
    fn test_parse_sse_response() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"{\\\"results\\\":[{\\\"title\\\":\\\"SSE Result\\\",\\\"url\\\":\\\"https://sse.example.com\\\",\\\"text\\\":\\\"SSE description\\\"}]}\"}]},\"id\":1}\n\n";

        let rpc = ExaSearchEngine::parse_sse_response(body).unwrap();
        let results = ExaSearchEngine::extract_results(&rpc);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "SSE Result");
        assert_eq!(results[0].url, "https://sse.example.com");
    }

    #[test]
    fn test_parse_sse_no_data_lines() {
        let body = "event: message\n\n";
        let result = ExaSearchEngine::parse_sse_response(body);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_json_response_error() {
        let body = r#"{
            "jsonrpc": "2.0",
            "error": {"code": -32600, "message": "Invalid Request"},
            "id": 1
        }"#;

        let rpc = ExaSearchEngine::parse_json_response(body).unwrap();
        assert!(rpc.error.is_some());
        assert_eq!(rpc.error.as_ref().unwrap().code, -32600);
    }

    #[test]
    fn test_parse_generic_json_array_fallback() {
        let body = r#"{
            "jsonrpc": "2.0",
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": "[{\"title\":\"Generic\",\"url\":\"https://generic.com\",\"description\":\"generic desc\"}]"
                    }
                ]
            },
            "id": 1
        }"#;

        let rpc = ExaSearchEngine::parse_json_response(body).unwrap();
        let results = ExaSearchEngine::extract_results(&rpc);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Generic");
        assert_eq!(results[0].url, "https://generic.com");
    }

    #[test]
    fn test_extract_results_empty_content() {
        let body = r#"{"jsonrpc":"2.0","result":{"content":[]},"id":1}"#;
        let rpc = ExaSearchEngine::parse_json_response(body).unwrap();
        let results = ExaSearchEngine::extract_results(&rpc);
        assert!(results.is_empty());
    }

    #[test]
    fn test_extract_results_no_url_skipped() {
        let body = r#"{
            "jsonrpc": "2.0",
            "result": {
                "content": [{"type": "text", "text": "[{\"title\":\"No URL\",\"url\":\"\"}]"}]
            },
            "id": 1
        }"#;
        let rpc = ExaSearchEngine::parse_json_response(body).unwrap();
        let results = ExaSearchEngine::extract_results(&rpc);
        assert!(results.is_empty());
    }

    // ========== trait 方法 ==========

    #[test]
    fn test_engine_name() {
        let engine = test_engine();
        assert_eq!(engine.name(), "Exa");
    }

    #[test]
    fn test_engine_type() {
        let engine = test_engine();
        assert_eq!(engine.engine_type(), SearchEngineType::Exa);
    }

    #[test]
    fn test_engine_health() {
        let engine = test_engine();
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    // ========== 集成测试（wiremock） ==========

    #[tokio::test]
    async fn test_search_success() {
        let mock_server = wiremock::MockServer::start().await;

        let response_body = r#"{
            "jsonrpc": "2.0",
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": "{\"results\": [{\"title\": \"Test Result\", \"url\": \"https://test.com\", \"text\": \"Test description\"}]}"
                    }
                ]
            },
            "id": 1
        }"#;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::body_json(
                ExaSearchEngine::new(Client::new(), test_config())
                    .build_request("test query", 5),
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(response_body))
            .mount(&mock_server)
            .await;

        let config = ExaConfig {
            api_key: String::new(),
            endpoint: mock_server.uri(),
        };
        let engine = ExaSearchEngine::new(Client::new(), config);
        let request = SearchRequest::new("test query").with_limit(5);
        let response = engine.search(&request).await.unwrap();

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].title, "Test Result");
        assert_eq!(response.items[0].url, "https://test.com");
        assert_eq!(response.items[0].engine, SearchEngineType::Exa);
    }

    #[tokio::test]
    async fn test_search_http_error() {
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let config = ExaConfig {
            api_key: String::new(),
            endpoint: mock_server.uri(),
        };
        let engine = ExaSearchEngine::new(Client::new(), config);
        let request = SearchRequest::new("test").with_limit(5);
        let result = engine.search(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            SearchError::BadHttpStatus(name, code) => {
                assert_eq!(name, "Exa");
                assert_eq!(code, 500);
            }
            other => panic!("expected BadHttpStatus, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_search_json_rpc_error() {
        let mock_server = wiremock::MockServer::start().await;

        let response_body = r#"{
            "jsonrpc": "2.0",
            "error": {"code": -32601, "message": "Method not found"},
            "id": 1
        }"#;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(response_body))
            .mount(&mock_server)
            .await;

        let config = ExaConfig {
            api_key: String::new(),
            endpoint: mock_server.uri(),
        };
        let engine = ExaSearchEngine::new(Client::new(), config);
        let request = SearchRequest::new("test").with_limit(5);
        let result = engine.search(&request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            SearchError::EngineFailed(msg) => {
                assert!(msg.contains("Method not found"));
            }
            other => panic!("expected EngineFailed, got {:?}", other),
        }
    }
}
