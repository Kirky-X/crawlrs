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
use scraper::{Html, Selector};
use std::time::Duration;

/// Bing Search Engine implementation with connection pooling
pub struct BingSearchEngine {
    result_regex: regex::Regex,
    title_regex: regex::Regex,
    link_regex: regex::Regex,
    snippet_regex: regex::Regex,
}

/// Shared HTTP client for connection pooling
static HTTP_CLIENT: once_cell::sync::Lazy<reqwest::Client> = once_cell::sync::Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

impl Default for BingSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl BingSearchEngine {
    pub fn new() -> Self {
        let result_regex = regex::Regex::new(r#"(?s)<li class="b_algo"[^>]*>(.*?)</li>"#)
            .expect("Failed to compile result regex");
        let title_regex =
            regex::Regex::new(r"(?s)<h2[^>]*>(.*?)</h2>").expect("Failed to compile title regex");
        let link_regex = regex::Regex::new(r#"<a[^>]*href="([^"]*)"[^>]*>"#)
            .expect("Failed to compile link regex");
        let snippet_regex =
            regex::Regex::new(r"(?s)<p[^>]*>(.*?)</p>").expect("Failed to compile snippet regex");

        Self {
            result_regex,
            title_regex,
            link_regex,
            snippet_regex,
        }
    }

    fn build_bing_url(&self, query: &str, page: u32) -> String {
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

    async fn parse_search_results(&self, html: &str) -> Result<Vec<ResponseItem>, SearchError> {
        if html.is_empty() {
            return Err(SearchError::Parse(
                "Empty HTML response received".to_string(),
            ));
        }

        let mut results = Vec::new();
        let result_matches: Vec<_> = self.result_regex.find_iter(html).collect();
        results.reserve(result_matches.len().min(50));

        for result_match in result_matches {
            let result_html = result_match.as_str();

            let title_html = self
                .title_regex
                .captures(result_html)
                .and_then(|cap| cap.get(1))
                .map(|m| m.as_str())
                .unwrap_or_default();

            let url = self
                .link_regex
                .captures(title_html)
                .and_then(|cap| cap.get(1))
                .map(|m| m.as_str())
                .unwrap_or_default();

            let title = title_html.replace(r"<[^>]+>", "");

            let snippet = self
                .snippet_regex
                .captures(result_html)
                .and_then(|cap| cap.get(1))
                .map(|m| m.as_str().replace(r"<[^>]+>", ""));

            if title.is_empty() || url.is_empty() || !url.starts_with("http") {
                continue;
            }

            results.push(ResponseItem {
                title,
                url,
                description: snippet.unwrap_or_default(),
                engine: SearchEngineType::Bing,
            });
        }

        Ok(results)
    }
}

#[async_trait]
impl SearchEngine for BingSearchEngine {
    fn get_name(&self) -> &'static str {
        "Bing"
    }
    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Bing
    }
    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        if request.query.trim().is_empty() {
            return Err(SearchError::Parse(
                "Search query cannot be empty".to_string(),
            ));
        }

        let url = self.build_bing_url(&request.query, 1);

        let response = HTTP_CLIENT
            .get(&url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
            )
            .header("Accept-Language", "en-US,en;q=0.5")
            .header("DNT", "1")
            .header("Connection", "keep-alive")
            .header("Upgrade-Insecure-Requests", "1")
            .send()
            .await
            .map_err(|e| SearchError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(SearchError::Engine(format!(
                "HTTP error {}",
                response.status()
            )));
        }

        let html = response
            .text()
            .await
            .map_err(|e| SearchError::Network(e.to_string()))?;
        let items = self.parse_search_results(&html).await?;

        Ok(Response {
            items,
            total_results: Some(items.len() as u64),
            engine: SearchEngineType::Bing,
        })
    }
}
