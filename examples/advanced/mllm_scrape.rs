// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! MLLM 自主导航爬取示例
//!
//! 演示如何使用 `needs_mllm()` 标记发起 MLLM（多模态大语言模型）自主导航爬取请求。
//! MLLM 引擎通过视觉模型分析页面截图，自主决定导航动作（点击/滚动/输入），
//! 直到提取到目标内容或达到最大迭代次数。
//!
//! # 前置条件
//!
//! - 编译时需启用 `engine-mllm` feature（隐含 `engine-playwright` + `llm`）
//! - 需配置视觉模型 API key（如 `GEMINI_API_KEY` 环境变量）
//! - 需安装 Playwright 浏览器（`cargo run --bin crawlrs -- migrate`）
//!
//! # 使用方法
//!
//! ```bash
//! # 设置视觉模型 API key
//! export GEMINI_API_KEY="your-api-key"
//!
//! # 运行示例
//! cargo run --example mllm_scrape --features full
//! ```

use crawlrs::engines::engine_client::{EngineClient, ScrapeRequest};
use log::info;
use std::time::Duration;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🤖 MLLM 自主导航爬取示例");
    info!("=====================================\n");

    let client = EngineClient::new();

    // 示例 1: 基础 MLLM 爬取
    // 使用 needs_mllm() 标记，引擎路由器会自动选择 MLLM 引擎
    info!("📋 示例 1: 基础 MLLM 爬取");
    let request = ScrapeRequest::new("https://example.com")
        .needs_mllm()
        .timeout(Duration::from_secs(120));

    match client.scrape(&request).await {
        Ok(response) => {
            info!("  ✅ 状态码: {}", response.status_code);
            info!("  📄 内容长度: {} 字节", response.content.len());
            info!("  ⏱️ 响应时间: {} ms", response.response_time_ms);
        }
        Err(e) => {
            info!("  ❌ 错误: {:?}", e);
            info!("  💡 提示: 确保已启用 engine-mllm feature 并配置视觉模型 API key");
        }
    }

    info!("");

    // 示例 2: MLLM + 截图 + JS 渲染
    // MLLM 引擎本身会截图并分析，但可以额外标记 needs_screenshot 保存截图
    info!("📋 示例 2: MLLM + 截图 + JS 渲染");
    let request = ScrapeRequest::new("https://example.com/products")
        .needs_mllm()
        .needs_js()
        .needs_screenshot()
        .timeout(Duration::from_secs(180));

    match client.scrape(&request).await {
        Ok(response) => {
            info!("  ✅ 状态码: {}", response.status_code);
            info!("  📄 内容长度: {} 字节", response.content.len());
            info!("  ⏱️ 响应时间: {} ms", response.response_time_ms);
            if let Some(screenshot) = &response.screenshot {
                info!("  📸 截图大小: {} 字节", screenshot.len());
            }
        }
        Err(e) => {
            info!("  ❌ 错误: {:?}", e);
        }
    }

    info!("");

    // 示例 3: 对比 MLLM 与普通爬取
    // 同一个 URL，分别用普通模式和 MLLM 模式爬取，对比结果
    info!("📋 示例 3: 普通爬取 vs MLLM 爬取对比");
    let url = "https://example.com";

    // 普通爬取
    let normal_request = ScrapeRequest::new(url).timeout(Duration::from_secs(30));
    let normal_result = client.scrape(&normal_request).await;

    // MLLM 爬取
    let mllm_request = ScrapeRequest::new(url)
        .needs_mllm()
        .timeout(Duration::from_secs(120));
    let mllm_result = client.scrape(&mllm_request).await;

    info!("  普通爬取:");
    match normal_result {
        Ok(r) => {
            info!(
                "    状态码: {}, 内容: {} 字节, 耗时: {} ms",
                r.status_code,
                r.content.len(),
                r.response_time_ms
            );
        }
        Err(e) => info!("    错误: {:?}", e),
    }

    info!("  MLLM 爬取:");
    match mllm_result {
        Ok(r) => {
            info!(
                "    状态码: {}, 内容: {} 字节, 耗时: {} ms",
                r.status_code,
                r.content.len(),
                r.response_time_ms
            );
        }
        Err(e) => info!("    错误: {:?}", e),
    }

    info!("\n=====================================");
    info!("✨ MLLM 自主导航爬取示例完成");
    info!("");
    info!("📖 配置说明:");
    info!("  - [engines.mllm] 配置段控制 MLLM 引擎参数");
    info!("  - vision_model: 视觉模型标识（如 gemini:gemini-2.0-flash）");
    info!("  - max_iterations: agentic loop 最大轮次（默认 10）");
    info!("  - mrt_seconds: 单引擎最大响应时间（默认 60s）");
}
