// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::engine_client::{
    EngineClient, ScrapeOptions, ScrapeRequest as EngineScrapeRequest,
};
use crate::search::{
    client::html_parser::HtmlParser,
    engine_trait::SearchEngine,
    error::SearchError,
    response::{Response, ResponseItem},
    types::{EngineHealth, SearchEngineType},
    SearchRequest,
};
use async_trait::async_trait;
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;

/// Baidu Search Categories
#[derive(Debug, Clone, Copy)]
pub enum BaiduSearchCategory {
    General,
    Images,
    News,
}

/// Baidu Search Engine implementation with EngineClient support
pub struct BaiduSearchEngine {
    parser: HtmlParser,
    engine_client: Arc<EngineClient>,
}

impl BaiduSearchEngine {
    pub fn new(engine_client: Arc<EngineClient>) -> Self {
        Self {
            parser: HtmlParser::for_baidu(),
            engine_client,
        }
    }

    pub fn build_baidu_url(
        &self,
        query: &str,
        page: u32,
        category: BaiduSearchCategory,
    ) -> (String, HashMap<String, String>) {
        let mut params = HashMap::with_capacity(8);
        let offset = ((page - 1) * 10).to_string();

        match category {
            BaiduSearchCategory::General => {
                params.insert("wd".to_string(), query.to_string());
                params.insert("rn".to_string(), "10".to_string());
                params.insert("pn".to_string(), offset);
                params.insert("tn".to_string(), "json".to_string());
                ("https://www.baidu.com/s".to_string(), params)
            }
            BaiduSearchCategory::Images => {
                params.insert("word".to_string(), query.to_string());
                params.insert("tn".to_string(), "resultjson_com".to_string());
                params.insert("pn".to_string(), offset);
                params.insert("rn".to_string(), "30".to_string()); // Images usually have more results
                ("https://image.baidu.com/search/acjson".to_string(), params)
            }
            _ => {
                // Fallback to general
                params.insert("wd".to_string(), query.to_string());
                ("https://www.baidu.com/s".to_string(), params)
            }
        }
    }

    pub fn parse_baidu_response(&self, json_str: &str) -> Result<Vec<ResponseItem>, SearchError> {
        let json: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| SearchError::Parse(format!("Failed to parse Baidu JSON: {}", e)))?;

        let mut results = Vec::new();

        if let Some(entry_array) = json
            .get("feed")
            .and_then(|f: &serde_json::Value| f.get("entry"))
            .and_then(|e: &serde_json::Value| e.as_array())
        {
            for entry in entry_array {
                let title = entry
                    .get("title")
                    .and_then(|t: &serde_json::Value| t.as_str())
                    .unwrap_or_default()
                    .to_string();
                let url = entry
                    .get("url")
                    .and_then(|u: &serde_json::Value| u.as_str())
                    .unwrap_or_default()
                    .to_string();
                let description = entry
                    .get("abs")
                    .and_then(|a: &serde_json::Value| a.as_str())
                    .unwrap_or_default()
                    .to_string();

                if !title.is_empty() && !url.is_empty() {
                    results.push(ResponseItem {
                        // Use html-escape to safely encode HTML entities, preventing XSS attacks
                        title: html_escape::encode_text(&title).trim().to_string(),
                        url,
                        description: html_escape::encode_text(&description).trim().to_string(),
                        engine: SearchEngineType::Baidu,
                    });
                }
            }
        }

        Ok(results)
    }

    pub async fn parse_search_results(
        &self,
        html: &str,
        _query: &str,
    ) -> Result<Vec<ResponseItem>, SearchError> {
        Ok(self.parser.parse(html, SearchEngineType::Baidu))
    }
}

