// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 异步批量处理示例
//!
//! 演示如何使用 `tokio::spawn` + `JoinSet` 并发执行多个爬取任务，
//! 并统一收集结果。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example async_batch
//! ```

use crawlrs::engines::engine_client::{EngineClient, ScrapeRequest};
use log::info;
use std::time::Duration;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 异步批量处理示例");
    info!("=====================================\n");

    let client = EngineClient::new();

    // 1. 准备批量目标 URL
    let urls = vec![
        "https://example.com",
        "https://example.org",
        "https://example.net",
        "https://httpbin.org/html",
        "https://httpbin.org/json",
    ];

    info!("📋 批量任务数量: {}", urls.len());
    info!("");

    // 2. 使用 JoinSet 并发执行
    let mut join_set = tokio::task::JoinSet::new();

    for (idx, url) in urls.iter().enumerate() {
        let client_clone = client.clone();
        let url_clone = url.to_string();

        join_set.spawn(async move {
            let request = ScrapeRequest::new(&url_clone).timeout(Duration::from_secs(30));
            let result = client_clone.scrape(&request).await;
            (idx, url_clone, result)
        });
    }

    // 3. 收集结果
    let mut success_count = 0usize;
    let mut fail_count = 0usize;

    info!("🔄 等待所有任务完成...");
    info!("");

    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok((idx, url, Ok(response))) => {
                success_count += 1;
                info!(
                    "  ✅ [{}] {} — 状态码: {}, 内容: {} 字节",
                    idx, url, response.status_code, response.content.len()
                );
            }
            Ok((idx, url, Err(e))) => {
                fail_count += 1;
                info!("  ❌ [{}] {} — 错误: {:?}", idx, url, e);
            }
            Err(join_err) => {
                fail_count += 1;
                info!("  ❌ Task panicked: {:?}", join_err);
            }
        }
    }

    // 4. 汇总统计
    info!("");
    info!("📊 批量处理统计:");
    info!("  总数: {}", urls.len());
    info!("  成功: {}", success_count);
    info!("  失败: {}", fail_count);

    info!("\n=====================================");
    info!("✨ 异步批量处理示例完成");
}
