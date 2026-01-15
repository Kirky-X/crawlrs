// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Basic Playwright + Chrome connectivity test

use crawlrs::engines::client::playwright::PlaywrightEngine;
use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🚀 Testing Playwright + Chrome connectivity...\n");

    let engine = PlaywrightEngine;

    // Test 1: Simple page load
    println!("Test 1: Loading example.com...");
    let request = ScrapeRequest {
        url: "https://example.com".to_string(),
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
        sync_wait_ms: 0,
    };

    match engine.scrape(&request).await {
        Ok(response) => {
            println!("✅ Example.com loaded successfully!");
            println!("   Status: {}", response.status_code);
            println!("   Content length: {} bytes", response.content.len());
        }
        Err(e) => {
            println!("❌ Example.com failed: {:?}", e);
        }
    }

    println!("\nTest 2: Loading百度...");
    let request = ScrapeRequest {
        url: "https://www.baidu.com".to_string(),
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

    match engine.scrape(&request).await {
        Ok(response) => {
            println!("✅ Baidu loaded successfully!");
            println!("   Status: {}", response.status_code);
            println!("   Content length: {} bytes", response.content.len());
        }
        Err(e) => {
            println!("❌ Baidu failed: {:?}", e);
        }
    }
}
