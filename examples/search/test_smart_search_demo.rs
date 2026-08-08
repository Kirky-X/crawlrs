// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::engines::engine_client::{EngineClient, ScraperEngine};
use crawlrs::search::smart as smart_search;
use crawlrs::search::SearchRequest;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🚀 测试智能搜索功能");

    // 创建引擎客户端
    let reqwest_engine: Arc<dyn ScraperEngine> = Arc::new(ReqwestEngine::new_with_timeout_and_mrt(
        Arc::new(
            reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap(),
        ),
        60,
        Duration::from_secs(30),
    ));
    let engine_client = Arc::new(EngineClient::with_engines(vec![reqwest_engine]));

    // 创建智能搜索引擎（Baidu 不需要 JS，ReqwestEngine 可处理）
    let smart_engine = smart_search::create_baidu_smart_search(engine_client);

    println!("🔍 执行搜索测试...");
    let request = SearchRequest::new("rust programming").with_limit(5);
    match smart_engine.search(&request).await {
        Ok(results) => {
            println!("✅ 搜索成功！找到 {} 个结果", results.items.len());
            for (i, result) in results.items.iter().enumerate() {
                println!("  {}. {} - {}", i + 1, result.title, result.url);
            }
        }
        Err(e) => {
            println!("❌ 搜索失败: {:?}", e);
        }
    }
}
