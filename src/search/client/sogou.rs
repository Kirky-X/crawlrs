// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::engine_client::{
    EngineClient, ScrapeOptions, ScrapeRequest as EngineScrapeRequest,
};
use crate::search::{
    engine_trait::SearchEngine,
    error::SearchError,
    response::{Response, ResponseItem},
    types::{EngineHealth, SearchEngineType},
    SearchRequest,
};
use async_trait::async_trait;
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::sync::Arc;

/// 安全解析CSS选择器，如果解析失败则返回None
fn safe_parse_selector(selector_str: &str) -> Option<Selector> {
    Selector::parse(selector_str).ok()
}

/// Sogou Search Engine implementation with EngineClient support
pub struct SogouSearchEngine {
    engine_client: Arc<EngineClient>,
}

impl SogouSearchEngine {
    pub fn new(engine_client: Arc<EngineClient>) -> Self {
        Self { engine_client }
    }

    /// 解析并补全搜狗搜索结果中的URL
    /// 处理相对路径和中转URL格式
    pub fn resolve_url(&self, url: &str) -> String {
        if url.is_empty() {
            return String::new();
        }

        // 处理直接完整URL (仅允许 http/https)
        if url.starts_with("http://") || url.starts_with("https://") {
            // 验证 URL 格式并检查协议
            if let Ok(parsed) = url::Url::parse(url) {
                // 只允许 http/https 协议
                if parsed.scheme() == "http" || parsed.scheme() == "https" {
                    return url.to_string();
                }
            }
            return String::new();
        }

        // 处理搜狗中转链接: /link?url=...
        if url.starts_with("/link?url=") {
            // 提取参数中的URL并解码
            let encoded_url = url.trim_start_matches("/link?url=");
            // URL解码
            match urlencoding::decode(encoded_url) {
                Ok(decoded) => {
                    // 递归验证解码后的 URL
                    return self.resolve_url(&decoded);
                }
                Err(_) => return String::new(),
            };
        }

        // 处理其他相对路径 (仅搜狗域名)
        if url.starts_with("/") {
            return format!("https://www.sogou.com{}", url);
        }

        // 其他情况拒绝
        String::new()
    }

    pub fn parse_search_results(
        &self,
        html_content: &str,
    ) -> Result<Vec<ResponseItem>, SearchError> {
        let document = Html::parse_document(html_content);
        let result_selector =
            safe_parse_selector(".vrwrap, .rb").expect("Failed to parse Sogou result selector");
        let title_selector =
            safe_parse_selector("h3").expect("Failed to parse Sogou title selector");
        let link_selector =
            safe_parse_selector("h3 a").expect("Failed to parse Sogou link selector");

        let mut results = Vec::new();

        for element in document.select(&result_selector) {
            // 提取标题 - 获取纯文本并清理空白
            let title_node = element.select(&title_selector).next();
            let raw_title = match title_node {
                Some(node) => node.text().collect::<String>(),
                None => continue,
            };
            let title = html_escape::encode_text(raw_title.trim()).to_string();

            if title.is_empty() {
                continue;
            }

            // 提取链接
            let url_node = element.select(&link_selector).next();
            let raw_url = match url_node {
                Some(node) => node.value().attr("href").unwrap_or("").to_string(),
                None => continue,
            };

            // 解析并补全URL
            let resolved_url = self.resolve_url(&raw_url);

            if !resolved_url.is_empty() {
                results.push(ResponseItem {
                    title,
                    url: resolved_url,
                    description: String::new(),
                    engine: SearchEngineType::Sogou,
                });
            }
        }

        Ok(results)
    }
}

#[async_trait]
impl SearchEngine for SogouSearchEngine {
    fn name(&self) -> &'static str {
        "Sogou"
    }
    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Sogou
    }
    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        if std::env::var("SOGOU_TEST_RESULTS").unwrap_or_default() == "true" {
            let escaped_query = html_escape::encode_text(&request.query);
            return Ok(Response {
                items: vec![
                    ResponseItem {
                        title: format!("Test Result 1 for {}", escaped_query),
                        url: "https://sogou.com/1".to_string(),
                        description: "Test description 1".to_string(),
                        engine: SearchEngineType::Sogou,
                    },
                    ResponseItem {
                        title: format!("Test Result 2 for {}", escaped_query),
                        url: "https://sogou.com/2".to_string(),
                        description: "Test description 2".to_string(),
                        engine: SearchEngineType::Sogou,
                    },
                ],
                total_results: Some(2),
                engine: SearchEngineType::Sogou,
            });
        }

        let base_url = "https://www.sogou.com/web";

        // 构建带查询参数的完整 URL
        let full_url = format!(
            "{}?query={}&num={}",
            base_url,
            urlencoding::encode(&request.query),
            request.limit
        );

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
                "Sogou search error: {}",
                scrape_response.status_code
            )));
        }

        let html_content = scrape_response.content;
        let items = self.parse_search_results(&html_content)?;
        let total_count = items.len() as u64;

        Ok(Response {
            items,
            total_results: Some(total_count),
            engine: SearchEngineType::Sogou,
        })
    }
}

