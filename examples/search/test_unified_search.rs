// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(deprecated)]

//! 统一搜索引擎测试入口
//!
//! 支持同时测试所有搜索引擎，统一输出格式和 URL 可访问性检查

use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::engines::engine_client::EngineClient;
use crawlrs::engines::traits::ScraperEngine;
use crawlrs::search::client::baidu::BaiduSearchEngine;
use crawlrs::search::client::bing::BingSearchEngine;
use crawlrs::search::client::google::GoogleSearchEngine;
use crawlrs::search::client::sogou::SogouSearchEngine;
use crawlrs::utils::search_test::{run_engine_test_with_output, TestResult};
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tracing::info;

const ENGINE_TIMEOUT_SECS: u64 = 30;
const TEST_KEYWORD: &str = "gemini-3-pro";
const RESULT_LIMIT: u32 = 10;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(true)
        .init();

    info!("==========================================");
    info!("统一搜索引擎测试");
    info!("关键词: {}", TEST_KEYWORD);
    info!("==========================================");
    info!("");

    let mut google_result = Ok(TestResult::default());
    let mut bing_result = Ok(TestResult::default());
    let mut baidu_result = Ok(TestResult::default());
    let mut sogou_result = Ok(TestResult::default());

    let timeout_duration = Duration::from_secs(ENGINE_TIMEOUT_SECS);

    info!(
        "[1/4] 测试 Google 搜索引擎 (超时 {} 秒)...",
        ENGINE_TIMEOUT_SECS
    );
    // Create EngineClient
    let reqwest_engine = Arc::new(ReqwestEngine::default());
    let fire_engine_cdp = Arc::new(crawlrs::engines::client::fire_cdp::FireEngineCdp::new());
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine, fire_engine_cdp];
    let engine_client = Arc::new(EngineClient::with_engines(engines));
    let google_engine = GoogleSearchEngine::new(engine_client);
    match timeout(
        timeout_duration,
        run_engine_test_with_output(
            "Google",
            google_engine,
            Some(TEST_KEYWORD),
            ENGINE_TIMEOUT_SECS,
            Some(RESULT_LIMIT),
        ),
    )
    .await
    {
        Ok(result) => google_result = result,
        Err(_) => info!("[TIMEOUT] Google 搜索超时"),
    }
    info!("");

    info!(
        "[2/4] 测试 Bing 搜索引擎 (超时 {} 秒)...",
        ENGINE_TIMEOUT_SECS
    );
    let bing_engine = BingSearchEngine::new();
    match timeout(
        timeout_duration,
        run_engine_test_with_output(
            "Bing",
            bing_engine,
            Some(TEST_KEYWORD),
            ENGINE_TIMEOUT_SECS,
            Some(RESULT_LIMIT),
        ),
    )
    .await
    {
        Ok(result) => bing_result = result,
        Err(_) => info!("[TIMEOUT] Bing 搜索超时"),
    }
    info!("");

    info!(
        "[3/4] 测试 Baidu 搜索引擎 (超时 {} 秒)...",
        ENGINE_TIMEOUT_SECS
    );
    let baidu_engine = BaiduSearchEngine::new();
    match timeout(
        timeout_duration,
        run_engine_test_with_output(
            "Baidu",
            baidu_engine,
            Some(TEST_KEYWORD),
            ENGINE_TIMEOUT_SECS,
            Some(RESULT_LIMIT),
        ),
    )
    .await
    {
        Ok(result) => baidu_result = result,
        Err(_) => info!("[TIMEOUT] Baidu 搜索超时"),
    }
    info!("");

    info!(
        "[4/4] 测试 Sogou 搜索引擎 (超时 {} 秒)...",
        ENGINE_TIMEOUT_SECS
    );
    let sogou_engine = SogouSearchEngine::new();
    match timeout(
        timeout_duration,
        run_engine_test_with_output(
            "Sogou",
            sogou_engine,
            Some(TEST_KEYWORD),
            ENGINE_TIMEOUT_SECS,
            Some(RESULT_LIMIT),
        ),
    )
    .await
    {
        Ok(result) => sogou_result = result,
        Err(_) => info!("[TIMEOUT] Sogou 搜索超时"),
    }

    print_summary(&[
        ("Google", &google_result),
        ("Bing", &bing_result),
        ("Baidu", &baidu_result),
        ("Sogou", &sogou_result),
    ]);

    info!("==========================================");
    info!("所有搜索引擎测试完成");
    info!("==========================================");
}

fn print_summary(results: &[(&str, &anyhow::Result<TestResult>)]) {
    info!("");
    info!("==========================================");
    info!("测试结果汇总");
    info!("==========================================");

    let mut total_accessible = 0;
    let mut total_inaccessible = 0;
    let mut total_success = 0;

    for (name, result) in results {
        match result {
            Ok(test_result) => {
                info!(
                    "  {}: 成功 {} 个, ✅ {} 个, ❌ {} 个",
                    name, test_result.total, test_result.accessible, test_result.inaccessible
                );
                total_accessible += test_result.accessible;
                total_inaccessible += test_result.inaccessible;
                total_success += 1;
            }
            Err(_) => {
                info!("  {}: 测试失败", name);
            }
        }
    }

    info!("");
    info!("总计: 成功测试 {} 个引擎", total_success);
    info!("  ✅ 可访问: {} 个", total_accessible);
    info!("  ❌ 不可访问: {} 个", total_inaccessible);
}