#[async_trait]
impl SearchEngine for BaiduSearchEngine {
    fn name(&self) -> &'static str {
        "Baidu"
    }
    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Baidu
    }
    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        if std::env::var("BAIDU_TEST_RESULTS").unwrap_or_default() == "true" {
            let escaped_query = html_escape::encode_text(&request.query);
            return Ok(Response {
                items: vec![
                    ResponseItem {
                        title: format!("Baidu Test Result 1 for {}", escaped_query),
                        url: "https://baidu.com/1".to_string(),
                        description: "Test description 1".to_string(),
                        engine: SearchEngineType::Baidu,
                    },
                    ResponseItem {
                        title: format!("Baidu Test Result 2 for {}", escaped_query),
                        url: "https://baidu.com/2".to_string(),
                        description: "Test description 2".to_string(),
                        engine: SearchEngineType::Baidu,
                    },
                ],
                total_results: Some(2),
                engine: SearchEngineType::Baidu,
            });
        }

        let (url, params) = self.build_baidu_url(&request.query, 1, BaiduSearchCategory::General);

        // 构建带查询参数的完整 URL
        let full_url = if !params.is_empty() {
            format!(
                "{}?{}",
                url,
                serde_urlencoded::to_string(&params).unwrap_or_default()
            )
        } else {
            url
        };

        // 构建请求头
        let mut headers = HashMap::new();
        headers.insert(
            "Accept".to_string(),
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8"
                .to_string(),
        );
        headers.insert(
            "Accept-Language".to_string(),
            "zh-CN,zh;q=0.9,en;q=0.8".to_string(),
        );
        headers.insert("DNT".to_string(), "1".to_string());
        headers.insert("Connection".to_string(), "keep-alive".to_string());

        // 使用 EngineClient 进行请求
        let options = ScrapeOptions {
            headers,
            timeout: std::time::Duration::from_secs(30),
            ..Default::default()
        };

        let engine_request = EngineScrapeRequest {
            url: full_url,
            options,
        };

        let scrape_response = self
            .engine_client
            .scrape(&engine_request)
            .await
            .map_err(|e| SearchError::Engine(format!("EngineClient error: {}", e)))?;

        if scrape_response.status_code < 200 || scrape_response.status_code >= 300 {
            return Err(SearchError::Engine(format!(
                "Baidu search error: {}",
                scrape_response.status_code
            )));
        }

        let content = scrape_response.content;

        // Try parsing as HTML (Baidu returns HTML by default now)
        let items = self.parse_search_results(&content, &request.query).await?;

        // If items are empty, try JSON as fallback (though unlikely with current URL)
        let items = if items.is_empty() {
            if let Ok(json_items) = self.parse_baidu_response(&content) {
                if !json_items.is_empty() {
                    json_items
                } else {
                    items
                }
            } else {
                items
            }
        } else {
            items
        };

        let total_count = items.len() as u64;

        Ok(Response {
            items,
            total_results: Some(total_count),
            engine: SearchEngineType::Baidu,
        })
    }
}

#[cfg(test)]
mod tests {
    // Copyright (c) 2025 Kirky.X
    // Licensed under the Apache License, Version 2.0
    // See LICENSE file in the project root for full license information.

    use super::*;
    use crate::engines::engine_client::EngineClient;
    use std::sync::Arc;

    // 辅助函数：创建测试用的 BaiduSearchEngine 实例
    fn create_engine() -> BaiduSearchEngine {
        let engine_client = Arc::new(EngineClient::new());
        BaiduSearchEngine::new(engine_client)
    }

    // ========== build_baidu_url 测试 ==========

    #[test]
    fn test_build_baidu_url_general_page1() {
        // 测试通用搜索第一页的 URL 和参数构建
        let engine = create_engine();
        let (url, params) = engine.build_baidu_url("rust 语言", 1, BaiduSearchCategory::General);

        assert_eq!(url, "https://www.baidu.com/s");
        assert_eq!(params.get("wd"), Some(&"rust 语言".to_string()));
        assert_eq!(params.get("rn"), Some(&"10".to_string()));
        assert_eq!(params.get("pn"), Some(&"0".to_string()));
        assert_eq!(params.get("tn"), Some(&"json".to_string()));
    }

    #[test]
    fn test_build_baidu_url_general_page3_offset() {
        // 测试页码对应的偏移量计算：page=3 → pn=20
        let engine = create_engine();
        let (_url, params) = engine.build_baidu_url("test", 3, BaiduSearchCategory::General);

        assert_eq!(params.get("pn"), Some(&"20".to_string()));
    }

