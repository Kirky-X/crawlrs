// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(deprecated)]

//! Google 搜索引擎真实搜索测试

use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::engines::engine_client::EngineClient;
use crawlrs::engines::traits::ScraperEngine;
use crawlrs::search::client::google::GoogleSearchEngine;
use crawlrs::utils::search_test::run_engine_test_with_output;
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tracing::info;

const TEST_KEYWORD: &str = "gemini-3-pro";
const TIMEOUT_SECS: u64 = 90;
const RESULT_LIMIT: u32 = 10;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(true)
        .init();

    info!("==========================================");
    info!("测试 Google 搜索引擎真实搜索功能");
    info!("测试关键词: {}", TEST_KEYWORD);
    info!("超时时间: {} 秒", TIMEOUT_SECS);
    info!("==========================================");

    let timeout_duration = Duration::from_secs(TIMEOUT_SECS);

    // Create EngineClient
    let reqwest_engine = Arc::new(ReqwestEngine);
    let fire_engine_cdp = Arc::new(crawlrs::engines::client::fire_cdp::FireEngineCdp::new());
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine, fire_engine_cdp];
    let engine_client = Arc::new(EngineClient::with_engines(engines));

    match timeout(
        timeout_duration,
        run_engine_test_with_output(
            "Google",
            GoogleSearchEngine::new(engine_client),
            Some(TEST_KEYWORD),
            TIMEOUT_SECS,
            Some(RESULT_LIMIT),
        ),
    )
    .await
    {
        Ok(Ok(result)) => {
            info!("");
            info!("[SUCCESS] Google 搜索成功完成");
            info!("  总结果数: {}", result.total);
            info!("  ✅ 可访问: {}", result.accessible);
            info!("  ❌ 不可访问: {}", result.inaccessible);
        }
        Ok(Err(e)) => {
            info!("[FAILED] Google 搜索出错: {:?}", e);
        }
        Err(_) => {
            info!("[TIMEOUT] Google 搜索超时 ({} 秒)", TIMEOUT_SECS);
        }
    }

    info!("==========================================");
    info!("测试完成");
}
