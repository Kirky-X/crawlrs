// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::{EngineClient, ScrapeOptions, ScrapeRequest};
use crate::engines::router::{EngineRouter, EngineRouterTrait};
use crate::search::engine_trait::{SearchEngine, SearchRequest};
use anyhow::Result;
use std::sync::Arc;

#[derive(Debug, Default, Clone)]
pub struct TestResult {
    pub total: usize,
    pub accessible: usize,
    pub inaccessible: usize,
}

pub async fn run_engine_test_with_output<E: SearchEngine>(
    name: &str,
    engine: E,
    query: Option<&str>,
    timeout_secs: u64,
    limit: Option<u32>,
) -> Result<TestResult> {
    use tokio::time::timeout;

    let start_time = std::time::Instant::now();
    let query_str = query.unwrap_or("test query");

    let request = SearchRequest::new(query_str).with_limit(limit.unwrap_or(10));

    let result = timeout(
        std::time::Duration::from_secs(timeout_secs),
        engine.search(&request),
    )
    .await;

    let elapsed = start_time.elapsed();

    let engine_client = build_test_engine_client();

    match result {
        Ok(Ok(response)) => {
            let total = response.items.len();
            let mut accessible = 0;
            let mut inaccessible = 0;

            for entry in response.items {
                let url = &entry.url;
                let is_accessible = match engine_client.as_ref() {
                    Some(client) => check_url_accessible(client, url).await,
                    None => false,
                };
                if is_accessible {
                    accessible += 1;
                } else {
                    inaccessible += 1;
                }
            }

            log::info!(
                "[{}] Search completed in {:.2}s",
                name,
                elapsed.as_secs_f64()
            );
            log::info!("[{}] Total results: {}", name, total);

            Ok(TestResult {
                total,
                accessible,
                inaccessible,
            })
        }
        Ok(Err(e)) => {
            log::error!("[{}] Search failed: {:?}", name, e);
            Err(e.into())
        }
        Err(_) => {
            log::error!("[{}] Search timed out after {}s", name, timeout_secs);
            Err(anyhow::anyhow!("Search timed out"))
        }
    }
}

fn build_test_engine_client() -> Option<Arc<EngineClient>> {
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    let reqwest_engine = ReqwestEngine::new(Arc::new(http_client));
    let router: Arc<dyn EngineRouterTrait> =
        Arc::new(EngineRouter::new(vec![Arc::new(reqwest_engine)]));
    Some(Arc::new(EngineClient::with_router(router)))
}

async fn check_url_accessible(engine_client: &EngineClient, url: &str) -> bool {
    let options = ScrapeOptions::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let request = ScrapeRequest::new(url).with_options(options);
    engine_client
        .scrape(&request)
        .await
        .map(|response| response.is_success())
        .unwrap_or(false)
}
