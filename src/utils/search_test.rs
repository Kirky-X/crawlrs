// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use anyhow::Result;
use crawlrs::infrastructure::search::SearchEngine;

pub struct TestResult {
    pub total: usize,
    pub accessible: usize,
    pub inaccessible: usize,
}

pub async fn run_engine_test_with_output<E: SearchEngine>(
    name: &str,
    mut engine: E,
    query: Option<&str>,
    timeout_secs: u64,
    limit: Option<u32>,
) -> Result<TestResult> {
    use crawlrs::infrastructure::search::SearchResult;
    use tokio::time::timeout;

    let start_time = std::time::Instant::now();
    let query = query.unwrap_or("test query");

    let result = timeout(
        std::time::Duration::from_secs(timeout_secs),
        engine.search(query, limit.unwrap_or(10)),
    )
    .await;

    let elapsed = start_time.elapsed();

    match result {
        Ok(Ok(entries)) => {
            let total = entries.len();
            let mut accessible = 0;
            let mut inaccessible = 0;

            for entry in entries {
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
            Err(e)
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

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;

    let response = client.head(url).send().await;
    response.map(|r| r.status().is_success()).unwrap_or(false)
}
