// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(deprecated)]

#[cfg(feature = "engine-playwright")]
use crawlrs::engines::client::playwright::PlaywrightEngine;
use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
use std::time::Duration;

pub fn create_scrape_request(url: String, needs_js: bool, timeout_secs: u64) -> ScrapeRequest {
    ScrapeRequest {
        url,
        headers: std::collections::HashMap::new(),
        timeout: Duration::from_secs(timeout_secs),
        needs_js,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: true,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        sync_wait_ms: 0,
    }
}

#[allow(dead_code)]
#[cfg(feature = "engine-playwright")]
pub async fn test_page_access(url: &str, needs_js: bool, timeout_secs: u64) -> bool {
    let engine = PlaywrightEngine;
    let request = create_scrape_request(url.to_string(), needs_js, timeout_secs);

    match engine.scrape(&request).await {
        Ok(response) => {
            println!("✅ 成功访问 {}", url);
            println!("状态码: {:?}", response.status_code);
            println!("内容长度: {} 字符", response.content.len());
            true
        }
        Err(e) => {
            println!("❌ 访问失败: {:?}", e);
            false
        }
    }
}

#[allow(dead_code)]
#[cfg(not(feature = "engine-playwright"))]
pub async fn test_page_access(_url: &str, _needs_js: bool, _timeout_secs: u64) -> bool {
    println!("⚠️  Playwright not available, skipping test");
    false
}

#[allow(dead_code)]
pub fn get_chrome_ws_url() -> String {
    std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL")
        .unwrap_or_else(|_| "http://localhost:9222".to_string())
}

#[allow(dead_code)]
pub fn set_chrome_ws_url(url: &str) {
    std::env::set_var("CHROMIUM_REMOTE_DEBUGGING_URL", url);
}

#[allow(dead_code)]
pub fn get_remote_chrome_ws_url() -> String {
    get_chrome_ws_url()
}

#[allow(dead_code)]
pub fn set_remote_chrome_url(url: &str) {
    set_chrome_ws_url(url)
}
