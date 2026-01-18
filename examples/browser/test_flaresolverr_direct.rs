// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Debug Google search - shows raw HTML content
//!
//! This example requires the `engine-flaresolverr` feature to be enabled.
//! Run with: cargo run --bin test_flaresolverr_direct --features engine-flaresolverr

#[cfg(feature = "engine-flaresolverr")]
use crawlrs::engines::client::flare_solverr::FlareSolverrEngine;
use crawlrs::engines::engine_client::{EngineClient, ScrapeRequest};
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🚀 Testing FlareSolverr directly...\n");

    #[cfg(feature = "engine-flaresolverr")]
    {
        let engine = FlareSolverrEngine::new();

        println!("Loading Google search results...\n");
        let request = ScrapeRequest::new("https://www.google.com/search?q=test")
            .timeout(Duration::from_secs(90))
            .needs_js();

        match client.scrape(&request).await {
            Ok(response) => {
                println!("✅ Page loaded!");
                println!("   Status: {}", response.status_code);
                println!("   Content length: {} bytes\n", response.content.len());

                // Check for CAPTCHA
                let content = &response.content;
                let is_captcha = content.contains("CAPTCHA")
                    || content.contains("captcha")
                    || content.contains("验证码");

                println!(
                    "CAPTCHA detected: {}",
                    if is_captcha { "YES ⚠️" } else { "NO" }
                );

                // Check for search results
                let has_results = content.contains("class=\"g\"")
                    || content.contains("search result")
                    || content.contains("About");
                println!(
                    "Has search results: {}",
                    if has_results { "YES ✅" } else { "NO ❌" }
                );

                // Show first 1500 characters
                let preview_len = std::cmp::min(1500, content.len());
                println!("\n📄 Content preview (first {} chars):\n", preview_len);
                println!("{}", &content[..preview_len]);
            }
            Err(e) => {
                println!("❌ Failed: {:?}", e);
            }
        }
    }

    #[cfg(not(feature = "engine-flaresolverr"))]
    {
        println!("⚠️  This example requires the `engine-flaresolverr` feature.");
        println!("   To run this example, use:");
        println!("   cargo run --bin test_flaresolverr_direct --features engine-flaresolverr");
        println!();
        println!("   Using basic EngineClient instead...");

        let client = EngineClient::new();
        let request = ScrapeRequest::new("https://example.com").timeout(Duration::from_secs(30));

        match client.scrape(&request).await {
            Ok(response) => {
                println!("✅ Basic scrape successful!");
                println!("   Status: {}", response.status_code);
                println!("   Content length: {} bytes", response.content.len());
            }
            Err(e) => {
                println!("❌ Failed: {:?}", e);
            }
        }
    }
}
