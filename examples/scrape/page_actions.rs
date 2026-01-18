// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 页面交互示例
//!
//! 演示如何使用 crawlrs 执行页面交互操作，包括：
//! - 等待（Wait）
//! - 点击（Click）
//! - 滚动（Scroll）
//! - 输入（Input）
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example page_actions
//! ```
//!
//! # 注意事项
//!
//! 页面交互功能需要启用 `engine-playwright` 特性。

use crawlrs::engines::engine_client::{EngineClient, PageAction, ScrapeOptionsBuilder};
use std::time::Duration;
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("🚀 开始页面交互示例");
    info!("=====================================\n");

    let client = EngineClient::new();
    let url = "https://example.com";

    info!("🎮 目标页面: {}", url);
    info!("");

    // 1. 等待操作
    info!("1️⃣  等待操作（Wait）");
    info!("-----------------------------");
    info!("   场景: 等待页面加载完成或等待指定时间");
    info!("   API: PageAction::Wait {{ milliseconds: 3000 }}");
    info!("");

    // 2. 点击操作
    info!("2️⃣  点击操作（Click）");
    info!("-----------------------------");
    info!("   场景: 点击按钮、链接或其他可点击元素");
    info!("   API: PageAction::Click {{ selector: \".submit-btn\" }}");
    info!("");

    // 3. 滚动操作
    info!("3️⃣  滚动操作（Scroll）");
    info!("-----------------------------");
    info!("   场景: 滚动页面以加载更多内容");
    info!("   API: PageAction::Scroll {{ direction: ScrollDirection::Down }}");
    info!("   支持方向: Up, Down, Bottom, Top");
    info!("");

    // 4. 输入操作
    info!("4️⃣  输入操作（Input）");
    info!("-----------------------------");
    info!("   场景: 在输入框中输入文本");
    info!("   API: PageAction::Input {{ selector: \"#search\", text: \"hello\" }}");
    info!("");

    // 5. 完整交互示例
    info!("5️⃣  完整交互序列示例");
    info!("-----------------------------");

    // 构建交互动作序列
    let actions = vec![
        // 等待页面初始加载
        PageAction::Wait { milliseconds: 2000 },
        // 向下滚动以加载更多内容
        PageAction::Scroll {
            direction: crawlrs::engines::engine_client::ScrollDirection::Down,
        },
        // 等待加载
        PageAction::Wait { milliseconds: 1000 },
        // 再次滚动
        PageAction::Scroll {
            direction: crawlrs::engines::engine_client::ScrollDirection::Down,
        },
        // 等待加载完成
        PageAction::Wait { milliseconds: 1000 },
        // 滚动回顶部
        PageAction::Scroll {
            direction: crawlrs::engines::engine_client::ScrollDirection::Up,
        },
    ];

    info!("✅ 动作序列构建成功");
    info!("   总动作数: {}", actions.len());
    for (i, action) in actions.iter().enumerate() {
        match action {
            PageAction::Wait { milliseconds } => info!("   [{:2}] 等待 {}ms", i + 1, milliseconds),
            PageAction::Click { selector } => info!("   [{:2}] 点击选择器: {}", i + 1, selector),
            PageAction::Scroll { direction } => info!("   [{:2}] 滚动方向: {:?}", i + 1, direction),
            PageAction::Input { selector, text } => {
                info!("   [{:2}] 输入到 {}: \"{}\"", i + 1, selector, text)
            }
        }
    }
    info!("");

    // 实际执行（演示）
    info!("🔄 准备执行交互动作...");

    let options = ScrapeOptionsBuilder::default()
        .actions(actions)
        .timeout(Duration::from_secs(120))
        .build();

    let request = crawlrs::engines::engine_client::ScrapeRequest::new(url).with_options(options);

    match client.scrape(&request).await {
        Ok(response) => {
            info!("✅ 交互执行完成");
            info!("  状态码: {}", response.status_code);
            info!("  内容长度: {} 字节", response.content.len());
            info!("  截图数量: {}", response.screenshot.is_some() as u32);
        }
        Err(e) => {
            info!(
                "⚠️  交互执行失败（这是预期的，如果未配置Playwright）: {:?}",
                e
            );
            info!("   提示: 交互功能需要启用 engine-playwright 特性");
        }
    }

    info!("");
    info!("=====================================");
    info!("✨ 页面交互示例完成");
    info!("");
    info!("💡 最佳实践:");
    info!("   - 在执行交互前等待页面加载完成");
    info!("   - 使用具体的选择器而不是通用选择器");
    info!("   - 在滚动之间添加适当的等待时间");
    info!("   - 设置合理的超时时间");
}
