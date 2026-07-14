// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::engine_client::{EngineClient, ScrapeRequest as EngineScrapeRequest};
use crate::search::{
    engine_trait::SearchEngine,
    error::SearchError,
    response::{Response, ResponseItem},
    types::{EngineHealth, SearchEngineType},
    SearchRequest,
};
use async_trait::async_trait;
use chrono::Utc;
use log::{info, warn};
use rand::Rng;
use scraper::{Html, Selector};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// 安全解析CSS选择器，如果解析失败则返回None
fn safe_parse_selector(selector_str: &str) -> Option<Selector> {
    Selector::parse(selector_str).ok()
}

/// Google Search Engine implementation
struct ArcIdCache {
    arc_id: String,
    generated_at: i64,
}

/// Google Search Engine implementation
pub struct GoogleSearchEngine {
    arc_id_cache: Arc<RwLock<ArcIdCache>>,
    engine_client: Arc<EngineClient>,
}

impl GoogleSearchEngine {
    pub fn new(engine_client: Arc<EngineClient>) -> Self {
        Self {
            arc_id_cache: Arc::new(RwLock::new(ArcIdCache {
                arc_id: Self::generate_random_id(),
                generated_at: Utc::now().timestamp(),
            })),
            engine_client,
        }
    }

    /// Generate 23-character random ARC_ID
    fn generate_random_id() -> String {
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
        let mut rng = rand::rng();
        (0..23)
            .map(|_| {
                let idx = rng.random_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }

    /// Get ARC_ID (auto-refreshes every hour)
    pub async fn get_arc_id(&self, start_offset: usize) -> String {
        let mut cache = self.arc_id_cache.write().await;
        let now = Utc::now().timestamp();

        if now - cache.generated_at > 3600 {
            cache.arc_id = Self::generate_random_id();
            cache.generated_at = now;
            info!("Google ARC_ID refreshed: {}", cache.arc_id);
        }

        format!(
            "arc_id:srp_{}_1{:02},use_ac:true,_fmt:prog",
            cache.arc_id, start_offset
        )
    }

    pub async fn force_refresh_arc_id(&self) {
        let mut cache = self.arc_id_cache.write().await;
        cache.arc_id = Self::generate_random_id();
        cache.generated_at = Utc::now().timestamp();
        info!("Google ARC_ID forcefully refreshed: {}", cache.arc_id);
    }

    /// Parse Google HTML results with XSS protection
    pub fn parse_results(&self, html: &str) -> Result<Vec<ResponseItem>, SearchError> {
        let document = Html::parse_document(html);
        let mut results = Vec::new();

        info!("Parsing Google search results...");

        // 根据 temp/search.md 中的逆向工程结果
        // Google 结果包裹在 div[jscontroller*="SC7lYd"] 中
        let result_selector = safe_parse_selector("div[jscontroller*='SC7lYd']")
            .expect("Failed to parse Google selector: div[jscontroller*='SC7lYd']");

        // 标题在 a > h3 中
        let title_selector =
            safe_parse_selector("a h3, h3").expect("Failed to parse Google title selector");

        // URL 从 a 的 href 属性提取
        let link_selector =
            safe_parse_selector("a[href]").expect("Failed to parse Google link selector");

        // 摘要从 div[data-sncf="1"] 中提取
        let snippet_selector = safe_parse_selector("div[data-sncf='1'], div[data-snc]")
            .expect("Failed to parse Google snippet selector");

        // 提取搜索结果块
        for result_element in document.select(&result_selector) {
            // 提取标题
            let title_node = result_element.select(&title_selector).next();
            if title_node.is_none() {
                continue;
            }
            let title = title_node
                .expect("title_node should not be None after is_none() check")
                .text()
                .collect::<String>()
                .trim()
                .to_string();

            if title.is_empty() {
                continue;
            }

            // 提取链接
            let url_node = result_element.select(&link_selector).next();
            if url_node.is_none() {
                continue;
            }
            let mut url = url_node
                .expect("url_node should not be None after is_none() check")
                .value()
                .attr("href")
                .unwrap_or("")
                .to_string();

            if url.is_empty() {
                continue;
            }

            // 清理 URL - 处理 /url?q= 格式
            if url.starts_with("/url?q=") {
                url = url
                    .trim_start_matches("/url?q=")
                    .split('&')
                    .next()
                    .unwrap_or(&url)
                    .to_string();
            } else if url.starts_with("/") && !url.starts_with("//") {
                url = format!("https://www.google.com{}", url);
            }

            if !url.starts_with("http") {
                continue;
            }

            // 提取摘要 - data-sncf="1" 通常包含摘要文本
            let content_nodes = result_element.select(&snippet_selector).next();
            let description = content_nodes
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            // 去重
            if results.iter().any(|r: &ResponseItem| r.url == url) {
                continue;
            }

            results.push(ResponseItem {
                title: Self::escape_html(&title),
                url,
                description: Self::escape_html(&description),
                engine: SearchEngineType::Google,
            });

            if results.len() >= 20 {
                break;
            }
        }

        info!(
            "Successfully parsed {} Google search results",
            results.len()
        );
        Ok(results)
    }

    /// Escape HTML entities to prevent XSS attacks
    /// Uses encode_text to convert special characters to safe entities
    fn escape_html(text: &str) -> String {
        html_escape::encode_text(text).trim().to_string()
    }
}

#[async_trait]
impl SearchEngine for GoogleSearchEngine {
    fn name(&self) -> &'static str {
        "Google"
    }
    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Google
    }
    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        // Test fallback mode - only enabled in development/test environments
        if std::env::var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS").unwrap_or_default() == "true" {
            // 使用配置服务获取环境，如果不可用则回退到环境变量
            let env = std::env::var("CRAWLRS_ENV")
                .or_else(|_| std::env::var("APP_ENVIRONMENT"))
                .unwrap_or_else(|_| "development".to_string());
            let is_dev_env = matches!(
                env.as_str(),
                "development" | "dev" | "test" | "testing" | ""
            );

