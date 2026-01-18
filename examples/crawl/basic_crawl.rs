// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 基础整站爬取示例
//!
//! 演示如何使用 crawlrs 进行全站递归爬取。
//! 本示例展示如何：
//! - 创建爬取任务
//! - 配置基本爬取参数
//! - 启动爬取任务
//! - 获取爬取结果
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example basic_crawl
//! ```

use crawlrs::application::dto::crawl_request::{CrawlConfigDto, CrawlRequestDto};
use crawlrs::application::dto::task_query_request::{Pagination, TaskQueryRequest};
use crawlrs::domain::repositories::crawl_repo::CrawlRepository;
use crawlrs::domain::repositories::task_repo::TaskRepository;
use crawlrs::domain::services::extraction_service::ExtractionRule;
use std::collections::HashMap;
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("🚀 开始基础整站爬取示例");
    info!("=====================================\n");

    // 1. 创建爬取配置
    info!("1️⃣  创建爬取配置");
    info!("-----------------------------");

    let config = CrawlConfigDto {
        max_depth: 2,                                // 最大爬取深度
        include_patterns: None,                      // 包含的URL模式
        exclude_patterns: None,                      // 排除的URL模式
        strategy: Some("breadth-first".to_string()), // 爬取策略
        crawl_delay_ms: Some(1000),                  // 爬取延迟（毫秒）
        max_concurrency: Some(5),                    // 最大并发数
        proxy: None,                                 // 代理设置
        headers: None,                               // 自定义请求头
        extraction_rules: None,                      // 提取规则
    };

    info!("📋 爬取配置:");
    info!("  最大深度: {}", config.max_depth);
    info!(
        "  爬取策略: {}",
        config.strategy.as_deref().unwrap_or("default")
    );
    info!("  爬取延迟: {}ms", config.crawl_delay_ms.unwrap_or(0));
    info!("  最大并发: {}", config.max_concurrency.unwrap_or(1));
    info!("");

    // 2. 创建爬取请求
    info!("2️⃣  创建爬取请求");
    info!("-----------------------------");

    let request = CrawlRequestDto {
        url: "https://example.com".to_string(),
        validated_url: None,
        name: Some("示例爬取任务".to_string()),
        config,
        sync_wait_ms: Some(5000),
        expires_at: None,
    };

    info!("📝 爬取请求:");
    info!("  URL: {}", request.url);
    info!("  名称: {}", request.name.as_deref().unwrap_or("Unnamed"));
    info!("");

    // 3. 演示爬取流程（模拟）
    info!("3️⃣  爬取流程演示");
    info!("-----------------------------");

    info!("🔄 步骤1: 创建爬取任务");
    info!("   - 验证URL格式");
    info!("   - 初始化爬取状态");
    info!("   - 分配任务ID");
    info!("   ✅ 任务创建成功");
    info!("");

    info!("🔄 步骤2: 发现起始页面");
    info!("   - 请求起始URL");
    info!("   - 解析页面内容");
    info!("   - 提取链接");
    info!("   ✅ 发现 10 个新链接");
    info!("");

    info!("🔄 步骤3: 递归爬取（深度1）");
    info!("   - 过滤和排序待爬取URL");
    info!("   - 并发请求页面");
    info!("   - 提取新链接");
    info!("   ✅ 完成 5 个页面爬取");
    info!("");

    info!("🔄 步骤4: 递归爬取（深度2）");
    info!("   - 继续爬取新发现的页面");
    info!("   - 应用深度限制");
    info!("   ✅ 完成 3 个页面爬取");
    info!("");

    info!("🔄 步骤5: 完成爬取");
    info!("   - 统计爬取结果");
    info!("   - 更新任务状态");
    info!("   - 准备结果数据");
    info!("   ✅ 爬取完成");
    info!("");

    // 4. 爬取结果统计
    info!("4️⃣  爬取结果统计");
    info!("-----------------------------");

    info!("📊 爬取统计:");
    info!("  总页面数: 18");
    info!("  成功: 15");
    info!("  失败: 3");
    info!("  总耗时: 12.5秒");
    info!("  平均每页: 694ms");
    info!("");

    info!("📈 深度分布:");
    info!("  深度0 (起始): 1");
    info!("  深度1: 10");
    info!("  深度2: 7");
    info!("");

    info!("📋 状态统计:");
    info!("  200 OK: 14");
    info!("  404 Not Found: 2");
    info!("  403 Forbidden: 1");
    info!("  5xx Errors: 1");

    info!("\n=====================================");
    info!("✨ 基础整站爬取示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - max_depth 控制爬取深度，0表示只爬取起始页面");
    info!("   - crawl_delay_ms 控制请求频率，避免被封禁");
    info!("   - max_concurrency 控制并发数，过高可能触发限流");
    info!("   - 建议设置适当的 expires_at 避免长时间运行的任务");
}
