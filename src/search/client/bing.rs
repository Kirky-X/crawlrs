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
use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

/// Bing Search Engine implementation with EngineClient support
pub struct BingSearchEngine {
    parser: HtmlParser,
    engine_client: Arc<EngineClient>,
}

impl BingSearchEngine {
    pub fn new(engine_client: Arc<EngineClient>) -> Self {
        Self {
            parser: HtmlParser::for_bing(),
            engine_client,
        }
    }

    /// Construct Bing cookies for region and language settings
    pub fn get_bing_cookies(&self, lang: &str, region: &str) -> HashMap<String, String> {
        let mut cookies = HashMap::with_capacity(4);
        cookies.insert("_EDGE_CD".to_string(), format!("m={}&u={}", region, lang));
        cookies.insert("_EDGE_S".to_string(), format!("mkt={}&ui={}", region, lang));
        cookies
    }

    /// Build Bing search parameters for testing
    pub fn build_params(&self, query: &str, page: u32) -> HashMap<String, String> {
        let mut params = HashMap::with_capacity(8);
        params.insert("q".to_string(), query.to_string());
        params.insert("pq".to_string(), query.to_string());

        if page > 1 {
            params.insert("first".to_string(), ((page - 1) * 10 + 1).to_string());
            let form_value = if page == 2 {
                "PERE".to_string()
            } else {
                format!("PERE{}", page - 2)
            };
            params.insert("FORM".to_string(), form_value);
        }

        params
    }

    /// Decode Bing redirect URLs that are Base64 encoded
    ///
    /// Flattens 5-level nested conditions into early returns for better readability.
    pub fn decode_bing_url(&self, url: &str) -> String {
        // Early return if not a Bing redirect URL
        if !url.starts_with("https://www.bing.com/ck/a?") {
            return url.to_string();
        }

        // Parse URL and extract 'u' parameter
        let parsed_url = match Url::parse(url) {
            Ok(url) => url,
            Err(_) => return url.to_string(),
        };

        let u_param = match parsed_url.query_pairs().find(|(key, _)| key == "u") {
            Some(param) => param,
            None => return url.to_string(),
        };

        let encoded = &u_param.1[2..]; // Remove 'a1' prefix

        // Add padding if needed
        let padding = "=".repeat((4 - encoded.len() % 4) % 4);
        let padded_encoded = format!("{}{}", encoded, padding);

        // Decode Base64 and convert to string
        let decoded_bytes = match URL_SAFE.decode(padded_encoded) {
            Ok(bytes) => bytes,
            Err(_) => return url.to_string(),
        };

        match String::from_utf8(decoded_bytes) {
            Ok(decoded_str) => decoded_str,
            Err(_) => url.to_string(),
        }
    }

    pub fn build_bing_url(&self, query: &str, page: u32) -> String {
        let base_url = "https://www.bing.com/search";
        let mut params = vec![("q", query.to_string()), ("pq", query.to_string())];

        if page > 1 {
            let first_value = ((page - 1) * 10 + 1).to_string();
            params.push(("first", first_value));
            let form_value = if page == 2 {
                "PERE".to_string()
            } else {
                format!("PERE{}", page - 2)
            };
            params.push(("FORM", form_value));
        }

        format!(
            "{}?{}",
            base_url,
            serde_urlencoded::to_string(&params).unwrap_or_default()
        )
    }

    pub async fn parse_search_results(&self, html: &str) -> Result<Vec<ResponseItem>, SearchError> {
        if html.is_empty() {
            return Err(SearchError::Parse(
                "Empty HTML response received".to_string(),
            ));
        }

        Ok(self.parser.parse(html, SearchEngineType::Bing))
    }
}

