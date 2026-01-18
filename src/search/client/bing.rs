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
use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use std::collections::HashMap;
use url::Url;

/// Bing Search Engine implementation with connection pooling
pub struct BingSearchEngine {
    parser: HtmlParser,
}

impl Default for BingSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl BingSearchEngine {
    pub fn new() -> Self {
        Self {
            parser: HtmlParser::for_bing(),
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

        let response = SHARED_HTTP_CLIENT
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
            .map_err(SearchError::Network)?;

        if !response.status().is_success() {
            return Err(SearchError::Engine(format!(
                "HTTP error {}",
                response.status()
            )));
        }

        let html = response.text().await.map_err(SearchError::Network)?;
        let items = self.parse_search_results(&html).await?;
        let total_count = items.len() as u64;

        Ok(Response {
            items,
            total_results: Some(total_count),
            engine: SearchEngineType::Bing,
        })
    }
}
