// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Debug Google search - shows raw HTML content

use crawlrs::engines::client::flare_solverr::FlareSolverrEngine;
use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🚀 Testing FlareSolverr directly...\n");

    let engine = FlareSolverrEngine::new();

    println!("Loading Google search results...\n");
    let request = ScrapeRequest {
        url: "https://www.google.com/search?q=test".to_string(),
        headers: std::collections::HashMap::new(),
        timeout: Duration::from_secs(90),
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
