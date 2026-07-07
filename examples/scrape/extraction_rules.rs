// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 提取规则示例
//!
//! 演示如何使用 crawlrs 配置和使用提取规则，包括：
//! - CSS选择器规则
//! - 属性提取规则
//! - 文本提取规则
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example extraction_rules
//!

use crawlrs::engines::engine_client::ScrapeRequest;
use std::time::Duration;
use log::info;

/// 提取规则配置
#[derive(Debug)]
struct ExtractionRule {
    name: String,
    selector: String,
    attribute: Option<String>,
    extract_text: bool,
}

impl ExtractionRule {
    fn new(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            selector: selector.into(),
            attribute: None,
            extract_text: true,
        }
    }

    fn with_attribute(mut self, attr: impl Into<String>) -> Self {
        self.attribute = Some(attr.into());
        self.extract_text = false;
        self
    }

    fn extract_text(mut self) -> Self {
        self.extract_text = true;
        self.attribute = None;
        self
    }
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始提取规则示例");
    info!("=====================================\n");

    let client = EngineClient::new();
    let url = "https://example.com";

    info!("🎯 目标: {}", url);
    info!("");

    // 1. 定义提取规则
    info!("1️⃣  定义提取规则");
    info!("-----------------------------");

    let rules = vec![
        ExtractionRule::new("页面标题", "h1"),
        ExtractionRule::new("段落文本", "p").extract_text(),
        ExtractionRule::new("链接地址", "a").with_attribute("href"),
        ExtractionRule::new("图片地址", "img").with_attribute("src"),
        ExtractionRule::new("图片alt", "img").with_attribute("alt"),
        ExtractionRule::new("所有链接文本", "a").extract_text(),
    ];

    info!("📋 提取规则列表:");
    for (i, rule) in rules.iter().enumerate() {
        info!(
            "  [{:2}] {}: selector=\"{}\"",
            i + 1,
            rule.name,
            rule.selector
        );
        if let Some(attr) = &rule.attribute {
            info!("      └── 属性: {}", attr);
        }
        if rule.extract_text {
            info!("      └── 提取文本");
        }
    }
    info!("");

    // 2. 执行爬取
    info!("2️⃣  执行爬取");
    info!("-----------------------------");

    let request = ScrapeRequest::new(url).timeout(Duration::from_secs(30));

    match client.scrape(&request).await {
        Ok(response) => {
            info!("✅ 爬取成功");
            info!("  状态码: {}", response.status_code);
            info!("  内容长度: {} 字节", response.content.len());
            info!("");

            // 3. 应用提取规则
            info!("3️⃣  应用提取规则");
            info!("-----------------------------");

            let content = &response.content;
            apply_extraction_rules(&content, &rules);
        }
        Err(e) => {
            info!("⚠️  爬取失败: {:?}", e);
            info!("   使用示例HTML演示提取逻辑");

            // 示例HTML
            let sample_html = r#"
            <!DOCTYPE html>
            <html>
            <head><title>示例页面</title></head>
            <body>
                <h1>欢迎访问示例页面</h1>
                <p>这是一个示例段落，包含<strong>重要</strong>信息。</p>
                <a href="https://example.com/link1">链接1</a>
                <a href="https://example.com/link2">链接2</a>
                <img src="/images/logo.png" alt="网站Logo">
                <p>另一个段落文本。</p>
            </body>
            </html>
            "#;

            info!("📄 使用示例HTML演示");
            apply_extraction_rules(sample_html, &rules);
        }
    }

    info!("\n=====================================");
    info!("✨ 提取规则示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - CSS选择器支持标准的CSS3选择器语法");
    info!("   - 使用属性提取获取href、src等属性值");
    info!("   - 文本提取会自动清理HTML标签");
    info!("   - 可以组合使用多个提取规则");
}

fn apply_extraction_rules(html: &str, rules: &[ExtractionRule]) {
    // 使用简单的正则表达式模拟CSS选择器匹配
    // 实际使用时，应使用专门的CSS选择器库如 `scraper`

    info!("📊 提取结果:");
    info!("-----------------------------");

    for rule in rules {
        info!("  【{}】", rule.name);
        info!("  选择器: {}", rule.selector);

        // 简单模拟匹配逻辑
        match rule.selector.as_str() {
            "h1" => {
                if let Some(captures) = regex::Regex::new(r"<h1[^>]*>([^<]*)</h1>")
                    .unwrap()
                    .captures(html)
                {
                    if let Some(m) = captures.get(1) {
                        info!("  结果: {}", m.as_str().trim());
                    }
                }
            }
            "p" => {
                let count = regex::Regex::new(r"<p[^>]*>([^<]*)</p>")
                    .unwrap()
                    .captures_iter(html)
                    .count();
                info!("  匹配数量: {} 个", count);
                if let Some(captures) = regex::Regex::new(r"<p[^>]*>([^<]*)</p>")
                    .unwrap()
                    .captures(html)
                {
                    if let Some(m) = captures.get(1) {
                        info!("  第一个: {}", m.as_str().trim());
                    }
                }
            }
            "a" => {
                if let Some(attr) = &rule.attribute {
                    if attr == "href" {
                        let hrefs: Vec<_> = regex::Regex::new(r#"<a[^>]+href="([^"]*)""#)
                            .unwrap()
                            .captures_iter(html)
                            .filter_map(|c| c.get(1))
                            .map(|m| m.as_str())
                            .collect();
                        info!(
                            "  {} 个链接: {:?}",
                            hrefs.len(),
                            hrefs.iter().take(3).collect::<Vec<_>>()
                        );
                    }
                } else if rule.extract_text {
                    let links: Vec<_> = regex::Regex::new(r"<a[^>]*>([^<]*)</a>")
                        .unwrap()
                        .captures_iter(html)
                        .filter_map(|c| c.get(1))
                        .map(|m| m.as_str().trim())
                        .collect();
                    info!("  {} 个链接文本: {:?}", links.len(), links);
                }
            }
            "img" => {
                if let Some(attr) = &rule.attribute {
                    if attr == "src" {
                        let srcs: Vec<_> = regex::Regex::new(r#"<img[^>]+src="([^"]*)""#)
                            .unwrap()
                            .captures_iter(html)
                            .filter_map(|c| c.get(1))
                            .map(|m| m.as_str())
                            .collect();
                        info!("  {} 个图片: {:?}", srcs.len(), srcs);
                    } else if attr == "alt" {
                        let alts: Vec<_> = regex::Regex::new(r#"<img[^>]+alt="([^"]*)""#)
                            .unwrap()
                            .captures_iter(html)
                            .filter_map(|c| c.get(1))
                            .map(|m| m.as_str())
                            .collect();
                        info!("  {} 个alt属性: {:?}", alts.len(), alts);
                    }
                }
            }
            _ => info!("  (未匹配)"),
        }
        info!("");
    }
}
