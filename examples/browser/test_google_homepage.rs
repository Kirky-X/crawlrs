// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Test Google homepage loading

use crawlrs::engines::engine_client::{EngineClient, ScrapeRequest};
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🚀 Testing Google homepage loading...\n");

    let client = EngineClient::new();

    // Test 1: Google homepage
    println!("Test 1: Loading Google homepage...");
    let request = ScrapeRequest::new("https://www.google.com")
        .needs_js()
        .timeout(Duration::from_secs(30));

    match client.scrape(&request).await {
        Ok(response) => {
            println!("✅ Google homepage loaded!");
            println!("   Status: {}", response.status_code);
            println!("   Content length: {} bytes", response.content.len());

            // Check for CAPTCHA
            let content = &response.content;
            if content.contains("CAPTCHA")
                || content.contains("captcha")
                || content.contains("验证码")
            {
                println!("   ⚠️  Detected CAPTCHA page!");
            }
        }
        Err(e) => {
            println!("❌ Google homepage failed: {:?}", e);
        }
    }

    println!("\nTest 2: Loading Google search results...");
    let request = ScrapeRequest::new("https://www.google.com/search?q=test")
        .needs_js()
        .timeout(Duration::from_secs(60));

    match client.scrape(&request).await {
        Ok(response) => {
            println!("✅ Google search loaded!");
            println!("   Status: {}", response.status_code);
            println!("   Content length: {} bytes", response.content.len());
        }
        Err(e) => {
            println!("❌ Google search failed: {:?}", e);
        }
    }
}
