// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Test Google homepage loading

use crawlrs::engines::client::playwright::PlaywrightEngine;
use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🚀 Testing Google homepage loading...\n");

    let engine = PlaywrightEngine;

    // Test 1: Google homepage
    println!("Test 1: Loading Google homepage...");
    let request = ScrapeRequest {
        url: "https://www.google.com".to_string(),
        headers: std::collections::HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: true,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: vec![],
        sync_wait_ms: 3000,
    };

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
    let request = ScrapeRequest {
        url: "https://www.google.com/search?q=test".to_string(),
        headers: std::collections::HashMap::new(),
        timeout: Duration::from_secs(60),
        needs_js: true,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: vec![],
        sync_wait_ms: 5000,
    };

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
