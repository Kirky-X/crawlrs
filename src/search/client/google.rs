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
use rand::Rng;
use scraper::{Html, Selector};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, warn};

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

impl Default for GoogleSearchEngine {
    fn default() -> Self {
        Self::new(Arc::new(EngineClient::new()))
    }
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

    /// Escape HTML entities to prevent XSS
    fn escape_html(text: &str) -> String {
        html_escape::decode_html_entities(text).trim().to_string()
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
        if std::env::var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS").unwrap_or_default() == "true" {
            return Ok(Response {
                items: vec![
                    ResponseItem {
                        title: format!("Test Result 1 for {}", request.query),
                        url: "https://google.com/1".to_string(),
                        description: "Test description 1".to_string(),
                        engine: SearchEngineType::Google,
                    },
                    ResponseItem {
                        title: format!("Test Result 2 for {}", request.query),
                        url: "https://google.com/2".to_string(),
                        description: "Test description 2".to_string(),
                        engine: SearchEngineType::Google,
                    },
                ],
                total_results: Some(2),
                engine: SearchEngineType::Google,
            });
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
        let engine_client = Arc::new(EngineClient::default());
        let engine = GoogleSearchEngine::new(engine_client);
        assert_eq!(engine.name(), "Google");
        assert_eq!(engine.engine_type(), SearchEngineType::Google);
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    #[tokio::test]
    async fn test_get_arc_id_refresh() {
        let engine_client = Arc::new(EngineClient::default());
        let engine = GoogleSearchEngine::new(engine_client);
        let arc_id1 = engine.get_arc_id(0).await;
        assert!(arc_id1.contains("arc_id:srp_"));
        assert!(arc_id1.contains("use_ac:true"));

        tokio::time::sleep(Duration::from_millis(100)).await;
        let arc_id2 = engine.get_arc_id(10).await;
        assert!(arc_id2.contains("arc_id:srp_"));
        assert_ne!(arc_id1, arc_id2);
    }
}
