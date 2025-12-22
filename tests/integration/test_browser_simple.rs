use crawlrs::engines::playwright_engine::PlaywrightEngine;
use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
use std::collections::HashMap;
use std::time::Duration;

fn main() {
    println!("=== 浏览器连接测试 ===");

    // 设置远程调试URL
    std::env::set_var("CHROMIUM_REMOTE_DEBUGGING_URL", "http://localhost:9222");

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let engine = PlaywrightEngine;

        // 测试1: 简单的HTTP页面访问
        println!("\n1. 测试访问 httpbin.org...");
        let request = ScrapeRequest {
            url: "https://httpbin.org/html".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(15),
            needs_js: false, // 不需要JS的简单页面
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
                println!("✅ 成功访问页面");
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
    });
}