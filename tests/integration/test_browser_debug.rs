use crawlrs::engines::playwright_engine::PlaywrightEngine;
use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
use std::collections::HashMap;
use std::time::Duration;

fn main() {
    println!("=== 浏览器连接调试测试 ===");

    // 设置远程调试URL
    std::env::set_var("CHROMIUM_REMOTE_DEBUGGING_URL", "http://localhost:9222");
    std::env::set_var("RUST_LOG", "debug");

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let engine = PlaywrightEngine;

        println!(
            "环境变量 CHROMIUM_REMOTE_DEBUGGING_URL: {:?}",
            std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL").ok()
        );

        // 测试简单的页面访问
        println!("\n测试访问 example.com（不需要JS）...");
        let request = ScrapeRequest {
            url: "https://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: true,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };

        match engine.scrape(request).await {
            Ok(response) => {
                println!("✅ 成功访问 example.com");
                println!("状态码: {:?}", response.status_code);
                println!("内容长度: {} 字符", response.content.len());
                if response.content.len() > 100 {
                    println!("前100个字符: {}", &response.content[..100]);
                }
            }
            Err(e) => {
                println!("❌ 访问失败: {:?}", e);
            }
        }

        // 测试需要JS的页面
        println!("\n测试访问 Google（需要JS）...");
        let request2 = ScrapeRequest {
            url: "https://www.google.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(15),
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
                println!("✅ 成功访问 Google");
                println!("状态码: {:?}", response.status_code);
                println!("内容长度: {} 字符", response.content.len());
                if response.content.len() > 200 {
                    println!("前200个字符: {}", &response.content[..200]);
                }
            }
            Err(e) => {
                println!("❌ Google 访问失败: {:?}", e);
            }
        }
    });
}