// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Google Search engine test - uses EngineClient smart routing
//!
//! This test demonstrates EngineClient's automatic engine selection:
//! - If needs_js=false: Reqwest gets 100, others get low scores
//! - If needs_js=true: FlareSolverr/Playwright gets 100, Reqwest gets 10
//!
//! Run with: cargo run --features "search-google,engine-reqwest,engine-flaresolverr" --example test_google

use crawlrs::engines::client::flare_solverr::FlareSolverrEngine;
use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::engines::engine_client::EngineClient;
use crawlrs::engines::traits::ScraperEngine;
use crawlrs::search::client::google::GoogleSearchEngine;
use crawlrs::search::SearchEngine;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    println!("🚀 EngineClient Smart Routing Test for Google\n");

    // Create engines - EngineClient will automatically select based on request requirements
    let reqwest_engine: Arc<dyn ScraperEngine> = Arc::new(ReqwestEngine::default());
    let flaresolverr_engine: Arc<dyn ScraperEngine> = Arc::new(FlareSolverrEngine::new());

    // Register all engines - EngineClient's smart routing will select optimal one
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine, flaresolverr_engine];
    let engine_client = Arc::new(EngineClient::with_engines(engines));

    println!(
        "✅ EngineClient initialized with {} engines",
        engine_client.engine_count()
    );
    println!(
        "   Registered engines: {:?}\n",
        engine_client.registered_engines()
    );
    println!("🌐 Testing Google Search (EngineClient will auto-select optimal engine)...\n");

    match GoogleSearchEngine::new(engine_client)
        .search(&crawlrs::search::SearchRequest::new("test"))
        .await
    {
        Ok(response) => {
            println!("✅ Google search successful!");
            println!("Results: {}\n", response.items.len());

            if response.items.is_empty() {
                println!("⚠️ No results parsed");
                println!("💡 This might indicate:");
                println!("   - CAPTCHA page returned (Google detected automation)");
                println!("   - Parsing selectors need updating");
            } else {
                for (i, item) in response.items.iter().enumerate() {
                    println!("{}. {}", i + 1, item.title);
                    println!("   URL: {}\n", item.url);
                }
            }
        }
        Err(e) => {
            println!("❌ Google search failed: {:?}", e);
        }
    }
}
