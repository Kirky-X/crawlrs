use crawlrs::engines::playwright_engine::PlaywrightEngine;
use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
use std::collections::HashMap;
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("=== 直接测试Playwright连接Google ===");

    // 设置远程Chrome调试URL
    std::env::set_var(
        "CHROMIUM_REMOTE_DEBUGGING_URL",
        "ws://localhost:9222/devtools/browser/16bfd1e5-af2b-45c4-85c2-9d8ac98d2817",
    );

    let engine = PlaywrightEngine;

    // 测试1: 访问简单页面
    println!("\n1. 测试访问 example.com...");
    let request1 = ScrapeRequest {
        url: "https://example.com".to_string(),
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: true,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: true,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
    };

    match engine.scrape(request1).await {
        Ok(response) => {
            println!("✅ 成功访问 example.com");
            println!("状态码: {:?}", response.status_code);
            println!("内容长度: {} 字符", response.content.len());
        }
        Err(e) => {
            println!("❌ 访问失败: {:?}", e);
        }
    }

    // 测试2: 访问Google搜索
    println!("\n2. 测试访问Google搜索...");
    let request2 = ScrapeRequest {
        url: "https://www.google.com/search?q=rust+programming".to_string(),
        headers: HashMap::new(),
        timeout: Duration::from_secs(45),
        needs_js: true,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: true,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
    };

    match engine.scrape(request2).await {
        Ok(response) => {
            println!("✅ 成功访问Google搜索");
            println!("状态码: {:?}", response.status_code);
            println!("内容长度: {} 字符", response.content.len());
            if response.content.len() > 200 {
                println!("前200个字符: {}", &response.content[..200]);
            }
        }
        Err(e) => {
            println!("❌ Google搜索访问失败: {:?}", e);
        }
    }
}