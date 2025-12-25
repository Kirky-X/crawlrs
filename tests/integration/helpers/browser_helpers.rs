// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crawlrs::engines::playwright_engine::PlaywrightEngine;
use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
use std::time::Duration;

pub fn create_scrape_request(
    url: String,
    needs_js: bool,
    timeout_secs: u64,
) -> ScrapeRequest {
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

pub fn get_remote_chrome_ws_url() -> String {
    std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL")
        .unwrap_or_else(|_| "ws://localhost:9222/devtools/browser/default".to_string())
}

pub fn set_remote_chrome_url(url: &str) {
    std::env::set_var("CHROMIUM_REMOTE_DEBUGGING_URL", url);
}
