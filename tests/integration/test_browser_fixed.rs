// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crawlrs::engines::fire_engine_cdp::FireEngineCdp;
use crawlrs::engines::fire_engine_tls::FireEngineTls;
use crawlrs::engines::playwright_engine::PlaywrightEngine;
use crawlrs::engines::reqwest_engine::ReqwestEngine;
use crawlrs::engines::router::EngineRouter;
use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

fn main() {
    println!("=== 智能路由引擎测试 ===");

    // 设置远程调试URL
    std::env::set_var("CHROMIUM_REMOTE_DEBUGGING_URL", "http://localhost:9222");
    std::env::set_var("RUST_LOG", "info");

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // 创建所有引擎
        let reqwest_engine = Arc::new(ReqwestEngine);
        let playwright_engine = Arc::new(PlaywrightEngine);
        let fire_engine_tls = Arc::new(FireEngineTls::new());
        let fire_engine_cdp = Arc::new(FireEngineCdp::new());

        // 创建引擎列表（按优先级排序）
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            reqwest_engine,    // 最快，适合简单页面
            playwright_engine, // 支持JS，适合复杂页面
            fire_engine_tls,   // TLS指纹，适合反爬虫
            fire_engine_cdp,   // CDP支持，适合高级功能
        ];

        // 创建路由器
        let router = EngineRouter::new(engines);

        // 测试不同类型的页面
        let test_urls = vec![
            ("https://example.com", "简单页面"),
            ("https://httpbin.org/html", "HTML测试页面"),
            ("https://www.google.com", "Google首页"),
        ];

        for (url, description) in test_urls {
            println!("\n🔍 测试 {}: {}", description, url);
            
            let request = ScrapeRequest {
                url: url.to_string(),
                headers: HashMap::new(),
                timeout: Duration::from_secs(30),
                needs_js: url.contains("google"), // Google需要JS
                needs_screenshot: false,
                screenshot_config: None,
                mobile: false,
                proxy: None,
                skip_tls_verification: true,
                needs_tls_fingerprint: url.contains("google"), // Google需要TLS指纹
                use_fire_engine: false,
            };

            match router.scrape(request).await {
                Ok(response) => {
                    println!("✅ 成功访问 {}", description);
                    println!("状态码: {:?}", response.status_code);
                    println!("内容长度: {} 字符", response.content.len());
                    println!("使用引擎: {:?}", response.engine_used);
                    
                    if response.content.len() > 100 {
                        println!("前100个字符: {}", &response.content[..100]);
                    }
                }
                Err(e) => {
                    println!("❌ 访问 {} 失败: {:?}", description, e);
                }
            }
            
            // 等待一下，避免过于频繁的请求
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}