#[cfg(test)]
mod tests {
    // Copyright (c) 2025 Kirky.X
    // Licensed under the Apache License, Version 2.0
    use super::*;

    // 创建测试用 SogouSearchEngine
    fn make_engine() -> SogouSearchEngine {
        SogouSearchEngine::new(Arc::new(EngineClient::new()))
    }

    // ========== safe_parse_selector 测试 ==========

    #[test]
    fn test_safe_parse_selector_valid() {
        let result = safe_parse_selector(".vrwrap");
        assert!(result.is_some(), "valid CSS selector should parse");
    }

    #[test]
    fn test_safe_parse_selector_invalid() {
        let result = safe_parse_selector(">>>invalid<<<");
        assert!(result.is_none(), "invalid CSS selector should return None");
    }

    // ========== resolve_url 测试 ==========

    #[test]
    fn test_resolve_url_empty() {
        let engine = make_engine();
        assert_eq!(engine.resolve_url(""), "");
    }

    #[test]
    fn test_resolve_url_https() {
        let engine = make_engine();
        let url = "https://example.com/page";
        assert_eq!(engine.resolve_url(url), url);
    }

    #[test]
    fn test_resolve_url_http() {
        let engine = make_engine();
        let url = "http://example.com/page";
        assert_eq!(engine.resolve_url(url), url);
    }

    #[test]
    fn test_resolve_url_invalid_scheme_rejected() {
        let engine = make_engine();
        // ftp:// 开头的 URL 虽然以 http 开头匹配但会解析失败，返回空
        assert_eq!(engine.resolve_url("ftp://example.com"), "");
    }

    #[test]
    fn test_resolve_url_malformed_http_rejected() {
        let engine = make_engine();
        // "http://" 开头但无法解析的 URL 应返回空
        assert_eq!(engine.resolve_url("http://"), "");
    }

    #[test]
    fn test_resolve_url_sogou_link_redirect() {
        let engine = make_engine();
        // 搜狗中转链接，包含完整 URL
        let url = "/link?url=https%3A%2F%2Fwww.example.com%2Fpage";
        let resolved = engine.resolve_url(url);
        assert_eq!(resolved, "https://www.example.com/page");
    }

    #[test]
    fn test_resolve_url_relative_path() {
        let engine = make_engine();
        let url = "/web?query=test";
        let resolved = engine.resolve_url(url);
        assert_eq!(resolved, "https://www.sogou.com/web?query=test");
    }

    #[test]
    fn test_resolve_url_non_http_non_relative_rejected() {
        let engine = make_engine();
        // 既不是 http/https 开头，也不是 / 开头，应返回空
        assert_eq!(engine.resolve_url("just-some-text"), "");
    }

    // ========== parse_search_results 测试 ==========

    #[test]
    fn test_parse_search_results_valid_html() {
        let engine = make_engine();
        let html = r#"
        <div class="vrwrap">
            <h3><a href="https://example.com/result1">Example Result 1</a></h3>
        </div>
        <div class="vrwrap">
            <h3><a href="https://example.com/result2">Example Result 2</a></h3>
        </div>
        "#;
        let results = engine.parse_search_results(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Example Result 1");
        assert_eq!(results[0].url, "https://example.com/result1");
        assert_eq!(results[0].engine, SearchEngineType::Sogou);
        assert_eq!(results[1].title, "Example Result 2");
    }

    #[test]
    fn test_parse_search_results_rb_class() {
        let engine = make_engine();
        // .rb 也是一个结果容器选择器
        let html = r#"
        <div class="rb">
            <h3><a href="https://example.com/rb-result">RB Result</a></h3>
        </div>
        "#;
        let results = engine.parse_search_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "RB Result");
    }

