// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! URL过滤模式示例
//!
//! 演示如何配置URL包含和排除模式，控制爬取范围。
//!
//! # 过滤类型
//!
//! - **include_patterns**: 只爬取匹配这些模式的URL
//! - **exclude_patterns**: 排除匹配这些模式的URL
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example filter_patterns
//!

use crawlrs::application::dto::crawl_request::CrawlConfigDto;
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("🚀 开始URL过滤模式示例");
    info!("=====================================\n");

    // 1. 基本概念
    info!("1️⃣  URL过滤基本概念");
    info!("-----------------------------");
    info!("");
    info!("📖 include_patterns (包含模式):");
    info!("   - 只爬取匹配这些模式的URL");
    info!("   - 默认为None，表示爬取所有URL");
    info!("   - 使用glob语法：*, ?, **");
    info!("");
    info!("📖 exclude_patterns (排除模式):");
    info!("   - 排除匹配这些模式的URL");
    info!("   - 默认为None，表示不排除任何URL");
    info!("   - 排除优先级高于包含");
    info!("");

    // 2. 模式语法
    info!("2️⃣  模式语法示例");
    info!("-----------------------------");
    info!("");
    info!("📝 通配符匹配:");
    info!("   /blog/*       匹配 /blog/post-1, /blog/post-2");
    info!("   *.html        匹配任何 .html 结尾的URL");
    info!("   **/admin      匹配任何路径下的 admin");
    info!("");
    info!("📝 前缀匹配:");
    info!("   /api/         匹配所有 /api 开头的URL");
    info!("   https://example.com/products/  匹配特定域名下的路径");
    info!("");
    info!("📝 正则表达式（部分支持）:");
    info!("   /post/[0-9]+  匹配 /post/123, /post/456");
    info!("   .*\\.jpg       匹配任何 .jpg 结尾的URL");
    info!("");

    // 3. 博客站点过滤示例
    info!("3️⃣  博客站点过滤示例");
    info!("-----------------------------");

    let blog_config = CrawlConfigDto {
        max_depth: 2,
        include_patterns: Some(vec![
            "https://blog.example.com/post/*".to_string(),
            "https://blog.example.com/page/*".to_string(),
        ]),
        exclude_patterns: Some(vec![
            "https://blog.example.com/tag/*".to_string(),
            "https://blog.example.com/category/*".to_string(),
            "https://blog.example.com/*/feed".to_string(),
            "*.xml".to_string(),
        ]),
        strategy: Some("breadth-first".to_string()),
        crawl_delay_ms: Some(1000),
        max_concurrency: Some(5),
        proxy: None,
        headers: None,
        extraction_rules: None,
    };

    info!("📝 博客配置:");
    info!("  ✅ 包含:");
    for p in blog_config.include_patterns.as_ref().unwrap() {
        info!("     - {}", p);
    }
    info!("  ❌ 排除:");
    for p in blog_config.exclude_patterns.as_ref().unwrap() {
        info!("     - {}", p);
    }
    info!("");

    // 4. 电商站点过滤示例
    info!("4️⃣  电商站点过滤示例");
    info!("-----------------------------");

    let ecommerce_config = CrawlConfigDto {
        max_depth: 3,
        include_patterns: Some(vec![
            "https://shop.example.com/product/*".to_string(),
            "https://shop.example.com/category/*".to_string(),
        ]),
        exclude_patterns: Some(vec![
            "https://shop.example.com/cart".to_string(),
            "https://shop.example.com/checkout".to_string(),
            "https://shop.example.com/account/*".to_string(),
            "https://shop.example.com/*/review".to_string(),
            "*/add-to-cart".to_string(),
        ]),
        strategy: Some("breadth-first".to_string()),
        crawl_delay_ms: Some(2000),
        max_concurrency: Some(3),
        proxy: None,
        headers: None,
        extraction_rules: None,
    };

    info!("📝 电商配置:");
    info!("  ✅ 包含:");
    for p in ecommerce_config.include_patterns.as_ref().unwrap() {
        info!("     - {}", p);
    }
    info!("  ❌ 排除:");
    for p in ecommerce_config.exclude_patterns.as_ref().unwrap() {
        info!("     - {}", p);
    }
    info!("");

    // 5. 常见场景
    info!("5️⃣  常见过滤场景");
    info!("-----------------------------");
    info!("");
    info!("🏷️  场景1: 只爬取文章内容");
    info!("   include: [\"/article/*\", \"/post/*\"]");
    info!("   exclude: [\"/comment/*\", \"/reply/*\"]");
    info!("");
    info!("🏷️  场景2: 排除管理后台");
    info!("   include: []  // 默认全部");
    info!("   exclude: [\"/admin/*\", \"/dashboard/*\", \"/wp-admin/*\"]");
    info!("");
    info!("🏷️  场景3: 只爬取特定语言");
    info!("   include: [\"/en/*\", \"/zh/*\"]");
    info!("   exclude: []");
    info!("");
    info!("🏷️  场景4: 排除特定文件类型");
    info!("   include: []");
    info!("   exclude: [\".pdf\", \".zip\", \".mp3\", \"*.xml\"]");
    info!("");

    // 6. 过滤效果演示
    info!("6️⃣  过滤效果演示");
    info!("-----------------------------");

    let test_urls = vec![
        "https://example.com/",
        "https://example.com/about",
        "https://example.com/blog/post-1",
        "https://example.com/blog/post-2",
        "https://example.com/tag/rust",
        "https://example.com/category/tutorials",
        "https://example.com/blog/post-1/feed",
        "https://example.com/api/users",
        "https://example.com/sitemap.xml",
    ];

    let include_patterns = Some(vec!["/blog/*".to_string(), "/about".to_string()]);
    let exclude_patterns = Some(vec!["/tag/*".to_string(), "*.xml".to_string()]);

    info!("📝 测试URL列表:");
    for url in &test_urls {
        let included = should_include(url, &include_patterns, &exclude_patterns);
        let status = if included { "✅" } else { "❌" };
        info!("  {} {}", status, url);
    }

    info!("\n=====================================");
    info!("✨ URL过滤模式示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - 排除模式优先级高于包含模式");
    info!("   - 使用具体的URL前缀比通配符更高效");
    info!("   - 建议先测试过滤效果再进行大规模爬取");
    info!("   - 考虑添加robots.txt中的禁止爬取URL到排除列表");
}

fn should_include(
    url: &str,
    includes: &Option<Vec<String>>,
    excludes: &Option<Vec<String>>,
) -> bool {
    // 检查排除
    if let Some(excl_patterns) = excludes {
        for pattern in excl_patterns {
            if matches_pattern(url, pattern) {
                return false;
            }
        }
    }

    // 检查包含
    if let Some(inc_patterns) = includes {
        if inc_patterns.is_empty() {
            return true;
        }
        for pattern in inc_patterns {
            if matches_pattern(url, pattern) {
                return true;
            }
        }
        return false;
    }

    true
}

fn matches_pattern(url: &str, pattern: &str) -> bool {
    // 简单实现：检查URL是否包含模式字符串
    // 实际实现应使用更复杂的模式匹配
    url.contains(pattern.trim_start_matches("*"))
        || pattern.ends_with("*") && url.starts_with(&pattern[..pattern.len() - 1])
        || pattern.starts_with("*") && url.ends_with(&pattern[1..])
}
