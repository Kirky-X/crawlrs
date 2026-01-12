// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(deprecated)]

use crawlrs::search::client::google::GoogleSearchEngine;

use crawlrs::engines::client::fire_cdp::FireEngineCdp;
use crawlrs::engines::client::fire_tls::FireEngineTls;
use crawlrs::engines::engine_client::EngineClient;
use std::sync::Arc;

#[allow(dead_code)]
pub fn create_google_engine() -> GoogleSearchEngine {
    let mut engines: Vec<Arc<dyn crawlrs::engines::traits::ScraperEngine>> = Vec::new();
    let fire_engine_cdp = Arc::new(FireEngineCdp::new());
    engines.push(fire_engine_cdp as Arc<dyn crawlrs::engines::traits::ScraperEngine>);
    let fire_engine_tls = Arc::new(FireEngineTls::new());
    engines.push(fire_engine_tls as Arc<dyn crawlrs::engines::traits::ScraperEngine>);
    let engine_client = Arc::new(EngineClient::with_engines(engines));
    GoogleSearchEngine::new(engine_client)
}

#[allow(dead_code)]
pub fn create_flaresolverr_google_engine(flaresolverr_url: &str) -> FlareSolverrGoogleEngine {
    FlareSolverrGoogleEngine::new(flaresolverr_url.to_string())
}

#[allow(dead_code)]
pub struct FlareSolverrGoogleEngine {
    flaresolverr_url: String,
    client: reqwest::Client,
}

#[allow(dead_code)]
impl FlareSolverrGoogleEngine {
    pub fn new(flaresolverr_url: String) -> Self {
        Self {
            flaresolverr_url,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap(),
        }
    }

    pub async fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<
        Vec<crawlrs::domain::models::search_result::SearchResult>,
        crawlrs::domain::search::engine::SearchError,
    > {
        let url = format!(
            "https://www.google.com/search?q={}&num={}",
            urlencoding::encode(query),
            max_results
        );

        let request = serde_json::json!({
            "cmd": "request.get",
            "url": url,
            "maxTimeout": 60000,
            "headers": {
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
            }
        });

        let response = self
            .client
            .post(format!("{}/v1", self.flaresolverr_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                crawlrs::domain::search::engine::SearchError::NetworkError(e.to_string())
            })?;

        let json_response: serde_json::Value = response.json().await.map_err(|e| {
            crawlrs::domain::search::engine::SearchError::NetworkError(e.to_string())
        })?;

        if json_response["status"] == "ok" {
            let html = json_response["solution"]["response"]
                .as_str()
                .ok_or_else(|| {
                    crawlrs::domain::search::engine::SearchError::NetworkError(
                        "No response content".to_string(),
                    )
                })?
                .to_string();

            self.parse_search_results(&html)
        } else {
            Err(crawlrs::domain::search::engine::SearchError::EngineError(
                json_response["message"]
                    .as_str()
                    .unwrap_or("Unknown error")
                    .to_string(),
            ))
        }
    }

    fn parse_search_results(
        &self,
        html: &str,
    ) -> Result<
        Vec<crawlrs::domain::models::search_result::SearchResult>,
        crawlrs::domain::search::engine::SearchError,
    > {
        use scraper::{Html, Selector};

        let document = Html::parse_document(html);
        let title_selector = Selector::parse("h3").unwrap();
        let _link_selector = Selector::parse("a").unwrap();
        let snippet_selector = Selector::parse(".VwiC3b").unwrap();

        let mut results = Vec::new();

        for (idx, element) in document.select(&title_selector).enumerate().take(10) {
            let title = element.text().collect::<String>();

            if let Some(link_elem) = element
                .ancestors()
                .find(|anc| anc.value().as_element().is_some_and(|e| e.name() == "a"))
            {
                let link = link_elem
                    .value()
                    .as_element()
                    .and_then(|e| e.attr("href"))
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                let description = document
                    .select(&snippet_selector)
                    .nth(idx)
                    .map(|e| e.text().collect::<String>())
                    .unwrap_or_default();

                if !title.is_empty() && link.starts_with("http") {
                    results.push(crawlrs::domain::models::search_result::SearchResult {
                        title,
                        url: link,
                        description: Some(description),
                        engine: "google".to_string(),
                        score: 0.0,
                        published_time: None,
                    });
                }
            }
        }

        Ok(results)
    }
}
