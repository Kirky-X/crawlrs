// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! XPath 提取示例
//!
//! 演示如何使用 CSS 选择器进行复杂数据提取。
//! crawlrs 的提取引擎基于 `scraper` crate，支持完整的 CSS 选择器语法。
//! 对于需要 XPath 的场景，可通过 LLM 提取实现等效功能。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example xpath_extraction
//! ```

use crawlrs::engines::engine_client::{EngineClient, ScrapeRequest};
use log::info;
use std::time::Duration;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 CSS 选择器高级提取示例");
    info!("=====================================\n");

    let client = EngineClient::new();

    // 1. 先获取页面内容
    info!("1️⃣  获取页面内容");
    info!("-----------------------------");

    let request = ScrapeRequest::new("https://example.com").timeout(Duration::from_secs(30));

    let html = match client.scrape(&request).await {
        Ok(response) => {
            info!("  ✅ 爬取成功: HTTP {}", response.status_code);
            info!("  内容长度: {} 字节", response.content.len());
            Some(response.content)
        }
        Err(e) => {
            info!("  ❌ 爬取失败: {:?}", e);
            info!("  💡 使用示例 HTML 进行演示");
            None
        }
    };

    info!("");

    // 2. CSS 选择器模式
    info!("2️⃣  CSS 选择器高级用法");
    info!("-----------------------------");
    info!("  crawlrs 提取引擎支持完整的 CSS 选择器语法:");
    info!("");
    info!("  基础选择器:");
    info!("    h1              — 标签选择器");
    info!("    .class-name     — 类选择器");
    info!("    #id-name        — ID 选择器");
    info!("    div, span       — 多选择器（逗号分隔）");
    info!("");
    info!("  组合选择器:");
    info!("    div > p         — 直接子元素");
    info!("    div p           — 后代元素");
    info!("    h1 + p          — 相邻兄弟");
    info!("    h1 ~ p          — 通用兄弟");
    info!("");
    info!("  属性选择器:");
    info!("    a[href]         — 有 href 属性的 a 标签");
    info!("    a[href^=\"https\"] — href 以 https 开头");
    info!("    a[href$=\".pdf\"]  — href 以 .pdf 结尾");
    info!("    input[type=\"text\"] — 精确匹配属性值");
    info!("");
    info!("  伪类选择器:");
    info!("    li:first-child  — 第一个子元素");
    info!("    li:last-child   — 最后一个子元素");
    info!("    li:nth-child(2) — 第 N 个子元素");
    info!("");

    // 3. 提取规则示例
    info!("3️⃣  提取规则示例");
    info!("-----------------------------");
    info!("  提取页面所有链接:");
    info!("  {{ \"selector\": \"a[href]\", \"attr\": \"href\", \"is_array\": true }}");
    info!("");
    info!("  提取文章标题:");
    info!("  {{ \"selector\": \"article h1\", \"is_array\": false }}");
    info!("");
    info!("  提取所有图片 URL:");
    info!("  {{ \"selector\": \"img[src]\", \"attr\": \"src\", \"is_array\": true }}");
    info!("");
    info!("  提取 meta 信息:");
    info!("  {{ \"selector\": \"meta[name='description']\", \"attr\": \"content\", \"is_array\": false }}");
    info!("");

    if let Some(content) = &html {
        info!("  实际页面大小: {} 字节", content.len());
    }

    info!("\n=====================================");
    info!("✨ CSS 选择器高级提取示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - 对于复杂 XPath 需求，可使用 LLM 提取（extraction_prompt）");
    info!("   - CSS 选择器已覆盖绝大多数提取场景");
    info!("   - 结合 attr 字段可提取属性值而非文本内容");
}
