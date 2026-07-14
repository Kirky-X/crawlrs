// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 深度控制爬取示例
//!
//! 演示如何配置爬取深度限制，包括：
//! - 浅层爬取（只爬取首页）
//! - 中层爬取（1-2层链接）
//! - 深层爬取（完整站点）
//!
//! # 深度概念
//!
//! - **深度0**: 起始页面本身
//! - **深度1**: 从起始页面直接链接到的页面
//! - **深度2**: 深度1页面链接到的页面
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example depth_control
//!

use crawlrs::application::dto::crawl_request::CrawlConfigDto;
use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始深度控制爬取示例");
    info!("=====================================\n");

    let _base_url = "https://example.com";

    // 1. 深度0：只爬取起始页面
    info!("1️⃣  深度0：只爬取起始页面");
    info!("-----------------------------");

    let _config0 = CrawlConfigDto {
        max_depth: 0,
        include_patterns: None,
        exclude_patterns: None,
        strategy: Some("breadth-first".to_string()),
        crawl_delay_ms: Some(1000),
        max_concurrency: Some(1),
        proxy: None,
        headers: None,
        extraction_rules: None,
    };

    info!("📊 预期结果:");
    info!("  - 爬取页面数: 1");
    info!("  - 耗时: ~500ms");
    info!("  - 适用场景: 快速获取单页内容");
    info!("");

    // 2. 深度1：一级链接
    info!("2️⃣  深度1：一级链接");
    info!("-----------------------------");

    let _config1 = CrawlConfigDto {
        max_depth: 1,
        include_patterns: None,
        exclude_patterns: None,
        strategy: Some("breadth-first".to_string()),
        crawl_delay_ms: Some(1000),
        max_concurrency: Some(3),
        proxy: None,
        headers: None,
        extraction_rules: None,
    };

    info!("📊 预期结果:");
    info!("  - 爬取页面数: 1 + N (一级链接数)");
    info!("  - 耗时: ~2-5秒");
    info!("  - 适用场景: 快速获取站点主要页面");
    info!("");

    // 3. 深度2：二级链接
    info!("3️⃣  深度2：二级链接");
    info!("-----------------------------");

    let _config2 = CrawlConfigDto {
        max_depth: 2,
        include_patterns: None,
        exclude_patterns: None,
        strategy: Some("breadth-first".to_string()),
        crawl_delay_ms: Some(1000),
        max_concurrency: Some(5),
        proxy: None,
        headers: None,
        extraction_rules: None,
    };

    info!("📊 预期结果:");
    info!("  - 爬取页面数: 1 + N1 + N1×N2");
    info!("  - 耗时: ~10-30秒");
    info!("  - 适用场景: 中等规模站点爬取");
    info!("");

    // 4. 深度3+：深层爬取
    info!("4️⃣  深度3：深层爬取");
    info!("-----------------------------");

    let _config3 = CrawlConfigDto {
        max_depth: 3,
        include_patterns: None,
        exclude_patterns: None,
        strategy: Some("breadth-first".to_string()),
        crawl_delay_ms: Some(1500),
        max_concurrency: Some(5),
        proxy: None,
        headers: None,
        extraction_rules: None,
    };

    info!("📊 预期结果:");
    info!("  - 爬取页面数: 可能达到数百");
    info!("  - 耗时: ~1-5分钟");
    info!("  - 适用场景: 完整站点爬取（需配合URL过滤）");
    info!("   ⚠️  警告: 深层爬取可能产生大量请求");
    info!("");

    // 5. 实际配置示例
    info!("5️⃣  实际配置示例");
    info!("-----------------------------");

    // 博客站点爬取
    let blog_config = CrawlConfigDto {
        max_depth: 2,
        include_patterns: Some(vec!["/post/".to_string(), "/page/".to_string()]),
        exclude_patterns: Some(vec!["/tag/".to_string(), "/category/".to_string()]),
        strategy: Some("breadth-first".to_string()),
        crawl_delay_ms: Some(2000),
        max_concurrency: Some(3),
        proxy: None,
        headers: None,
        extraction_rules: None,
    };

    info!("📝 博客站点配置:");
    info!("  max_depth: {}", blog_config.max_depth);
    info!("  include_patterns: {:?}", blog_config.include_patterns);
    info!("  exclude_patterns: {:?}", blog_config.exclude_patterns);
    info!(
        "  crawl_delay_ms: {}",
        blog_config.crawl_delay_ms.unwrap_or(0)
    );
    info!("");

    // 电商站点爬取
    let ecommerce_config = CrawlConfigDto {
        max_depth: 3,
        include_patterns: Some(vec!["/product/".to_string(), "/category/".to_string()]),
        exclude_patterns: Some(vec![
            "/cart/".to_string(),
            "/checkout/".to_string(),
            "/account/".to_string(),
        ]),
        strategy: Some("breadth-first".to_string()),
        crawl_delay_ms: Some(3000),
        max_concurrency: Some(2),
        proxy: None,
        headers: None,
        extraction_rules: None,
    };

    info!("📝 电商站点配置:");
    info!("  max_depth: {}", ecommerce_config.max_depth);
    info!(
        "  include_patterns: {:?}",
        ecommerce_config.include_patterns
    );
    info!(
        "  exclude_patterns: {:?}",
        ecommerce_config.exclude_patterns
    );
    info!(
        "  crawl_delay_ms: {}",
        ecommerce_config.crawl_delay_ms.unwrap_or(0)
    );
    info!(
        "  max_concurrency: {}",
        ecommerce_config.max_concurrency.unwrap_or(1)
    );
    info!("   💡 电商站点建议更低并发和更长延迟");

    info!("\n=====================================");
    info!("✨ 深度控制爬取示例完成");
    info!("");
    info!("💡 最佳实践:");
    info!("   - 首次爬取使用较小深度进行测试");
    info!("   - 深层爬取必须配合URL过滤");
    info!("   - 设置合适的 crawl_delay_ms 避免被封禁");
    info!("   - 使用 max_concurrency 控制资源消耗");
}