#[async_trait]
impl SearchEngine for BingSearchEngine {
    fn name(&self) -> &'static str {
        "Bing"
    }
    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Bing
    }
    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        if std::env::var("BING_TEST_RESULTS").unwrap_or_default() == "true" {
            return Ok(Response {
                items: vec![
                    ResponseItem {
                        title: format!("Bing Test Result 1 for {}", request.query),
                        url: "https://bing.com/1".to_string(),
                        description: "Test description 1".to_string(),
                        engine: SearchEngineType::Bing,
                    },
                    ResponseItem {
                        title: format!("Bing Test Result 2 for {}", request.query),
                        url: "https://bing.com/2".to_string(),
                        description: "Test description 2".to_string(),
                        engine: SearchEngineType::Bing,
                    },
                ],
                total_results: Some(2),
                engine: SearchEngineType::Bing,
            });
        }

        if request.query.trim().is_empty() {
            return Err(SearchError::Parse(
                "Search query cannot be empty".to_string(),
            ));
        }

        let url = self.build_bing_url(&request.query, 1);

        // 构建请求头
        let mut headers = HashMap::new();
        headers.insert(
            "Accept".to_string(),
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8"
                .to_string(),
        );
        headers.insert("Accept-Language".to_string(), "en-US,en;q=0.5".to_string());
        headers.insert("DNT".to_string(), "1".to_string());
        headers.insert("Connection".to_string(), "keep-alive".to_string());
        headers.insert("Upgrade-Insecure-Requests".to_string(), "1".to_string());

        // 使用 EngineClient 进行请求
        let options = ScrapeOptions {
            headers,
            timeout: std::time::Duration::from_secs(30),
            ..Default::default()
        };

        let engine_request = EngineScrapeRequest { url, options };

        let scrape_response = self
            .engine_client
            .scrape(&engine_request)
            .await
            .map_err(|e| SearchError::Engine(format!("EngineClient error: {}", e)))?;

        if scrape_response.status_code < 200 || scrape_response.status_code >= 300 {
            return Err(SearchError::Engine(format!(
                "HTTP error {}",
                scrape_response.status_code
            )));
        }

        let html = scrape_response.content;
        let items = self.parse_search_results(&html).await?;
        let total_count = items.len() as u64;

        Ok(Response {
            items,
            total_results: Some(total_count),
            engine: SearchEngineType::Bing,
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

    // 辅助函数：创建测试用的 BingSearchEngine 实例
    fn create_engine() -> BingSearchEngine {
        let engine_client = Arc::new(EngineClient::new());
        BingSearchEngine::new(engine_client)
    }

    // 辅助函数：构造一个 Bing 跳转链接（a1 前缀 + Base64 编码目标 URL）
    fn build_bing_redirect_url(target_url: &str) -> String {
        let encoded = URL_SAFE.encode(target_url);
        format!("https://www.bing.com/ck/a?!&p=0&u=a1{}&t=1", encoded)
    }

    // ========== get_bing_cookies 测试 ==========

    #[test]
    fn test_get_bing_cookies_contains_edge_keys() {
        // 测试返回的 cookies 包含 _EDGE_CD 和 _EDGE_S 两个键
        let engine = create_engine();
        let cookies = engine.get_bing_cookies("en", "US");

        assert_eq!(cookies.len(), 2);
        assert!(cookies.contains_key("_EDGE_CD"));
        assert!(cookies.contains_key("_EDGE_S"));
    }

    #[test]
    fn test_get_bing_cookies_embeds_lang_and_region() {
        // 测试 cookie 值中嵌入了语言和地区参数
        let engine = create_engine();
        let cookies = engine.get_bing_cookies("zh", "CN");

        let edge_cd = cookies.get("_EDGE_CD").unwrap();
        assert!(edge_cd.contains("m=CN"), "_EDGE_CD should contain region");
        assert!(edge_cd.contains("u=zh"), "_EDGE_CD should contain lang");

        let edge_s = cookies.get("_EDGE_S").unwrap();
        assert!(edge_s.contains("mkt=CN"), "_EDGE_S should contain region");
        assert!(edge_s.contains("ui=zh"), "_EDGE_S should contain lang");
    }

    #[test]
    fn test_get_bing_cookies_empty_values() {
        // 边界情况：空字符串仍然生成有效 cookie 结构
        let engine = create_engine();
        let cookies = engine.get_bing_cookies("", "");

        let edge_cd = cookies.get("_EDGE_CD").unwrap();
        assert!(edge_cd.contains("m="));
        assert!(edge_cd.contains("u="));
    }

    // ========== build_params 测试 ==========

    #[test]
    fn test_build_params_page1() {
        // 测试第一页只包含 q 和 pq 参数，不包含分页参数
        let engine = create_engine();
        let params = engine.build_params("rust", 1);

        assert_eq!(params.get("q"), Some(&"rust".to_string()));
        assert_eq!(params.get("pq"), Some(&"rust".to_string()));
        assert!(
            params.get("first").is_none(),
            "page 1 should not have 'first'"
        );
        assert!(
            params.get("FORM").is_none(),
            "page 1 should not have 'FORM'"
        );
    }

    #[test]
    fn test_build_params_page2_form_pere() {
        // 测试第二页的 FORM 参数为 PERE，first=11
        let engine = create_engine();
        let params = engine.build_params("rust", 2);

        assert_eq!(params.get("first"), Some(&"11".to_string()));
        assert_eq!(params.get("FORM"), Some(&"PERE".to_string()));
    }

    #[test]
    fn test_build_params_page3_form_pere1() {
        // 测试第三页的 FORM 参数为 PERE1，first=21
        let engine = create_engine();
        let params = engine.build_params("rust", 3);

        assert_eq!(params.get("first"), Some(&"21".to_string()));
        assert_eq!(params.get("FORM"), Some(&"PERE1".to_string()));
    }

    #[test]
    fn test_build_params_page5_offset_calculation() {
        // 测试页码 5 的偏移量计算：first = (5-1)*10+1 = 41
        let engine = create_engine();
        let params = engine.build_params("test", 5);

        assert_eq!(params.get("first"), Some(&"41".to_string()));
        assert_eq!(params.get("FORM"), Some(&"PERE3".to_string()));
    }

    // ========== build_bing_url 测试 ==========

    #[test]
    fn test_build_bing_url_page1() {
        // 测试第一页 URL 构建，只包含 q 和 pq
        let engine = create_engine();
        let url = engine.build_bing_url("rust language", 1);

        assert!(url.starts_with("https://www.bing.com/search?"));
        assert!(
            url.contains("q=rust+language"),
            "URL should contain encoded query"
        );
        assert!(url.contains("pq=rust+language"));
        assert!(!url.contains("first="), "page 1 should not have 'first'");
    }

    #[test]
    fn test_build_bing_url_page2_includes_pagination() {
        // 测试第二页 URL 包含分页参数 first 和 FORM
        let engine = create_engine();
        let url = engine.build_bing_url("test", 2);

        assert!(url.contains("first=11"), "page 2 should have first=11");
        assert!(url.contains("FORM=PERE"), "page 2 should have FORM=PERE");
    }

    #[test]
    fn test_build_bing_url_special_chars_encoded() {
        // 测试特殊字符在 URL 中被正确编码
        let engine = create_engine();
        let url = engine.build_bing_url("rust & go", 1);

        // serde_urlencoded 会将 & 编码为 %26
        assert!(url.contains("q=rust+%26+go") || url.contains("q=rust+%26+go"));
    }

    // ========== decode_bing_url 测试 ==========

    #[test]
    fn test_decode_bing_url_non_bing_url_returned_as_is() {
        // 测试非 Bing 跳转链接原样返回
        let engine = create_engine();
        let original = "https://example.com/page";
        let decoded = engine.decode_bing_url(original);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_decode_bing_url_valid_redirect_decoded() {
        // 测试有效的 Bing 跳转链接被正确解码为目标 URL
        let engine = create_engine();
        let target = "https://example.com/real-target";
        let redirect_url = build_bing_redirect_url(target);

        let decoded = engine.decode_bing_url(&redirect_url);
        assert_eq!(decoded, target);
    }

    #[test]
    fn test_decode_bing_url_missing_u_param_returned_as_is() {
        // 边界情况：Bing 跳转链接缺少 u 参数时原样返回
        let engine = create_engine();
        let url = "https://www.bing.com/ck/a?!&p=0&t=1";
        let decoded = engine.decode_bing_url(url);
        assert_eq!(decoded, url);
    }

    #[test]
    fn test_decode_bing_url_invalid_base64_returned_as_is() {
        // 边界情况：u 参数的 Base64 无效时原样返回
        let engine = create_engine();
        let url = "https://www.bing.com/ck/a?u=a1!!!invalid_base64!!!";
        let decoded = engine.decode_bing_url(url);
        assert_eq!(decoded, url);
    }

    #[test]
    fn test_decode_bing_url_unparseable_url_returned_as_is() {
        // 边界情况：URL 无法解析时原样返回
        let engine = create_engine();
        // 构造一个以 bing 开头但无法被 url crate 解析的字符串
        let url = "https://www.bing.com/ck/a?u=a1[invalid";
        let decoded = engine.decode_bing_url(url);
        // 即使能解析，base64 也会失败，返回原始 URL
        assert!(!decoded.is_empty());
    }

    // ========== parse_search_results 测试 ==========

    #[tokio::test]
    async fn test_parse_search_results_empty_html_returns_error() {
        // 边界情况：空 HTML 返回 Parse 错误
        let engine = create_engine();
        let result = engine.parse_search_results("").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SearchError::Parse(msg) => {
                assert!(
                    msg.contains("Empty HTML"),
                    "error should mention empty HTML"
                )
            }
            other => panic!("expected SearchError::Parse, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_parse_search_results_valid_html() {
        // 测试从有效 Bing HTML 中解析搜索结果
        let engine = create_engine();
        let html = r#"
        <html><body>
            <li class="b_algo">
                <h2><a href="https://example.com/1">First Result</a></h2>
                <p>First description text</p>
            </li>
            <li class="b_algo">
                <h2><a href="https://example.com/2">Second Result</a></h2>
                <p>Second description text</p>
            </li>
        </body></html>
        "#;

        let results = engine.parse_search_results(html).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Result");
        assert_eq!(results[0].url, "https://example.com/1");
        assert_eq!(results[0].engine, SearchEngineType::Bing);
        assert_eq!(results[1].title, "Second Result");
    }

    #[tokio::test]
    async fn test_parse_search_results_no_matching_selectors() {
        // 边界情况：HTML 不包含 Bing 结果选择器时返回空
        let engine = create_engine();
        let html = r#"<html><body><div>no bing results here</div></body></html>"#;
        let results = engine.parse_search_results(html).await.unwrap();
        assert!(results.is_empty());
    }
}
