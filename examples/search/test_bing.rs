// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Bing 搜索引擎真实搜索测试

use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::engines::engine_client::{EngineClient, ScraperEngine};
use crawlrs::search::client::BingSearchEngine;
use crawlrs::search::SearchEngine;
use crawlrs::search::SearchRequest;
use log::info;
use std::sync::Arc;
use tokio::time::Duration;

const TIMEOUT_SECS: u64 = 60;

#[tokio::main]
async fn main() {
    env_logger::init();

    info!("==========================================");
    info!("测试 Bing 搜索引擎真实搜索功能");
    info!("测试关键词: gemini-3-pro");
    info!("超时时间: {} 秒", TIMEOUT_SECS);
    info!("==========================================");
    info!("");

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
    let engine = BingSearchEngine::new(engine_client);
    let request = SearchRequest::new("gemini-3-pro").with_limit(10);

    match engine.search(&request).await {
        Ok(response) => {
            info!("[SUCCESS] Bing 搜索成功完成");
            info!("  总结果数: {}", response.items.len());
            for (i, result) in response.items.iter().enumerate() {
                info!("  结果 {}: {}", i + 1, result.title);
            }
        }
        Err(e) => {
            info!("[FAILED] Bing 搜索出错: {:?}", e);
        }
    }

    info!("");
    info!("==========================================");
    info!("测试完成");
}
