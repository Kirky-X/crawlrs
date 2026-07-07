// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 多格式输出示例
//!
//! 演示如何配置不同的输出格式（HTML、Markdown、JSON、Screenshot）。
//! crawlrs 支持多种输出格式，可以根据需求选择最合适的格式。
//!
//! # 输出格式
//!
//! - **HTML**: 原始HTML内容
//! - **Markdown**: 转换为Markdown格式
//! - **JSON**: 结构化JSON输出
//! - **Screenshot**: 页面截图（Base64编码）
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example multi_format_output
//! ```

use crawlrs::engines::engine_client::{EngineClient, ScrapeOptionsBuilder};
use std::time::Duration;
use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始多格式输出示例");
    info!("=====================================\n");

    let client = EngineClient::new();
    let url = "https://httpbin.org/html";

    // 1. HTML格式（默认）
    info!("📄 1. HTML格式输出");
    info!("-----------------------------");
    let request = crawlrs::engines::engine_client::ScrapeRequest::new(url);
    match client.scrape(&request).await {
        Ok(response) => {
            info!("✅ 成功获取HTML内容");
            info!("  长度: {} 字节", response.content.len());
            let preview = &response.content[..200.min(response.content.len())];
            info!("  预览: {}...", preview);
        }
        Err(e) => info!("❌ 获取失败: {:?}", e),
    }
    info!("");

    // 2. 模拟Markdown转换
    info!("📝 2. Markdown格式（通过文本提取）");
    info!("-----------------------------");
    let request = crawlrs::engines::engine_client::ScrapeRequest::new(url);
    match client.scrape(&request).await {
        Ok(response) => {
            // 简单演示：去除HTML标签模拟Markdown
            let content = &response.content;
            let markdown: String = content
                .lines()
                .map(|line| {
                    let clean =
                        line.trim_start_matches(|c: char| c.is_ascii_whitespace() && c != '\t');
                    if clean.starts_with("<h") && clean.ends_with(">") {
                        clean.replace(|c: char| c == '<' || c == '>', "")
                    } else if clean.starts_with("<p") && clean.ends_with(">") {
                        format!("\n{}\n", clean.replace(|c: char| c == '<' || c == '>', ""))
                    } else {
                        clean.to_string()
                    }
                })
                .filter(|s| !s.is_empty() && !s.starts_with("<") && !s.starts_with(">"))
                .collect::<Vec<_>>()
                .join("\n");

            info!("✅ 成功转换Markdown");
            info!("  长度: {} 字符", markdown.len());
            info!(
                "  预览:\n{}",
                markdown.lines().take(5).collect::<Vec<_>>().join("\n")
            );
        }
        Err(e) => info!("❌ 转换失败: {:?}", e),
    }
    info!("");

    // 3. JSON格式
    info!("📊 3. JSON格式输出");
    info!("-----------------------------");
    let request = crawlrs::engines::engine_client::ScrapeRequest::new(url);
    match client.scrape(&request).await {
        Ok(response) => {
            // 假设响应可以序列化为JSON
            info!("✅ 成功获取JSON结构");
            info!("  原始长度: {} 字节", response.content.len());

            // 尝试解析为JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response.content) {
                info!(
                    "  解析成功，包含键: {:?}",
                    json.as_object()
                        .map(|o| o.keys().take(5).collect::<Vec<_>>())
                );
            }
        }
        Err(e) => info!("❌ 获取失败: {:?}", e),
    }
    info!("");

    // 4. Screenshot格式（需要Playwright）
    info!("📸 4. Screenshot格式输出");
    info!("-----------------------------");
    info!("⚠️  截图功能需要Playwright引擎，演示基本流程");

    let options = ScrapeOptionsBuilder::default()
        .needs_screenshot(true)
        .timeout(Duration::from_secs(60))
        .build();

    let request = crawlrs::engines::engine_client::ScrapeRequest::new(url).with_options(options);

    match client.scrape(&request).await {
        Ok(response) => {
            if let Some(screenshot) = response.screenshot {
                info!("✅ 成功获取截图");
                info!("  截图大小: {} 字节", screenshot.len());
                info!(
                    "  Base64前缀: {}...",
                    &screenshot[..40.min(screenshot.len())]
                );
            } else {
                info!("⚠️  未返回截图（可能需要Playwright引擎）");
            }
        }
        Err(e) => {
            info!("⚠️  截图失败（这是预期的，如果未配置Playwright）: {:?}", e);
        }
    }

    info!("\n=====================================");
    info!("✨ 多格式输出示例完成");
    info!("");
    info!("💡 提示：");
    info!("   - HTML: 适合需要原始HTML内容的场景");
    info!("   - Markdown: 适合文本内容提取");
    info!("   - JSON: 适合程序化处理");
    info!("   - Screenshot: 适合需要视觉验证的场景");
}
