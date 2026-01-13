// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::search::{
    client::html_parser::HtmlParser,
    client::http_client::SHARED_HTTP_CLIENT,
    engine_trait::SearchEngine,
    error::SearchError,
    response::{Response, ResponseItem},
    types::{EngineHealth, SearchEngineType},
    SearchRequest,
};
use async_trait::async_trait;
use serde_json;
use std::collections::HashMap;

/// Baidu Search Categories
#[derive(Debug, Clone, Copy)]
pub enum BaiduSearchCategory {
    General,
    Images,
    News,
}

/// Baidu Search Engine implementation
pub struct BaiduSearchEngine {
    parser: HtmlParser,
}

impl Default for BaiduSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl BaiduSearchEngine {
    pub fn new() -> Self {
        Self {
            parser: HtmlParser::for_baidu(),
        }
    }

    pub fn build_baidu_url(
        &self,
        query: &str,
        page: u32,
        category: BaiduSearchCategory,
    ) -> (String, HashMap<String, String>) {
        let mut params = HashMap::with_capacity(8);
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
        let json: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| SearchError::Parse(format!("Failed to parse Baidu JSON: {}", e)))?;

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
        Ok(self.parser.parse(html, SearchEngineType::Baidu))
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
        if std::env::var("BAIDU_TEST_RESULTS").unwrap_or_default() == "true" {
            return Ok(Response {
                items: vec![
                    ResponseItem {
                        title: format!("Baidu Test Result 1 for {}", request.query),
                        url: "https://baidu.com/1".to_string(),
                        description: "Test description 1".to_string(),
                        engine: SearchEngineType::Baidu,
                    },
                    ResponseItem {
                        title: format!("Baidu Test Result 2 for {}", request.query),
                        url: "https://baidu.com/2".to_string(),
                        description: "Test description 2".to_string(),
                        engine: SearchEngineType::Baidu,
                    },
                ],
                total_results: Some(2),
                engine: SearchEngineType::Baidu,
            });
        }

        let (url, params) = self.build_baidu_url(&request.query, 1, BaiduSearchCategory::General);

        let response = SHARED_HTTP_CLIENT
            .get(url)
            .query(&params)
            .send()
            .await
            .map_err(SearchError::Network)?;

        if !response.status().is_success() {
            return Err(SearchError::Engine(format!(
                "Baidu search error: {}",
                response.status()
            )));
        }

        let content = response.text().await.map_err(SearchError::Network)?;

        // Try parsing as HTML (Baidu returns HTML by default now)
        let items = self.parse_search_results(&content, &request.query).await?;

        // If items are empty, try JSON as fallback (though unlikely with current URL)
        let items = if items.is_empty() {
            if let Ok(json_items) = self.parse_baidu_response(&content) {
                if !json_items.is_empty() {
                    json_items
                } else {
                    items
                }
            } else {
                items
            }
        } else {
            items
        };

        let total_count = items.len() as u64;

        Ok(Response {
            items,
            total_results: Some(total_count),
            engine: SearchEngineType::Baidu,
        })
    }
}
