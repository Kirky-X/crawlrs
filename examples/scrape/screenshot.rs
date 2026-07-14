// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 页面截图示例
//!
//! 演示如何使用 crawlrs 进行页面截图，包括：
//! - 配置截图选项
//! - 全页面截图 vs 区域截图
//! - 截图质量设置
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example screenshot
//! ```
//!
//! # 注意事项
//!
//! 截图功能需要启用 `engine-playwright` 特性。

use crawlrs::engines::engine_client::{EngineClient, ScrapeOptions, ScreenshotConfig};
use log::info;
use std::time::Duration;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始页面截图示例");
    info!("=====================================\n");

    let client = EngineClient::new();
    let client = EngineClient::new();
    let url = "https://example.com";

    info!("📸 目标页面: {}", url);
    info!("");

    // 1. 基本截图
    info!("1️⃣  基本截图（默认设置）");
    info!("-----------------------------");

    let options = ScrapeOptions::default()
        .needs_screenshot(true)
        .timeout(Duration::from_secs(60));

    let request = crawlrs::engines::engine_client::ScrapeRequest::new(url).with_options(options);

    match client.scrape(&request).await {
        Ok(response) => {
            if let Some(screenshot) = response.screenshot {
                info!("✅ 截图成功");
                info!("  大小: {} 字节", screenshot.len());
                info!("  Base64长度: {} 字符", screenshot.len() * 4 / 3);
                info!("  前缀: {}...", &screenshot[..40.min(screenshot.len())]);
            } else {
                info!("⚠️  未返回截图（需要Playwright引擎）");
                info!(
                    "   提示: 使用 `cargo run --features engine-playwright --example screenshot`"
                );
            }
        }
        Err(e) => {
            info!("❌ 截图失败: {:?}", e);
        }
    }
    info!("");

    // 2. 演示截图配置（即使无法实际截图，也展示API使用）
    info!("2️⃣  截图配置示例");
    info!("-----------------------------");

    // 全页面截图配置
    let full_page_config = ScreenshotConfig::default()
        .full_page(true)
        .quality(85)
        .format("png");

    // 区域截图配置
    let region_config = ScreenshotConfig::default()
        .selector("#main-content")
        .quality(90)
        .format("jpeg");

    info!("✅ 截图配置创建成功");
    info!(
        "  全页面配置: full_page={}, quality={}, format={}",
        full_page_config.full_page.unwrap_or(false),
        full_page_config.quality.unwrap_or(80),
        full_page_config.format.as_deref().unwrap_or("png")
    );
    info!(
        "  区域配置: selector={}, quality={}",
        region_config.selector.as_deref().unwrap_or("N/A"),
        region_config.quality.unwrap_or(80)
    );
    info!("");

    // 3. 实际测试（使用简单的HTML内容模拟）
    info!("3️⃣  截图数据处理示例");
    info!("-----------------------------");

    // 模拟截图数据（实际使用时会被真实截图替换）
    let mock_screenshot = create_mock_screenshot();

    info!("✅ 模拟截图数据");
    info!("  大小: {} 字节", mock_screenshot.len());
    info!(
        "  前缀: {}...",
        &mock_screenshot[..40.min(mock_screenshot.len())]
    );
    info!("");

    // 解码Base64示例
    info!("📊 Base64解码示例:");
    match base64::decode(&mock_screenshot[..100.min(mock_screenshot.len())]) {
        Ok(decoded) => {
            info!("  成功解码 {} 字节", decoded.len());
            info!("  前几个字节: {:?}", &decoded[..8.min(decoded.len())]);
        }
        Err(e) => {
            info!("  解码失败: {}", e);
        }
    }

    info!("\n=====================================");
    info!("✨ 页面截图示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - 截图保存在响应对象的 screenshot 字段中");
    info!("   - 截图格式为Base64编码的图像数据");
    info!("   - 使用 base64::decode() 或 <img src=\"data:image/png;base64,{}\"> 显示");
    info!("   - 全页面截图可能消耗更多内存和时间");
}

fn create_mock_screenshot() -> String {
    // 模拟一个小的Base64图像数据（实际使用时应为真实截图）
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==".to_string()
}
