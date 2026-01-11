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

/// Sogou Search Engine implementation
pub struct SogouSearchEngine {
    client: reqwest::Client,
}

impl Default for SogouSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SogouSearchEngine {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .build()
            .unwrap_or_default();

        Self { client }
    }

    fn parse_search_results(&self, html_content: &str) -> Result<Vec<ResponseItem>, SearchError> {
        let document = Html::parse_document(html_content);
        let result_selector = Selector::parse(".vrwrap, .rb").unwrap();
        let title_selector = Selector::parse("h3").unwrap();
        let link_selector = Selector::parse("h3 > a").unwrap();

        let mut results = Vec::new();

        for element in document.select(&result_selector) {
            let title = element
                .select(&title_selector)
                .next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default();

            let url = element
                .select(&link_selector)
                .next()
                .and_then(|e| e.value().attr("href"))
                .unwrap_or_default()
                .to_string();

            if !title.is_empty() && !url.is_empty() {
                results.push(ResponseItem {
                    title,
                    url,
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
    fn get_name(&self) -> &'static str {
        "Sogou"
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Sogou
    }

    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        let url = "https://www.sogou.com/web";
        let limit_str = request.limit.to_string();

        let query_params = vec![("query", request.query.clone()), ("num", limit_str)];

        let response = self
            .client
            .get(url)
            .query(&query_params)
            .send()
            .await
            .map_err(|e| SearchError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(SearchError::Engine(format!(
                "Sogou search error: {}",
                response.status()
            )));
        }

        let html_content = response
            .text()
            .await
            .map_err(|e| SearchError::Network(e.to_string()))?;

        let items = self.parse_search_results(&html_content)?;

        Ok(Response {
            items,
            total_results: Some(items.len() as u64),
            engine: SearchEngineType::Sogou,
        })
    }
}
