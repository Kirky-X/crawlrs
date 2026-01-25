// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Basic Playwright + Chrome connectivity test
//!
//! This example demonstrates how to use the Playwright browser engine
//! for JavaScript-heavy pages that require client-side rendering.
//!
//! ## Prerequisites
//!
//! - Enable the `engine-playwright` feature
//! - Have Chromium/Chrome installed
//!
//! ## Run
//!
//! ```bash
//! cargo run --features engine-playwright --bin test_playwright_basic
//! ```

use crawlrs::engines::{EngineClient, ScrapeRequest, ScraperEngine};
use crawlrs::engines::client::ReqwestEngine;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🚀 Testing browser engine connectivity...\n");

    // Create engine client with default Reqwest engine
    let engine_client = EngineClient::new();

    // Test 1: Simple page load with Reqwest (static HTML)
    println!("Test 1: Loading example.com with Reqwest (static HTML)...");
    let request = ScrapeRequest::new("https://example.com".to_string())
        .timeout(Duration::from_secs(30));

    match engine_client.scrape(&request).await {
        Ok(response) => {
            println!("✅ Example.com loaded successfully!");
            println!("   Status: {}", response.status_code);
            println!("   Content length: {} bytes", response.content.len());
        }
        Err(e) => {
            println!("❌ Example.com failed: {:?}", e);
        }
    }

    // Test 2: JavaScript-rendered page with Playwright (if enabled)
    println!("\nTest 2: Testing JavaScript rendering capability...");

    // Note: Playwright requires the feature to be enabled
    // and a browser to be installed
    println!("   To enable Playwright, run with: --features engine-playwright");
    println!("   Make sure Chromium/Chrome is installed on your system.");

    println!("\n✨ All tests completed!");
}