    #[test]
    fn test_build_baidu_url_images_category() {
        // 测试图片搜索分类的 URL 和参数
        let engine = create_engine();
        let (url, params) = engine.build_baidu_url("cat", 1, BaiduSearchCategory::Images);

        assert_eq!(url, "https://image.baidu.com/search/acjson");
        assert_eq!(params.get("word"), Some(&"cat".to_string()));
        assert_eq!(params.get("tn"), Some(&"resultjson_com".to_string()));
        assert_eq!(params.get("rn"), Some(&"30".to_string()));
        assert_eq!(params.get("pn"), Some(&"0".to_string()));
    }

    #[test]
    fn test_build_baidu_url_news_falls_back_to_general() {
        // 测试 News 分类回退到通用搜索（仅包含 wd 参数）
        let engine = create_engine();
        let (url, params) = engine.build_baidu_url("news", 1, BaiduSearchCategory::News);

        assert_eq!(url, "https://www.baidu.com/s");
        assert_eq!(params.get("wd"), Some(&"news".to_string()));
        assert!(params.get("rn").is_none(), "fallback should not set rn");
    }

    #[test]
    fn test_build_baidu_url_empty_query() {
        // 边界情况：空查询字符串仍然构建有效 URL
        let engine = create_engine();
        let (url, params) = engine.build_baidu_url("", 1, BaiduSearchCategory::General);

        assert_eq!(url, "https://www.baidu.com/s");
        assert_eq!(params.get("wd"), Some(&"".to_string()));
    }

    // ========== parse_baidu_response 测试 ==========

    #[test]
    fn test_parse_baidu_response_valid_json() {
        // 测试有效 JSON 响应解析出多个结果
        let engine = create_engine();
        let json = r#"{
            "feed": {
                "entry": [
                    {"title": "Result 1", "url": "https://example.com/1", "abs": "Desc 1"},
                    {"title": "Result 2", "url": "https://example.com/2", "abs": "Desc 2"}
                ]
            }
        }"#;

