// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::search::{
    client::http_client::SHARED_HTTP_CLIENT,
    engine_trait::SearchEngine,
    error::SearchError,
    response::{Response, ResponseItem},
    types::{EngineHealth, SearchEngineType},
    SearchRequest,
};
use async_trait::async_trait;
use scraper::{Html, Selector};
use std::time::Duration;

/// Sogou Search Engine implementation
pub struct SogouSearchEngine;

impl Default for SogouSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SogouSearchEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn parse_search_results(
        &self,
        html_content: &str,
    ) -> Result<Vec<ResponseItem>, SearchError> {
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
            return Ok(Response {
                items: vec![
                    ResponseItem {
                        title: format!("Test Result 1 for {}", request.query),
                        url: "https://sogou.com/1".to_string(),
                        description: "Test description 1".to_string(),
                        engine: SearchEngineType::Sogou,
                    },
                    ResponseItem {
                        title: format!("Test Result 2 for {}", request.query),
                        url: "https://sogou.com/2".to_string(),
                        description: "Test description 2".to_string(),
                        engine: SearchEngineType::Sogou,
                    },
                ],
                total_results: Some(2),
                engine: SearchEngineType::Sogou,
            });
        }

        let url = "https://www.sogou.com/web";

        let response = SHARED_HTTP_CLIENT
            .get(url)
            .query(&[
                ("query", request.query.clone()),
                ("num", request.limit.to_string()),
            ])
            .send()
            .await
            .map_err(SearchError::Network)?;

        if !response.status().is_success() {
            return Err(SearchError::Engine(format!(
                "Sogou search error: {}",
                response.status()
            )));
        }

        let html_content = response.text().await.map_err(SearchError::Network)?;
        let items = self.parse_search_results(&html_content)?;
        let total_count = items.len() as u64;

        Ok(Response {
            items,
            total_results: Some(total_count),
            engine: SearchEngineType::Sogou,
        })
    }
}
