// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crawlrs::engines::engine::{PlaywrightEngine, ReqwestEngine};
use crawlrs::engines::router::EngineRouter;
use crawlrs::search::smart as smart_search;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    println!("🚀 测试智能搜索功能");

    // 创建引擎
    let reqwest_engine = Arc::new(ReqwestEngine);
    let playwright_engine = Arc::new(PlaywrightEngine);
    let engines: Vec<Arc<dyn crawlrs::engines::traits::ScraperEngine>> =
        vec![reqwest_engine, playwright_engine];

    // 创建路由器
    let router = Arc::new(EngineRouter::new(engines));

    // 创建智能搜索引擎
    let smart_engine = smart_search::create_google_smart_search(router);

    println!("🔍 执行搜索测试...");
    match smart_engine.search("rust programming", 5, None, None).await {
        Ok(results) => {
            println!("✅ 搜索成功！找到 {} 个结果", results.len());
            for (i, result) in results.iter().enumerate() {
                println!("  {}. {} - {}", i + 1, result.title, result.url);
            }
        }
        Err(e) => {
            println!("❌ 搜索失败: {:?}", e);
        }
    }
}