            if !is_dev_env {
                log::warn!(
                    "GOOGLE_HTTP_FALLBACK_TEST_RESULTS ignored in production (CRAWLRS_ENV={})",
                    env
                );
            } else {
                log::info!("Using test fallback results for Google search");
                let escaped_query = html_escape::encode_text(&request.query);
                return Ok(Response {
                    items: vec![
                        ResponseItem {
                            title: format!("Test Result 1 for {}", escaped_query),
                            url: "https://google.com/1".to_string(),
                            description: "Test description 1".to_string(),
                            engine: SearchEngineType::Google,
                        },
                        ResponseItem {
                            title: format!("Test Result 2 for {}", escaped_query),
                            url: "https://google.com/2".to_string(),
                            description: "Test description 2".to_string(),
                            engine: SearchEngineType::Google,
                        },
                    ],
                    total_results: Some(2),
                    engine: SearchEngineType::Google,
                });
            }
        }

        let page = 1;
        let start = (page - 1) * request.limit;

        // Build query parameters
        let query_params: Vec<(&str, String)> = vec![
            ("q", request.query.clone()),
            ("ie", "utf8".to_string()),
            ("oe", "utf8".to_string()),
            ("start", start.to_string()),
            ("num", request.limit.to_string()),
            ("asearch", "arc".to_string()),
            ("async", self.get_arc_id(start as usize).await),
        ];

        info!(
            "Google search request: query={}, limit={}",
            request.query, request.limit
        );

        // Build Google search URL
        let google_url = format!(
            "https://www.google.com/search?{}",
            query_params
                .iter()
                .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
                .collect::<Vec<_>>()
                .join("&")
        );
        info!(
            "Constructed Google Search URL (length: {})",
            google_url.len()
        );

        // Use EngineClient to scrape the search result page
        // Google requires JavaScript rendering, so we set needs_js=true
        // The EngineClient's smart routing will automatically select the optimal engine
        // based on support_score (Playwright/Playwright will get 100, Reqwest will get 10)
        let engine_request = EngineScrapeRequest::new(&google_url)
            .with_options(
                crate::engines::engine_client::ScrapeOptions::builder()
                    .needs_js(true)  // Google requires JS rendering
                    .timeout(Duration::from_secs(60))
                    .headers(
                        vec![
                            (
                                "Accept".to_string(),
                                "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8"
                                    .to_string(),
                            ),
                            ("Accept-Language".to_string(), "en-US,en;q=0.9".to_string()),
                        ]
                        .into_iter()
                        .collect(),
                    )
                    .build(),
            );

        let scrape_response = self
            .engine_client
            .scrape(&engine_request)
            .await
            .map_err(|e| SearchError::Engine(e.to_string()))?;

        // Handle non-200 status codes
        if !scrape_response.is_success() {
            if scrape_response.status_code == 429 {
                warn!("Google rate limit exceeded (429)");
                return Err(SearchError::Engine(
                    "Google rate limit exceeded".to_string(),
                ));
            }
            warn!(
                "Google returned status code: {}",
                scrape_response.status_code
            );
            // We might still try to parse if content is present, but usually error page
        }

        let results = self.parse_results(&scrape_response.content)?;

        // If no results and status was OK, it might be a different layout or captcha
        if results.is_empty() {
            warn!(
                "No results found on Google page. Content length: {}",
                scrape_response.content.len()
            );
        }

        Ok(Response {
            items: results,
            total_results: None,
            engine: SearchEngineType::Google,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_random_id() {
        let id1 = GoogleSearchEngine::generate_random_id();
        let id2 = GoogleSearchEngine::generate_random_id();
        assert_eq!(id1.len(), 23);
        assert_eq!(id2.len(), 23);
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn test_google_search_engine_creation() {
        use crate::engines::engine_client::EngineClient;
        let engine_client = Arc::new(EngineClient::new());
        let engine = GoogleSearchEngine::new(engine_client);
        assert_eq!(engine.name(), "Google");
        assert_eq!(engine.engine_type(), SearchEngineType::Google);
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    #[tokio::test]
    async fn test_get_arc_id_refresh() {
        use crate::engines::engine_client::EngineClient;
        let engine_client = Arc::new(EngineClient::new());
        let engine = GoogleSearchEngine::new(engine_client);
        let arc_id1 = engine.get_arc_id(0).await;
        assert!(arc_id1.contains("arc_id:srp_"));
        assert!(arc_id1.contains("use_ac:true"));

        tokio::time::sleep(Duration::from_millis(100)).await;
        let arc_id2 = engine.get_arc_id(10).await;
        assert!(arc_id2.contains("arc_id:srp_"));
        assert_ne!(arc_id1, arc_id2);
    }

    // ========== generate_random_id 补充测试 ==========

    #[test]
    fn test_generate_random_id_charset() {
        // 测试生成的 ID 只包含合法字符（A-Z a-z 0-9 _ -）
        let id = GoogleSearchEngine::generate_random_id();
        let valid_chars: std::collections::HashSet<char> =
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-"
                .chars()
                .collect();
        for c in id.chars() {
            assert!(
                valid_chars.contains(&c),
                "invalid character '{}' in ID: {}",
                c,
                id
            );
        }
    }

    #[test]
    fn test_generate_random_id_multiple_calls_all_correct_length() {
        // 测试多次调用都返回 23 字符长度的 ID
        for _ in 0..10 {
            let id = GoogleSearchEngine::generate_random_id();
            assert_eq!(id.len(), 23, "every ID should be 23 characters");
        }
    }

    // ========== escape_html 测试 ==========

    #[test]
    fn test_escape_html_plain_text_unchanged() {
        // 测试普通文本不被修改
        let text = "Rust Programming Language";
        assert_eq!(
            GoogleSearchEngine::escape_html(text),
            "Rust Programming Language"
        );
    }

    #[test]
    fn test_escape_html_special_chars_encoded() {
        // 测试 HTML 特殊字符 & < > 被编码
        let text = "<script>alert('xss')</script> & more";
        let escaped = GoogleSearchEngine::escape_html(text);
        assert!(!escaped.contains('<'), "should not contain raw <");
        assert!(!escaped.contains('>'), "should not contain raw >");
        assert!(escaped.contains("&lt;"), "should contain &lt;");
        assert!(escaped.contains("&gt;"), "should contain &gt;");
        assert!(escaped.contains("&amp;"), "should contain &amp;");
    }

    #[test]
    fn test_escape_html_empty_string() {
        // 边界情况：空字符串返回空字符串
        assert_eq!(GoogleSearchEngine::escape_html(""), "");
    }

    #[test]
    fn test_escape_html_trims_whitespace() {
        // 测试首尾空白被 trim
        let text = "  trimmed content  ";
        assert_eq!(GoogleSearchEngine::escape_html(text), "trimmed content");
    }

    // ========== parse_results 测试 ==========

    fn create_engine() -> GoogleSearchEngine {
        use crate::engines::engine_client::EngineClient;
        let engine_client = Arc::new(EngineClient::new());
        GoogleSearchEngine::new(engine_client)
    }

    #[test]
    fn test_parse_results_empty_html() {
        // 边界情况：空 HTML 返回 Ok 但结果为空
        let engine = create_engine();
        let results = engine.parse_results("").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_valid_html() {
        // 测试从有效 Google HTML 解析结果
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/1"><h3>First Result</h3></a>
                <div data-sncf="1">First snippet</div>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/2"><h3>Second Result</h3></a>
                <div data-sncf="1">Second snippet</div>
            </div>
        </body></html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Result");
        assert_eq!(results[0].url, "https://example.com/1");
        assert_eq!(results[0].description, "First snippet");
        assert_eq!(results[0].engine, SearchEngineType::Google);
        assert_eq!(results[1].title, "Second Result");
    }

    #[test]
    fn test_parse_results_no_matching_selectors() {
        // 边界情况：HTML 不包含 Google 结果选择器时返回空
        let engine = create_engine();
        let html = r#"<html><body><div>no results here</div></body></html>"#;
        let results = engine.parse_results(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_url_q_format_cleaned() {
        // 测试 /url?q= 格式的链接被正确清理
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="/url?q=https://example.com/real&sa=t"><h3>Cleaned URL</h3></a>
            </div>
        </body></html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/real");
    }

    #[test]
    fn test_parse_results_relative_url_prefixed() {
        // 测试相对 URL 被添加 google.com 前缀
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="/relative/path"><h3>Relative URL</h3></a>
            </div>
        </body></html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://www.google.com/relative/path");
    }

    #[test]
    fn test_parse_results_skips_non_http_urls() {
        // 边界情况：非 http 开头的 URL 被跳过
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="javascript:void(0)"><h3>JS Link</h3></a>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/valid"><h3>Valid</h3></a>
            </div>
        </body></html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/valid");
    }

    #[test]
    fn test_parse_results_deduplicates_by_url() {
        // 测试相同 URL 的结果被去重
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/dup"><h3>First Title</h3></a>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/dup"><h3>Duplicate Title</h3></a>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/unique"><h3>Unique Title</h3></a>
            </div>
        </body></html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Title");
        assert_eq!(results[1].title, "Unique Title");
    }

    #[test]
    fn test_parse_results_max_20_limit() {
        // 测试结果数量上限为 20
        let engine = create_engine();
        let mut html = String::from("<html><body>");
        for i in 0..25 {
            html.push_str(&format!(
                r#"<div jscontroller="SC7lYd"><a href="https://example.com/{}"><h3>Result {}</h3></a></div>"#,
                i, i
            ));
        }
        html.push_str("</body></html>");

        let results = engine.parse_results(&html).unwrap();
        assert_eq!(results.len(), 20, "should cap at 20 results");
    }

    #[test]
    fn test_parse_results_skips_missing_title() {
        // 边界情况：缺少 h3 标题的块被跳过
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/1"></a>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/2"><h3>Valid Result</h3></a>
            </div>
        </body></html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Valid Result");
    }
}
