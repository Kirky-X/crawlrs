// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Smart 聚合搜索引擎演示
//!
//! 并发查询 Baidu + Bing + Sogou → SimHash 去重 → RRF 融合 → 相关度评分

use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::engines::engine_client::{EngineClient, ScraperEngine};
use crawlrs::search::engine_trait::SearchRequest;
use crawlrs::search::smart::create_smart_search;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("=== Smart Search Engine Demo ===");
    println!("Concurrent: Baidu + Bing + Sogou -> Dedup -> RRF -> Scoring\n");

    let reqwest_engine: Arc<dyn ScraperEngine> = Arc::new(ReqwestEngine::new_with_timeout_and_mrt(
        Arc::new(
            reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap(),
        ),
        60,
        Duration::from_secs(30),
    ));
    let engine_client = Arc::new(EngineClient::with_engines(vec![reqwest_engine]));
    let smart_engine = create_smart_search(engine_client);

    let query = "rust programming language";
    println!("Query: \"{}\"", query);
    println!("Engine type: {:?}\n", smart_engine.engine_type());

    let request = SearchRequest::new(query).with_limit(10);

    match tokio::time::timeout(Duration::from_secs(90), smart_engine.search(&request)).await {
        Ok(Ok(response)) => {
            println!(
                "Results: {} items (engine: {:?})\n",
                response.items.len(),
                response.engine
            );
            for (i, item) in response.items.iter().enumerate() {
                println!("  {:2}. [{}] {}", i + 1, item.engine.name(), item.title);
                println!("      {}", item.url);
                if !item.description.is_empty() {
                    let desc = if item.description.chars().count() > 120 {
                        format!(
                            "{}...",
                            item.description.chars().take(120).collect::<String>()
                        )
                    } else {
                        item.description.clone()
                    };
                    println!("      {}", desc);
                }
                println!();
            }
        }
        Ok(Err(e)) => {
            println!("Search failed: {:?}", e);
        }
        Err(_) => {
            println!("Search timed out (90s)");
        }
    }
}
