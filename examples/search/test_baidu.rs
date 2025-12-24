// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

//! Baidu 搜索引擎真实搜索测试

use crawlrs::infrastructure::search::baidu::BaiduSearchEngine;
use crawlrs::utils::search_test::run_engine_test_with_output;
use tokio::time::{timeout, Duration};
use tracing::info;

const TEST_KEYWORD: &str = "gemini-3-pro";
const TIMEOUT_SECS: u64 = 60;
const RESULT_LIMIT: u32 = 10;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(true)
        .init();

    info!("==========================================");
    info!("测试 Baidu 搜索引擎真实搜索功能");
    info!("测试关键词: {}", TEST_KEYWORD);
    info!("超时时间: {} 秒", TIMEOUT_SECS);
    info!("==========================================");

    let timeout_duration = Duration::from_secs(TIMEOUT_SECS);

    match timeout(timeout_duration, run_engine_test_with_output(
        "Baidu",
        BaiduSearchEngine::new(),
        Some(TEST_KEYWORD),
        TIMEOUT_SECS,
        Some(RESULT_LIMIT),
    )).await {
        Ok(Ok(result)) => {
            info!("");
            info!("[SUCCESS] Baidu 搜索成功完成");
            info!("  总结果数: {}", result.total);
            info!("  ✅ 可访问: {}", result.accessible);
            info!("  ❌ 不可访问: {}", result.inaccessible);
        }
        Ok(Err(e)) => {
            info!("[FAILED] Baidu 搜索出错: {:?}", e);
        }
        Err(_) => {
            info!("[TIMEOUT] Baidu 搜索超时 ({} 秒)", TIMEOUT_SECS);
        }
    }

    info!("==========================================");
    info!("测试完成");
}
