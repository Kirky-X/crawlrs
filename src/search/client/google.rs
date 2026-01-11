// Copyright (c) 2025 Kirky.X
//
// Licensed under MIT License
// See LICENSE file in the project root for full license information.

use super::{
    error::SearchError,
    response::{Response, ResponseItem},
    types::{EngineHealth, SearchEngineType},
    SearchRequest,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rand::Rng;
use reqwest::Client;
use scraper::{ElementRef, Html, Selector};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Google HTTP client - reused across requests for connection pooling
static HTTP_CLIENT: once_cell::sync::Lazy<Client> = once_cell::sync::Lazy::new(|| {
    Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .unwrap_or_else(|_| Client::new())
});

/// Google ARC_ID cache structure
struct ArcIdCache {
    arc_id: String,
    generated_at: i64,
}

/// Google Search Engine implementation
pub struct GoogleSearchEngine {
    arc_id_cache: Arc<RwLock<ArcIdCache>>,
}

impl Default for GoogleSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleSearchEngine {
    pub fn new() -> Self {
        Self {
            arc_id_cache: Arc::new(RwLock::new(ArcIdCache {
                arc_id: Self::generate_random_id(),
                generated_at: Utc::now().timestamp(),
            })),
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

    /// Parse Google HTML results with XSS protection
    fn parse_results(&self, html: &str) -> Result<Vec<ResponseItem>, SearchError> {
        let document = Html::parse_document(html);
        let mut results = Vec::new();

        info!("Parsing Google search results...");

        // Multiple selector strategies for robustness
        let selectors = [
            Selector::parse("div[jscontroller*='SC7lYd']").unwrap(),
            Selector::parse("div.g").unwrap(),
            Selector::parse("div[data-hveid]").unwrap(),
            Selector::parse("div:has(> a > h3)").unwrap(),
        ];

        let result_elements = selectors
            .iter()
            .find_map(|s| {
                let elements: Vec<_> = document.select(s).collect();
                if !elements.is_empty() {
                    Some(elements)
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let title_selector = Selector::parse("h3").unwrap();
        let link_selector = Selector::parse("a[href]").unwrap();
        let snippet_selectors = [
            Selector::parse("[data-sncf], div[data-snc]").unwrap(),
            Selector::parse("span.st, div.st, p.st").unwrap(),
            Selector::parse("div[class*='snippet'], div[class*='desc']").unwrap(),
        ];

        for element in result_elements {
            // Extract title
            let title = element
                .select(&title_selector)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            if title.is_empty() {
                continue;
            }

            // Extract URL
            let mut found_url = String::new();
            if let Some(a) = element.select(&link_selector).next() {
                if a.select(&title_selector).next().is_some() {
                    if let Some(href) = a.value().attr("href") {
                        if !href.is_empty() {
                            found_url = href.to_string();
                        }
                    }
                }
            }
            if found_url.is_empty() {
                for a in element.select(&link_selector) {
                    if let Some(href) = a.value().attr("href") {
                        if !href.is_empty() && href.starts_with("http") {
                            found_url = href.to_string();
                            break;
                        }
                    }
                }
            }

            // Clean and validate URL
            let clean_url = if found_url.starts_with("/url?q=") {
                found_url
                    .trim_start_matches("/url?q=")
                    .split('&')
                    .next()
                    .unwrap_or(&found_url)
                    .to_string()
            } else if found_url.starts_with("/") && !found_url.starts_with("//") {
                format!("https://www.google.com{}", found_url)
            } else {
                found_url
            };

            if clean_url.is_empty() || !clean_url.starts_with("http") {
                continue;
            }

            // Extract snippet
            let description = snippet_selectors
                .iter()
                .find_map(|s| {
                    element
                        .select(s)
                        .next()
                        .map(|e| e.text().collect::<String>().trim().to_string())
                        .filter(|t| !t.is_empty())
                })
                .unwrap_or_default();

            // Deduplicate by URL
            if results.iter().any(|r: &ResponseItem| r.url == clean_url) {
                continue;
            }

            results.push(ResponseItem {
                title: Self::escape_html(&title),
                url: clean_url,
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
    fn get_name(&self) -> &'static str {
        "Google"
    }
    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Google
    }
    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        let page = 1;
        let start = (page - 1) * request.limit;

        // Build query parameters
        let mut query_params: Vec<(&str, String)> = vec![
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

        let response = HTTP_CLIENT
            .get(&google_url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
            )
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("DNT", "1")
            .header("Connection", "keep-alive")
            .header("Upgrade-Insecure-Requests", "1")
            .send()
            .await
            .map_err(|e| SearchError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(SearchError::Engine(format!(
                "Google search returned status: {}",
                response.status()
            )));
        }

        let html = response
            .text()
            .await
            .map_err(|e| SearchError::Network(e.to_string()))?;
        info!("Google search returned HTML length: {} bytes", html.len());

        if html.len() < 1000 {
            warn!("Google search returned insufficient content (likely blocked)");
            return Err(SearchError::Engine(
                "Google search returned insufficient content (likely blocked)".to_string(),
            ));
        }

        let items = self.parse_results(&html)?;

        Ok(Response {
            items,
            total_results: Some(items.len() as u64),
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
        let engine = GoogleSearchEngine::new();
        assert_eq!(engine.get_name(), "Google");
        assert_eq!(engine.engine_type(), SearchEngineType::Google);
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    #[tokio::test]
    async fn test_get_arc_id_refresh() {
        let engine = GoogleSearchEngine::new();
        let arc_id1 = engine.get_arc_id(0).await;
        assert!(arc_id1.contains("arc_id:srp_"));
        assert!(arc_id1.contains("use_ac:true"));

        tokio::time::sleep(Duration::from_millis(100)).await;
        let arc_id2 = engine.get_arc_id(10).await;
        assert!(arc_id2.contains("arc_id:srp_"));
        assert_ne!(arc_id1, arc_id2);
    }
}
