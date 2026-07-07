// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 基础爬取示例
//!
//! 演示如何使用 crawlrs 进行最基本的网页爬取操作。
//! 本示例展示如何：
//! - 创建爬取请求
//! - 执行爬取操作
//! - 处理响应结果
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example basic_scrape
//! ```
//!
//! # 预期输出
//!
//! 成功时将显示爬取状态、内容长度等信息。

use crawlrs::engines::engine_client::{EngineClient, ScrapeRequest};
use std::time::Duration;
use log::info;

#[tokio::main]
async fn main() {
    // 初始化日志系统
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始基础爬取示例");
    info!("=====================================\n");

    // 创建引擎客户端
    let client = EngineClient::new();

    // 目标URL
    let url = "https://example.com";

    info!("📌 目标URL: {}", url);

    // 创建爬取请求
    let request = ScrapeRequest::new(url).timeout(Duration::from_secs(30));

    // 执行爬取
    info!("🔄 正在爬取页面...");
    match client.scrape(&request).await {
        Ok(response) => {
            info!("✅ 爬取成功!");
            info!("  状态码: {}", response.status_code);
            info!("  内容长度: {} 字节", response.content.len());
            info!("  MIME类型: {:?}", response.content_type);

            // 显示部分内容（避免输出过长）
            let preview = &response.content[..200.min(response.content.len())];
            info!("  内容预览:\n{}", preview);
        }
        Err(e) => {
            info!("❌ 爬取失败: {:?}", e);
        }
    }

    info!("\n=====================================");
    info!("✨ 基础爬取示例完成");
}