        let results = engine.parse_baidu_response(json).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Result 1");
        assert_eq!(results[0].url, "https://example.com/1");
        assert_eq!(results[0].description, "Desc 1");
        assert_eq!(results[0].engine, SearchEngineType::Baidu);
        assert_eq!(results[1].title, "Result 2");
    }

    #[test]
    fn test_parse_baidu_response_invalid_json() {
        // 测试无效 JSON 返回 Parse 错误
        let engine = create_engine();
        let result = engine.parse_baidu_response("not a valid json {{{");
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SearchError::Parse(msg) => assert!(msg.contains("Failed to parse Baidu JSON")),
            other => panic!("expected SearchError::Parse, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_baidu_response_skips_empty_title_or_url() {
        // 边界情况：空 title 或空 url 的条目应被跳过
        let engine = create_engine();
        let json = r#"{
            "feed": {
                "entry": [
                    {"title": "", "url": "https://example.com/1", "abs": "Desc 1"},
                    {"title": "Result 2", "url": "", "abs": "Desc 2"},
                    {"title": "Valid", "url": "https://example.com/3", "abs": "Desc 3"}
                ]
            }
        }"#;

        let results = engine.parse_baidu_response(json).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Valid");
        assert_eq!(results[0].url, "https://example.com/3");
    }

    #[test]
    fn test_parse_baidu_response_no_feed_entry() {
        // 边界情况：JSON 不包含 feed/entry 字段时返回空结果
        let engine = create_engine();
        let json = r#"{"other": "data"}"#;
        let results = engine.parse_baidu_response(json).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_baidu_response_html_entities_encoded() {
        // 测试 HTML 实体被编码以防止 XSS
        let engine = create_engine();
        let json = r#"{
            "feed": {
                "entry": [
                    {"title": "<script>alert(1)</script>", "url": "https://example.com", "abs": "a & b <c>"}
                ]
            }
        }"#;

        let results = engine.parse_baidu_response(json).unwrap();
        assert_eq!(results.len(), 1);
        // 编码后不应包含原始的 <script> 标签
        assert!(!results[0].title.contains("<script>"));
        assert!(results[0].title.contains("&lt;script&gt;"));
        assert!(!results[0].description.contains("<c>"));
    }

    // ========== parse_search_results 测试 ==========

    #[tokio::test]
    async fn test_parse_search_results_valid_html() {
        // 测试从有效百度 HTML 中解析搜索结果
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div class="c-container">
                <h3><a href="https://example.com/1">First Result</a></h3>
                <div class="c-abstract">First description</div>
            </div>
            <div class="c-container">
                <h3><a href="https://example.com/2">Second Result</a></h3>
                <div class="c-abstract">Second description</div>
            </div>
        </body></html>
        "#;

        let results = engine.parse_search_results(html, "test").await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Result");
        assert_eq!(results[0].url, "https://example.com/1");
        assert_eq!(results[0].engine, SearchEngineType::Baidu);
        assert_eq!(results[1].title, "Second Result");
    }

    #[tokio::test]
    async fn test_parse_search_results_empty_html() {
        // 边界情况：空 HTML 返回空结果
        let engine = create_engine();
        let results = engine.parse_search_results("", "test").await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_parse_search_results_no_matching_selectors() {
        // 边界情况：HTML 不包含百度结果选择器时返回空
        let engine = create_engine();
        let html = r#"<html><body><div>no results here</div></body></html>"#;
        let results = engine.parse_search_results(html, "test").await.unwrap();
        assert!(results.is_empty());
    }

    // ========== SearchEngine trait 方法测试 ==========

    #[test]
    fn test_engine_creation() {
        // Verify engine construction and trait accessors
        let engine = create_engine();
        assert_eq!(engine.name(), "Baidu");
        assert_eq!(engine.engine_type(), SearchEngineType::Baidu);
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    // ========== search() test fallback path ==========

    /// Mutex to serialize tests that mutate process-level environment
    /// variables (std::env::set_var is not thread-safe across tests).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn make_search_request(query: &str) -> SearchRequest {
        SearchRequest::new(query)
    }

    #[tokio::test]
    async fn test_search_fallback_returns_hardcoded_results() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("BAIDU_TEST_RESULTS", "true");

        let engine = create_engine();
        let request = make_search_request("rust programming");

        let response = engine.search(&request).await;

        // Clean up env var ASAP to minimize cross-test interference
        std::env::remove_var("BAIDU_TEST_RESULTS");

        let response = response.expect("fallback should return Ok");
        assert_eq!(response.items.len(), 2, "fallback should return 2 items");
        assert_eq!(response.engine, SearchEngineType::Baidu);
        assert_eq!(response.total_results, Some(2));
        assert!(response.items[0].title.contains("rust programming"));
        assert!(response.items[1].title.contains("rust programming"));
        assert_eq!(response.items[0].url, "https://baidu.com/1");
        assert_eq!(response.items[1].url, "https://baidu.com/2");
        assert_eq!(response.items[0].engine, SearchEngineType::Baidu);
        assert_eq!(response.items[1].engine, SearchEngineType::Baidu);
    }

    #[tokio::test]
    async fn test_search_fallback_escapes_query_in_title() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("BAIDU_TEST_RESULTS", "true");

        let engine = create_engine();
        // Query with HTML special characters that should be escaped
        let request = make_search_request("<script>alert(1)</script>");

        let response = engine.search(&request).await;

        std::env::remove_var("BAIDU_TEST_RESULTS");

        let response = response.expect("fallback should return Ok");
        // The title should contain the escaped query, not raw HTML
        assert!(response.items[0].title.contains("&lt;script&gt;"));
        assert!(!response.items[0].title.contains("<script>"));
        assert!(response.items[1].title.contains("&lt;script&gt;"));
        assert!(!response.items[1].title.contains("<script>"));
    }

    #[tokio::test]
    async fn test_search_fallback_description_and_engine_fields() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("BAIDU_TEST_RESULTS", "true");

        let engine = create_engine();
        let request = make_search_request("fields test");

        let response = engine.search(&request).await;

        std::env::remove_var("BAIDU_TEST_RESULTS");

        let response = response.expect("fallback should return Ok");
        assert_eq!(response.items[0].description, "Test description 1");
        assert_eq!(response.items[1].description, "Test description 2");
        assert_eq!(response.items[0].engine, SearchEngineType::Baidu);
        assert_eq!(response.items[1].engine, SearchEngineType::Baidu);
    }
}
