// Copyright (c) 2025 Kirky.X
//
// Licensed under MIT License
// See LICENSE file in the project root for full license information.

use crate::search::{
    engine_trait::SearchEngine,
    error::SearchError,
    response::{Response, ResponseItem},
    types::{EngineHealth, SearchEngineType},
    SearchRequest,
};
use async_trait::async_trait;
use scraper::{Html, Selector};
use serde_json;
use std::collections::HashMap;
use std::time::Duration;

/// Baidu Search Categories
#[derive(Debug, Clone, Copy)]
pub enum BaiduSearchCategory {
    General,
    Images,
    News,
}

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

    pub fn build_baidu_url(
        &self,
        query: &str,
        page: u32,
        category: BaiduSearchCategory,
    ) -> (String, HashMap<String, String>) {
        let mut params = HashMap::new();
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
        let json: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
            SearchError::ContentParsing(format!("Failed to parse Baidu JSON: {}", e))
        })?;

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
                        title: title.replace("&lt;", "<").replace("&gt;", ">"),
                        url,
                        description: description.replace("&lt;", "<").replace("&gt;", ">"),
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
        let url = "https://www.baidu.com/s";

        let response = HTTP_CLIENT
            .get(url)
            .query(&[
                ("wd", request.query.clone()),
                ("rn", request.limit.to_string()),
                ("tn", "json".to_string()),
            ])
            .send()
            .await
            .map_err(|e| SearchError::Network(e))?;

        if !response.status().is_success() {
            return Err(SearchError::Engine(format!(
                "Baidu search error: {}",
                response.status()
            )));
        }

        let content = response.text().await.map_err(|e| SearchError::Network(e))?;

        // Since we requested json with tn=json, use parse_baidu_response
        let items = self.parse_baidu_response(&content)?;
        let total_count = items.len() as u64;

        Ok(Response {
            items,
            total_results: Some(total_count),
            engine: SearchEngineType::Baidu,
        })
    }
}
