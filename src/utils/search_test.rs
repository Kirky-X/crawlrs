// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::search::engine_trait::{SearchEngine, SearchRequest};
use anyhow::Result;

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

    match result {
        Ok(Ok(response)) => {
            let total = response.items.len();
            let mut accessible = 0;
            let mut inaccessible = 0;

            for entry in response.items {
                let url = &entry.url;
                let is_accessible = check_url_accessible(url).await;
                if is_accessible {
                    accessible += 1;
                } else {
                    inaccessible += 1;
                }
            }

            tracing::info!(
                "[{}] Search completed in {:.2}s",
                name,
                elapsed.as_secs_f64()
            );
            tracing::info!("[{}] Total results: {}", name, total);

            Ok(TestResult {
                total,
                accessible,
                inaccessible,
            })
        }
        Ok(Err(e)) => {
            tracing::error!("[{}] Search failed: {:?}", name, e);
            Err(e.into())
        }
        Err(_) => {
            tracing::error!("[{}] Search timed out after {}s", name, timeout_secs);
            Err(anyhow::anyhow!("Search timed out"))
        }
    }
}

async fn check_url_accessible(url: &str) -> bool {
    use reqwest::Client;
    use std::time::Duration;

    let client = match Client::builder().timeout(Duration::from_secs(5)).build() {
        Ok(c) => c,
        Err(_) => return false,
    };

    let response = client.head(url).send().await;
    response.map(|r| r.status().is_success()).unwrap_or(false)
}
