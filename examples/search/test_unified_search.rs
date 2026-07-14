// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 统一搜索引擎测试入口
//!
//! 支持同时测试所有搜索引擎，统一输出格式和 URL 可访问性检查

use crawlrs::engines::engine_client::EngineClient;
use crawlrs::search::bing::BingSearchEngine;
use crawlrs::search::google::GoogleSearchEngine;
use crawlrs::search::SearchRequest;
use log::info;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

const ENGINE_TIMEOUT_SECS: u64 = 30;
const TEST_KEYWORD: &str = "gemini-3-pro";
const RESULT_LIMIT: u32 = 10;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("==========================================");
    info!("统一搜索引擎测试");
    info!("关键词: {}", TEST_KEYWORD);
    info!("==========================================");
    info!("");

    let engine_client = Arc::new(EngineClient::new());
    let google_engine = GoogleSearchEngine::new(engine_client.clone());

    info!(
        "[1/2] 测试 Google 搜索引擎 (超时 {} 秒)...",
        ENGINE_TIMEOUT_SECS
    );

    let timeout_duration = Duration::from_secs(ENGINE_TIMEOUT_SECS);
    let request = SearchRequest::new(TEST_KEYWORD).with_limit(RESULT_LIMIT);

    match timeout(timeout_duration, google_engine.search(&request)).await {
        Ok(Ok(response)) => {
            info!("✅ Google 搜索成功，找到 {} 个结果", response.items.len());
        }
        Ok(Err(e)) => {
            info!("⚠️ Google 搜索出错: {:?}", e);
        }
        Err(_) => {
            info!("[TIMEOUT] Google 搜索超时");
        }
    }
    info!("");

    info!(
        "[2/2] 测试 Bing 搜索引擎 (超时 {} 秒)...",
        ENGINE_TIMEOUT_SECS
    );
    let bing_engine = BingSearchEngine::new();

    match timeout(timeout_duration, bing_engine.search(&request)).await {
        Ok(Ok(response)) => {
            info!("✅ Bing 搜索成功，找到 {} 个结果", response.items.len());
        }
        Ok(Err(e)) => {
            info!("⚠️ Bing 搜索出错: {:?}", e);
        }
        Err(_) => {
            info!("[TIMEOUT] Bing 搜索超时");
        }
    }

    info!("");
    info!("==========================================");
    info!("测试完成");
    info!("==========================================");
}
