// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Test Google search and show content preview

use crawlrs::engines::engine_client::{EngineClient, ScrapeRequest};
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🚀 Testing Google search with content preview...\n");

    let client = EngineClient::new();

    println!("Loading Google search results for 'test'...\n");
    let request = ScrapeRequest::new("https://www.google.com/search?q=test")
        .needs_js()
        .timeout(Duration::from_secs(60));

    match client.scrape(&request).await {
        Ok(response) => {
            println!("✅ Page loaded!");
            println!("   Status: {}", response.status_code);
            println!("   Content length: {} bytes\n", response.content.len());

            // Check for CAPTCHA
            let content = &response.content;
            let is_captcha = content.contains("CAPTCHA")
                || content.contains("captcha")
                || content.contains("验证码")
                || content.contains("unusual traffic");

            println!(
                "CAPTCHA detected: {}",
                if is_captcha { "YES ⚠️" } else { "NO" }
            );

            // Show first 1000 characters
            let preview_len = std::cmp::min(1500, content.len());
            println!("\n📄 Content preview (first {} chars):\n", preview_len);
            println!("{}", &content[..preview_len]);
        }
        Err(e) => {
            println!("❌ Failed: {:?}", e);
        }
    }
}
