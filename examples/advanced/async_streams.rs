// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 异步流式处理示例
//!
//! 演示如何使用 `tokio::sync::mpsc` 通道将爬取结果以流式方式逐步输出，
//! 适用于大量 URL 场景下避免一次性加载所有结果到内存。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example async_streams
//! ```

use crawlrs::engines::engine_client::{EngineClient, ScrapeRequest};
use log::info;
use std::time::Duration;
use tokio::sync::mpsc;

/// 单个爬取结果
struct ScrapeResult {
    url: String,
    status: Result<u16, String>,
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 异步流式处理示例");
    info!("=====================================\n");

    let client = EngineClient::new();

    let urls = vec![
        "https://example.com",
        "https://httpbin.org/html",
        "https://httpbin.org/json",
        "https://example.org",
    ];

    // 1. 创建 mpsc 通道（背压控制，缓冲区大小 = 2）
    let (tx, mut rx) = mpsc::channel::<ScrapeResult>(2);

    // 2. 生产者：并发爬取，逐条发送到通道
    let client_producer = client.clone();
    let urls_producer = urls.clone();
    let producer = tokio::spawn(async move {
        for url in &urls_producer {
            let request = ScrapeRequest::new(*url).timeout(Duration::from_secs(30));
            let result = match client_producer.scrape(&request).await {
                Ok(response) => ScrapeResult {
                    url: url.to_string(),
                    status: Ok(response.status_code),
                },
                Err(e) => ScrapeResult {
                    url: url.to_string(),
                    status: Err(format!("{:?}", e)),
                },
            };
            // 逐条发送，通道满时自动背压等待
            if tx.send(result).await.is_err() {
                log::error!("Receiver dropped, aborting producer");
                break;
            }
        }
    });

    // 3. 消费者：逐条处理结果（流式输出）
    info!("🔄 流式处理结果:");
    info!("-----------------------------");

    let mut processed = 0usize;
    while let Some(result) = rx.recv().await {
        processed += 1;
        match &result.status {
            Ok(code) => {
                info!(
                    "  [{}/{}] ✅ {} — HTTP {}",
                    processed,
                    urls.len(),
                    result.url,
                    code
                );
            }
            Err(e) => {
                info!("  [{}/{}] ❌ {} — {}", processed, urls.len(), result.url, e);
            }
        }
    }

    // 等待生产者完成
    let _ = producer.await;

    info!("");
    info!("📊 共处理 {} 个 URL", processed);

    info!("\n=====================================");
    info!("✨ 异步流式处理示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - mpsc 通道提供背压控制，防止生产者过快导致内存暴涨");
    info!("   - 适合处理大量 URL 的流式场景");
    info!("   - 缓冲区大小可根据内存和延迟需求调整");
}
