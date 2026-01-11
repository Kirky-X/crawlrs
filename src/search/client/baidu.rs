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

/// Baidu Search Engine implementation
pub struct BaiduSearchEngine;

/// Shared HTTP client for connection pooling
static HTTP_CLIENT: once_cell::sync::Lazy<reqwest::Client> = once_cell::sync::Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

impl Default for BaiduSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl BaiduSearchEngine {
    pub fn new() -> Self {
        Self
    }

    async fn parse_search_results(
        &self,
        html: &str,
        _query: &str,
    ) -> Result<Vec<ResponseItem>, SearchError> {
        let document = Html::parse_document(html);
        let result_selector = Selector::parse("div.c-container")
            .unwrap_or_else(|_| Selector::parse("div.result").unwrap());
        let title_selector =
            Selector::parse("h3 a").unwrap_or_else(|_| Selector::parse("a").unwrap());
        let link_selector = Selector::parse("a").unwrap();
        let snippet_selector =
            Selector::parse("div.c-abstract").unwrap_or_else(|_| Selector::parse("div").unwrap());

        let mut results = Vec::new();

        for element in document.select(&result_selector) {
            let title = element
                .select(&title_selector)
                .next()
                .map(|el| el.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            let url = element
                .select(&link_selector)
                .next()
                .and_then(|el| el.value().attr("href"))
                .map(|href| href.to_string())
                .unwrap_or_default();

            let description = element
                .select(&snippet_selector)
                .next()
                .map(|el| el.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            if !title.is_empty() && !url.is_empty() {
                results.push(ResponseItem {
                    title,
                    url,
                    description,
                    engine: SearchEngineType::Baidu,
                });
            }
        }

        Ok(results)
    }
}

#[async_trait]
impl SearchEngine for BaiduSearchEngine {
    fn get_name(&self) -> &'static str {
        "Baidu"
    }
    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Baidu
    }
    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        let url = "https://www.baidu.com/s";

        let response = HTTP_CLIENT
            .get(url)
            .query(&[
                ("wd", request.query.clone()),
                ("rn", request.limit.to_string()),
                ("tn", "json"),
            ])
            .send()
            .await
            .map_err(|e| SearchError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(SearchError::Engine(format!(
                "Baidu search error: {}",
                response.status()
            )));
        }

        let html = response
            .text()
            .await
            .map_err(|e| SearchError::Network(e.to_string()))?;
        let items = self.parse_search_results(&html, &request.query).await?;

        Ok(Response {
            items,
            total_results: Some(items.len() as u64),
            engine: SearchEngineType::Baidu,
        })
    }
}