    #[test]
    fn test_parse_search_results_empty_html() {
        let engine = make_engine();
        let results = engine.parse_search_results("").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_search_results_no_results() {
        let engine = make_engine();
        let html = "<html><body><p>No results here</p></body></html>";
        let results = engine.parse_search_results(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_search_results_missing_title_skipped() {
        let engine = make_engine();
        // 没有 h3 标题的 vrwrap 应被跳过
        let html = r#"
        <div class="vrwrap">
            <a href="https://example.com/no-title">No Title</a>
        </div>
        <div class="vrwrap">
            <h3><a href="https://example.com/with-title">With Title</a></h3>
        </div>
        "#;
        let results = engine.parse_search_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "With Title");
    }

    #[test]
    fn test_parse_search_results_empty_title_skipped() {
        let engine = make_engine();
        // 空标题应被跳过
        let html = r#"
        <div class="vrwrap">
            <h3>   </h3>
            <a href="https://example.com/empty-title">Empty Title</a>
        </div>
        "#;
        let results = engine.parse_search_results(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_search_results_sogou_redirect_link() {
        let engine = make_engine();
        // 搜狗中转链接应被正确解析
        let html = r#"
        <div class="vrwrap">
            <h3><a href="/link?url=https%3A%2F%2Fwww.example.com%2Farticle">Redirect Result</a></h3>
        </div>
        "#;
        let results = engine.parse_search_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://www.example.com/article");
    }

    #[test]
    fn test_parse_search_results_empty_url_skipped() {
        let engine = make_engine();
        // href 为空或 resolve_url 返回空时应跳过
        let html = r#"
        <div class="vrwrap">
            <h3><a>No href</a></h3>
        </div>
        "#;
        let results = engine.parse_search_results(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_search_results_description_is_empty() {
        let engine = make_engine();
        let html = r#"
        <div class="vrwrap">
            <h3><a href="https://example.com/test">Test</a></h3>
        </div>
        "#;
        let results = engine.parse_search_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "");
    }

    // ========== SearchEngine trait 方法测试 ==========

    #[test]
    fn test_name() {
        let engine = make_engine();
        assert_eq!(engine.name(), "Sogou");
    }

    #[test]
    fn test_engine_type() {
        let engine = make_engine();
        assert_eq!(engine.engine_type(), SearchEngineType::Sogou);
    }

    #[test]
    fn test_health() {
        let engine = make_engine();
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
    async fn test_search_with_test_results_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("SOGOU_TEST_RESULTS", "true");
        let engine = make_engine();
        let request = make_search_request("test query");
        let result = engine.search(&request).await;
        std::env::remove_var("SOGOU_TEST_RESULTS");

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.items.len(), 2);
        assert!(response.items[0].title.contains("test query"));
        assert_eq!(response.engine, SearchEngineType::Sogou);
        assert_eq!(response.total_results, Some(2));
    }

    #[tokio::test]
    async fn test_search_fallback_returns_correct_urls_and_engine() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("SOGOU_TEST_RESULTS", "true");

        let engine = make_engine();
        let request = make_search_request("url check");

        let response = engine.search(&request).await;

        std::env::remove_var("SOGOU_TEST_RESULTS");

        let response = response.expect("fallback should return Ok");
        assert_eq!(response.items.len(), 2);
        assert_eq!(response.items[0].url, "https://sogou.com/1");
        assert_eq!(response.items[1].url, "https://sogou.com/2");
        assert_eq!(response.items[0].engine, SearchEngineType::Sogou);
        assert_eq!(response.items[1].engine, SearchEngineType::Sogou);
        assert_eq!(response.items[0].description, "Test description 1");
        assert_eq!(response.items[1].description, "Test description 2");
    }

    #[tokio::test]
    async fn test_search_fallback_escapes_query_in_title() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("SOGOU_TEST_RESULTS", "true");

        let engine = make_engine();
        // Query with HTML special characters that should be escaped
        let request = make_search_request("<script>alert(1)</script>");

        let response = engine.search(&request).await;

        std::env::remove_var("SOGOU_TEST_RESULTS");

        let response = response.expect("fallback should return Ok");
        // The title should contain the escaped query, not raw HTML
        assert!(response.items[0].title.contains("&lt;script&gt;"));
        assert!(!response.items[0].title.contains("<script>"));
        assert!(response.items[1].title.contains("&lt;script&gt;"));
        assert!(!response.items[1].title.contains("<script>"));
    }
}
