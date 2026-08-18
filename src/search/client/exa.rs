// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Exa 搜索引擎 — MCP Streamable HTTP 协议
//!
//! 通过 Exa MCP 端点 (`https://mcp.exa.ai/mcp`) 执行 Web 搜索。
//! 实现完整的 MCP 会话生命周期：initialize → notifications/initialized → tools/call。
//! 支持匿名访问（无需 API Key）和带 API Key 的认证访问。

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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
/// MCP 协议版本
const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

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

/// MCP 会话状态
struct SessionState {
    session_id: Option<String>,
    initialized: bool,
}

/// Exa 搜索引擎（MCP Streamable HTTP 协议）
pub struct ExaSearchEngine {
    client: Client,
    config: ExaConfig,
    session: Arc<parking_lot::Mutex<SessionState>>,
    request_id: AtomicU64,
}

impl ExaSearchEngine {
    pub fn new(client: Client, config: ExaConfig) -> Self {
        Self {
            client,
            config,
            session: Arc::new(parking_lot::Mutex::new(SessionState {
                session_id: None,
                initialized: false,
            })),
            request_id: AtomicU64::new(1),
        }
    }

    /// 获取下一个请求 ID
    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::Relaxed)
    }

    /// 获取实际请求 URL（含可选 API Key）
    fn request_url(&self) -> String {
        if !self.config.api_key.is_empty() {
            format!("{}?apiKey={}", self.config.endpoint, self.config.api_key)
        } else {
            self.config.endpoint.clone()
        }
    }

    /// 发送 MCP 请求并解析 SSE/JSON 响应
    async fn send_mcp_request(
        &self,
        body: &Value,
        session_id: Option<&str>,
    ) -> Result<(Value, Option<String>), SearchError> {
        let url = self.request_url();
        let mut req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(body)
            .timeout(std::time::Duration::from_secs(EXA_TIMEOUT_SECS));

        if let Some(sid) = session_id {
            req = req.header("Mcp-Session-Id", sid);
        }

        let response = req.send().await?;
        let status = response.status();

        if !status.is_success() {
            return Err(SearchError::BadHttpStatus(
                "Exa".to_string(),
                status.as_u16(),
            ));
        }

        // 提取新的 session ID（如果有）
        let new_session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body_text = response.text().await?;
        if body_text.len() > MAX_RESPONSE_SIZE {
            return Err(SearchError::Parse(format!(
                "Exa response exceeds 256KB limit: {} bytes",
                body_text.len()
            )));
        }

        // 根据 Content-Type 解析响应
        let json_value = if content_type.contains("text/event-stream") {
            Self::parse_sse_to_json(&body_text)?
        } else {
            serde_json::from_str::<Value>(&body_text).map_err(|e| {
                SearchError::Parse(format!("Failed to parse Exa JSON response: {}", e))
            })?
        };

        Ok((json_value, new_session_id))
    }

    /// 初始化 MCP 会话
    async fn ensure_initialized(&self) -> Result<(), SearchError> {
        // 快速路径：已初始化
        {
            let state = self.session.lock();
            if state.initialized {
                return Ok(());
            }
        }

        // 发送 initialize 请求
        let init_body = json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "initialize",
            "params": {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "crawlrs",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });

        let (response, new_session_id) = self.send_mcp_request(&init_body, None).await?;

        // 检查 initialize 响应
        if let Some(error) = response.get("error") {
            return Err(SearchError::EngineFailed(format!(
                "Exa MCP initialize error: {}",
                error
            )));
        }

        // 更新 session state
        {
            let mut state = self.session.lock();
            state.session_id = new_session_id.or_else(|| state.session_id.take());
        }

        // 发送 initialized 通知
        let session_id = self.session.lock().session_id.clone();
        let notify_body = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });

        // 通知请求不需要响应体，忽略错误
        let _ = self
            .send_mcp_request(&notify_body, session_id.as_deref())
            .await;

        // 标记已初始化
        self.session.lock().initialized = true;

        Ok(())
    }

    /// 解析 SSE 响应为 JSON Value
    fn parse_sse_to_json(body: &str) -> Result<Value, SearchError> {
        let mut last_data = None;
        for line in body.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                last_data = Some(data);
            }
        }

        let data = last_data.ok_or_else(|| {
            SearchError::Parse("No SSE data lines found in Exa response".to_string())
        })?;

        serde_json::from_str::<Value>(data)
            .map_err(|e| SearchError::Parse(format!("Failed to parse SSE JSON: {}", e)))
    }

    /// 从 MCP tools/call 响应中提取搜索结果
    fn extract_results_from_response(response: &Value) -> Vec<ExaSearchResult> {
        let mut results = Vec::new();

        // 获取 result.content 数组
        let content_array = response
            .get("result")
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_array());

        let Some(content_array) = content_array else {
            return results;
        };

        for content in content_array {
            let text = content.get("text").and_then(|t| t.as_str()).unwrap_or("");

            if text.is_empty() {
                continue;
            }

            // 尝试解析为结构化 JSON（McpResult 格式）
            if let Ok(mcp_result) = serde_json::from_str::<McpResult>(text) {
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

            // 尝试解析为 JSON 数组
            if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text) {
                for item in &items {
                    if let Some(result) = parse_generic_result_item(item) {
                        results.push(result);
                    }
                }
                continue;
            }

            // 解析文本格式（Exa MCP 实际返回格式）
            // 每个结果由 "\n---\n" 分隔，包含 Title:/URL:/Highlights: 等字段
            let text_results = Self::parse_text_results(text);
            results.extend(text_results);
        }

        results
    }

    /// 解析 Exa MCP 文本格式的搜索结果
    ///
    /// 格式：每个结果由 `\n---\n` 分隔，包含：
    /// ```text
    /// Title: ...
    /// URL: ...
    /// Published: ...
    /// Author: ...
    /// Highlights:
    /// ...
    /// ```
    fn parse_text_results(text: &str) -> Vec<ExaSearchResult> {
        let mut results = Vec::new();

        // 按 "---" 分隔符拆分结果块
        let blocks: Vec<&str> = text.split("\n---\n").collect();

        for block in blocks {
            let block = block.trim();
            if block.is_empty() {
                continue;
            }

            let mut title = String::new();
            let mut url = String::new();
            let mut description = String::new();

            for line in block.lines() {
                let line = line.trim();
                if let Some(t) = line.strip_prefix("Title: ") {
                    title = t.trim().to_string();
                } else if let Some(u) = line.strip_prefix("URL: ") {
                    url = u.trim().to_string();
                } else if line.starts_with("Highlights:") {
                    // Highlights 后面的行是描述内容的片段
                    // 收集非字段行作为描述
                    continue;
                } else if !line.starts_with("Published:")
                    && !line.starts_with("Author:")
                    && !line.is_empty()
                    && !line.starts_with("...")
                {
                    // 非元数据行，作为描述片段
                    if !description.is_empty() {
                        description.push(' ');
                    }
                    description.push_str(line);
                }
            }

            // 截断描述到合理长度
            if description.len() > 500 {
                description.truncate(500);
                description.push_str("...");
            }

            if !url.is_empty() {
                results.push(ExaSearchResult {
                    title,
                    url,
                    description,
                });
            }
        }

        results
    }

    /// 构造 tools/call 请求体
    fn build_search_request(&self, query: &str, limit: u32) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "tools/call",
            "params": {
                "name": "web_search_exa",
                "arguments": {
                    "query": query,
                    "numResults": limit,
                }
            }
        })
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

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        // 确保 MCP 会话已初始化
        self.ensure_initialized().await?;

        // 构造搜索请求
        let body = self.build_search_request(&request.query, request.limit);
        let session_id = self.session.lock().session_id.clone();

        let (response, new_session_id) =
            self.send_mcp_request(&body, session_id.as_deref()).await?;

        // 更新 session ID（如果服务器返回了新的）
        if let Some(new_sid) = new_session_id {
            self.session.lock().session_id = Some(new_sid);
        }

        // 检查 JSON-RPC 错误
        if let Some(error) = response.get("error") {
            return Err(SearchError::EngineFailed(format!(
                "Exa JSON-RPC error: {}",
                error
            )));
        }

        let results = Self::extract_results_from_response(&response);

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
    description: Option<String>,
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
        let req = engine.build_search_request("rust web scraping", 5);

        assert_eq!(req["jsonrpc"], "2.0");
        assert_eq!(req["method"], "tools/call");
        assert_eq!(req["params"]["name"], "web_search_exa");
        assert_eq!(req["params"]["arguments"]["query"], "rust web scraping");
        assert_eq!(req["params"]["arguments"]["numResults"], 5);
        assert!(req["id"].is_number());
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

    // ========== 文本格式解析 ==========

    #[test]
    fn test_parse_text_results_single() {
        let text = "Title: Rust Lang\nURL: https://rust-lang.org\nPublished: N/A\nAuthor: N/A\nHighlights:\nRust is fast\n...";

        let results = ExaSearchEngine::parse_text_results(text);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust Lang");
        assert_eq!(results[0].url, "https://rust-lang.org");
        assert!(results[0].description.contains("Rust is fast"));
    }

    #[test]
    fn test_parse_text_results_multiple() {
        let text = "Title: First\nURL: https://first.com\nPublished: N/A\nAuthor: N/A\nHighlights:\nFirst desc\n...\n\n---\n\nTitle: Second\nURL: https://second.com\nPublished: N/A\nAuthor: N/A\nHighlights:\nSecond desc";

        let results = ExaSearchEngine::parse_text_results(text);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First");
        assert_eq!(results[0].url, "https://first.com");
        assert_eq!(results[1].title, "Second");
        assert_eq!(results[1].url, "https://second.com");
    }

    #[test]
    fn test_parse_text_results_empty_url_skipped() {
        let text = "Title: No URL\nURL: \nPublished: N/A";

        let results = ExaSearchEngine::parse_text_results(text);
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_text_results_empty_input() {
        let results = ExaSearchEngine::parse_text_results("");
        assert!(results.is_empty());
    }

    // ========== SSE 解析 ==========

    #[test]
    fn test_parse_sse_to_json() {
        let body = "event: message\ndata: {\"result\":{\"content\":[]},\"id\":1}\n\n";
        let value = ExaSearchEngine::parse_sse_to_json(body).unwrap();
        assert!(value.get("result").is_some());
    }

    #[test]
    fn test_parse_sse_no_data_lines() {
        let body = "event: message\n\n";
        let result = ExaSearchEngine::parse_sse_to_json(body);
        assert!(result.is_err());
    }

    // ========== MCP 响应提取 ==========

    #[test]
    fn test_extract_results_text_format() {
        let response = json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "Title: Test\nURL: https://test.com\nPublished: N/A\nAuthor: N/A\nHighlights:\nTest description"
                }]
            },
            "id": 2
        });

        let results = ExaSearchEngine::extract_results_from_response(&response);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Test");
        assert_eq!(results[0].url, "https://test.com");
    }

    #[test]
    fn test_extract_results_json_format_fallback() {
        let response = json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "{\"results\": [{\"title\": \"JSON Result\", \"url\": \"https://json.com\", \"text\": \"desc\"}]}"
                }]
            },
            "id": 2
        });

        let results = ExaSearchEngine::extract_results_from_response(&response);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "JSON Result");
        assert_eq!(results[0].url, "https://json.com");
    }

    #[test]
    fn test_extract_results_empty_content() {
        let response = json!({"result": {"content": []}, "id": 1});
        let results = ExaSearchEngine::extract_results_from_response(&response);
        assert!(results.is_empty());
    }

    #[test]
    fn test_extract_results_no_result() {
        let response = json!({"id": 1});
        let results = ExaSearchEngine::extract_results_from_response(&response);
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
    async fn test_extract_results_from_real_exa_response() {
        // 真实 Exa MCP 响应的 JSON 结构（从 SSE data 行提取）
        let response = json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "Title: Rust Lang\nURL: https://rust-lang.org\nPublished: N/A\nAuthor: N/A\nHighlights:\nRust is blazingly fast\n...\n\n---\n\nTitle: Rust Book\nURL: https://doc.rust-lang.org/book/\nPublished: N/A\nAuthor: N/A\nHighlights:\nThe Rust Programming Language"
                }]
            },
            "jsonrpc": "2.0",
            "id": 2
        });

        let results = ExaSearchEngine::extract_results_from_response(&response);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Lang");
        assert_eq!(results[0].url, "https://rust-lang.org");
        assert!(results[0].description.contains("Rust is blazingly fast"));
        assert_eq!(results[1].title, "Rust Book");
        assert_eq!(results[1].url, "https://doc.rust-lang.org/book/");
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

    // ========== parse_generic_result_item ==========

    #[test]
    fn test_parse_generic_result_item_with_all_fields() {
        let item = json!({
            "title": "Test Title",
            "url": "https://example.com",
            "description": "Test description"
        });
        let result = parse_generic_result_item(&item);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.title, "Test Title");
        assert_eq!(r.url, "https://example.com");
        assert_eq!(r.description, "Test description");
    }

    #[test]
    fn test_parse_generic_result_item_empty_url_returns_none() {
        let item = json!({"title": "No URL", "url": ""});
        assert!(parse_generic_result_item(&item).is_none());
    }

    #[test]
    fn test_parse_generic_result_item_fallback_to_text_and_snippet() {
        let item_text = json!({"url": "https://a.com", "text": "from text field"});
        let r = parse_generic_result_item(&item_text).unwrap();
        assert_eq!(r.description, "from text field");

        let item_snippet = json!({"url": "https://b.com", "snippet": "from snippet"});
        let r = parse_generic_result_item(&item_snippet).unwrap();
        assert_eq!(r.description, "from snippet");
    }

    // ========== extract_results JSON array fallback ==========

    #[test]
    fn test_extract_results_json_array_format() {
        let response = json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "[{\"title\": \"Arr Item\", \"url\": \"https://arr.com\", \"description\": \"arr desc\"}]"
                }]
            }
        });
        let results = ExaSearchEngine::extract_results_from_response(&response);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Arr Item");
        assert_eq!(results[0].url, "https://arr.com");
    }

    #[test]
    fn test_extract_results_empty_text_skipped() {
        let response = json!({
            "result": {
                "content": [{"type": "text", "text": ""}]
            }
        });
        let results = ExaSearchEngine::extract_results_from_response(&response);
        assert!(results.is_empty());
    }

    // ========== 描述截断 ==========

    #[test]
    fn test_parse_text_results_description_truncated_at_500() {
        let long_desc = "x".repeat(600);
        let text = format!(
            "Title: Truncated\nURL: https://trunc.com\nHighlights:\n{}",
            long_desc
        );
        let results = ExaSearchEngine::parse_text_results(&text);
        assert_eq!(results.len(), 1);
        assert!(
            results[0].description.len() <= 503,
            "description should be truncated to ~500 chars + '...'"
        );
        assert!(results[0].description.ends_with("..."));
    }

    // ========== SSE 解析：多 data 行取最后一条 ==========

    #[test]
    fn test_parse_sse_multiple_data_lines_takes_last() {
        let body = "data: {\"a\":1}\ndata: {\"b\":2}\n\n";
        let value = ExaSearchEngine::parse_sse_to_json(body).unwrap();
        assert!(value.get("b").is_some(), "should use last data line");
    }

    #[test]
    fn test_parse_sse_invalid_json_returns_error() {
        let body = "data: not-json\n\n";
        assert!(ExaSearchEngine::parse_sse_to_json(body).is_err());
    }
}
