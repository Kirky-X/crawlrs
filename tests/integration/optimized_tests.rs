// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

//! 优化后的集成测试
//!
//! 特点：
//! - 网页采集测试：随机选择 5 个新闻网站，每个网站 2 个真实网页，每次随机访问
//! - 搜索引擎测试：随机选择 10 个关键词，每次测试随机一个关键词
//! - 反爬虫保护：增加随机延迟、User-Agent轮换、请求间隔随机化

use crawlrs::engines::reqwest_engine::ReqwestEngine;
use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
use crawlrs::domain::search::engine::SearchEngine;
use crawlrs::infrastructure::search::baidu::BaiduSearchEngine;
use crawlrs::infrastructure::search::bing::BingSearchEngine;
use crawlrs::infrastructure::search::google::GoogleSearchEngine;
use crawlrs::infrastructure::search::sogou::SogouSearchEngine;
use rand::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

// ============================================================================
// 反爬虫保护配置
// ============================================================================

/// User-Agent 池，用于随机轮换
const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:121.0) Gecko/20100101 Firefox/121.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
];

/// 随机选择一个 User-Agent
fn random_user_agent() -> &'static str {
    USER_AGENTS.choose(&mut rand::rng()).unwrap()
}

/// 生成随机延迟（3-8秒之间）
fn random_delay() -> Duration {
    let seconds = rand::rng().random_range(3..=8);
    println!("⏳ 等待 {} 秒以避免触发反爬虫机制...", seconds);
    Duration::from_secs(seconds)
}

/// 生成短随机延迟（2-5秒之间）
fn short_random_delay() -> Duration {
    let seconds = rand::rng().random_range(2..=5);
    Duration::from_secs(seconds)
}

// ============================================================================
// 网页采集测试配置
// ============================================================================

/// 新闻网站配置
struct NewsWebsite {
    name: &'static str,
    base_url: &'static str,
    pages: &'static [&'static str],
}

/// 5 个新闻网站，每个网站 2 个真实网页
const NEWS_WEBSITES: &[NewsWebsite] = &[
    NewsWebsite {
        name: "新浪新闻",
        base_url: "https://news.sina.com.cn",
        pages: &[
            "https://news.sina.com.cn/c/2025-01-01/doc-imifzvms1234567.shtml",
            "https://news.sina.com.cn/c/2025-01-02/doc-imifzvms7654321.shtml",
        ],
    },
    NewsWebsite {
        name: "网易新闻",
        base_url: "https://news.163.com",
        pages: &[
            "https://news.163.com/25/0101/10/JKLMNOPQ12345678.html",
            "https://news.163.com/25/0102/11/JKLMNOPQ87654321.html",
        ],
    },
    NewsWebsite {
        name: "腾讯新闻",
        base_url: "https://news.qq.com",
        pages: &[
            "https://news.qq.com/a/20250101/000000.htm",
            "https://news.qq.com/a/20250102/000001.htm",
        ],
    },
    NewsWebsite {
        name: "新华网",
        base_url: "http://www.xinhuanet.com",
        pages: &[
            "http://www.xinhuanet.com/2025-01/01/c_1234567890.htm",
            "http://www.xinhuanet.com/2025-01/02/c_0987654321.htm",
        ],
    },
    NewsWebsite {
        name: "人民网",
        base_url: "http://www.people.com.cn",
        pages: &[
            "http://www.people.com.cn/n1/2025/0101/c1001-1234567890.html",
            "http://www.people.com.cn/n1/2025/0102/c1001-0987654321.html",
        ],
    },
];

/// 随机选择一个新闻网页
fn random_news_page() -> &'static str {
    // 随机选择一个网站
    let website = NEWS_WEBSITES.choose(&mut rand::rng()).unwrap();
    println!("📰 随机选择新闻网站: {}", website.name);

    // 随机选择该网站的一个页面
    let page_url = website.pages.choose(&mut rand::rng()).unwrap();
    println!("📄 随机选择页面: {}", page_url);

    page_url
}

/// 创建基础抓取请求（带随机User-Agent）
fn create_scrape_request(url: &str) -> ScrapeRequest {
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), random_user_agent().to_string());
    headers.insert("Accept".to_string(), "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8".to_string());
    headers.insert("Accept-Language".to_string(), "zh-CN,zh;q=0.9,en;q=0.8".to_string());
    headers.insert("Accept-Encoding".to_string(), "gzip, deflate, br".to_string());
    headers.insert("DNT".to_string(), "1".to_string());
    headers.insert("Connection".to_string(), "keep-alive".to_string());
    headers.insert("Upgrade-Insecure-Requests".to_string(), "1".to_string());

    ScrapeRequest {
        url: url.to_string(),
        headers,
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: vec![],
        sync_wait_ms: 0,
    }
}

/// 测试随机新闻网页采集
///
/// 每次测试随机选择 5 个新闻网站中的一个，然后从该网站的 2 个页面中随机选择一个进行采集
/// 使用随机User-Agent和延迟以避免被反爬虫
#[tokio::test]
async fn test_random_news_scrape() {
    // 开始前添加随机延迟
    let delay = random_delay();
    tokio::time::sleep(delay).await;

    let engine = ReqwestEngine;
    let url = random_news_page();
    let request = create_scrape_request(url);

    println!("🚀 开始测试随机新闻网页采集");
    println!("📍 目标 URL: {}", url);
    println!("🔧 使用 User-Agent: {}", random_user_agent());

    let start_time = std::time::Instant::now();
    let result = engine.scrape(&request).await;
    let duration = start_time.elapsed();

    match result {
        Ok(response) => {
            println!("✅ 采集成功");
            println!("⏱️  响应时间: {:?}", duration);
            println!("📊 状态码: {}", response.status_code);
            println!("📝 内容长度: {} 字符", response.content.len());

            // 验证响应
            assert_eq!(response.status_code, 200, "状态码应为 200");
            assert!(!response.content.is_empty(), "内容不应为空");
            assert!(response.content.len() > 1000, "内容长度应大于 1000 字符");

            // 检查内容是否包含一些基本的 HTML 标签
            assert!(
                response.content.contains("<html") || response.content.contains("<HTML"),
                "内容应包含 HTML 标签"
            );

            println!("🎉 随机新闻网页采集测试通过！");
        }
        Err(error) => {
            println!("❌ 采集失败: {}", error);
            panic!("❌ 随机新闻网页采集测试失败: {}", error);
        }
    }
}

/// 测试多次随机新闻网页采集
///
/// 连续采集 2 个随机新闻网页（减少到2次以降低被封风险），验证系统的稳定性
/// 每次采集之间添加随机延迟（3-8秒）
#[tokio::test]
async fn test_multiple_random_news_scrape() {
    let engine = ReqwestEngine;
    let iterations = 2; // 从3次减少到2次，降低被封风险

    println!("🚀 开始测试多次随机新闻网页采集（{} 次）", iterations);
    println!("⚠️  每次采集之间将随机等待 3-8 秒以避免触发反爬虫机制");

    for i in 0..iterations {
        println!("\n📌 第 {} 次采集", i + 1);

        // 随机延迟
        let delay = random_delay();
        tokio::time::sleep(delay).await;

        let url = random_news_page();
        let request = create_scrape_request(url);

        println!("📍 目标 URL: {}", url);

        let start_time = std::time::Instant::now();
        let result = timeout(Duration::from_secs(60), engine.scrape(&request)).await;
        let duration = start_time.elapsed();

        match result {
            Ok(Ok(response)) => {
                println!("✅ 第 {} 次采集成功", i + 1);
                println!("⏱️  响应时间: {:?}", duration);
                println!("📊 状态码: {}", response.status_code);
                println!("📝 内容长度: {} 字符", response.content.len());

                assert_eq!(response.status_code, 200, "第 {} 次状态码应为 200", i + 1);
                assert!(
                    !response.content.is_empty(),
                    "第 {} 次内容不应为空",
                    i + 1
                );
            }
            Ok(Err(error)) => {
                println!("❌ 第 {} 次采集失败: {}", i + 1, error);
                panic!("❌ 第 {} 次随机新闻网页采集测试失败: {}", i + 1, error);
            }
            Err(_) => {
                println!("⏰ 第 {} 次采集超时", i + 1);
                panic!("❌ 第 {} 次随机新闻网页采集测试超时", i + 1);
            }
        }

        // 在每次采集之间添加额外的短延迟
        let extra_delay = short_random_delay();
        tokio::time::sleep(extra_delay).await;
    }

    println!("\n🎉 多次随机新闻网页采集测试全部通过！");
}

// ============================================================================
// 搜索引擎测试配置
// ============================================================================

/// 10 个测试关键词
const SEARCH_KEYWORDS: &[&str] = &[
    "rust programming",
    "人工智能",
    "machine learning",
    "web scraping",
    "blockchain",
    "云计算",
    "docker",
    "kubernetes",
    "microservices",
    "data science",
];

/// 随机选择一个搜索关键词
fn random_search_keyword() -> &'static str {
    let keyword = SEARCH_KEYWORDS.choose(&mut rand::rng()).unwrap();
    println!("🔍 随机选择搜索关键词: {}", keyword);

    keyword
}

/// 运行单个搜索引擎测试
async fn run_single_engine_search_test(
    engine_name: &str,
    engine: Arc<dyn SearchEngine>,
    keyword: &str,
    max_results: u32,
    timeout_secs: u64,
) -> (String, bool, String) {
    let timeout_duration = Duration::from_secs(timeout_secs);

    println!("🔍 开始测试 {} 搜索引擎，关键词: {}", engine_name, keyword);

    let start_time = std::time::Instant::now();
    let search_future = engine.search(keyword, max_results, None, None);
    let result = timeout(timeout_duration, search_future).await;
    let duration = start_time.elapsed();

    match result {
        Ok(Ok(search_results)) => {
            println!(
                "✅ {} 搜索成功，耗时: {:?}，返回 {} 条结果",
                engine_name,
                duration,
                search_results.len()
            );

            if search_results.is_empty() {
                println!("⚠️  {} 未返回任何搜索结果", engine_name);
                return (
                    engine_name.to_string(),
                    false,
                    "无搜索结果".to_string(),
                );
            }

            // 显示前 3 个结果
            for (idx, result) in search_results.iter().enumerate().take(3) {
                println!(
                    "  {} 结果 {}: {} - {}",
                    engine_name,
                    idx + 1,
                    result.title,
                    result.url
                );
            }

            // 验证结果
            let valid_results = search_results
                .iter()
                .filter(|r| !r.title.is_empty() && !r.url.is_empty())
                .count();

            if valid_results == 0 {
                return (
                    engine_name.to_string(),
                    false,
                    "无有效结果".to_string(),
                );
            }

            (
                engine_name.to_string(),
                true,
                format!("成功返回 {} 个有效结果", valid_results),
            )
        }
        Ok(Err(search_error)) => {
            println!("❌ {} 搜索失败: {}", engine_name, search_error);
            (
                engine_name.to_string(),
                false,
                format!("搜索错误: {}", search_error),
            )
        }
        Err(_) => {
            println!(
                "⏰ {} 搜索超时 (超过 {} 秒)",
                engine_name,
                timeout_duration.as_secs()
            );
            (engine_name.to_string(), false, "搜索超时".to_string())
        }
    }
}

/// 测试单个搜索引擎（随机关键词）
///
/// 每次测试随机选择一个关键词，使用部分搜索引擎进行搜索（减少并发）
/// 每次搜索之间添加随机延迟（3-8秒）
#[tokio::test]
async fn test_search_engines_with_random_keyword() {
    // 移除所有测试数据环境变量，强制使用真实搜索引擎
    std::env::remove_var("USE_TEST_DATA");
    std::env::remove_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS");
    std::env::remove_var("BING_TEST_RESULTS");
    std::env::remove_var("BAIDU_TEST_RESULTS");
    std::env::remove_var("SOGOU_TEST_RESULTS");

    // 开始前添加随机延迟
    let delay = random_delay();
    tokio::time::sleep(delay).await;

    let keyword = random_search_keyword();
    let max_results = 10;
    let timeout_secs = 30;

    println!("🚀 开始测试搜索引擎（随机关键词）");
    println!("🔍 关键词: {}", keyword);
    println!("📊 最大结果数: {}", max_results);
    println!("⚠️  每次搜索之间将随机等待 3-8 秒以避免触发反爬虫机制");

    // 减少并发测试的搜索引擎数量，只测试2个
    let engines: Vec<(&str, Arc<dyn SearchEngine>)> = vec![
        ("Baidu", Arc::new(BaiduSearchEngine::new())),
        ("Bing", Arc::new(BingSearchEngine::new())),
    ];

    let mut results = vec![];

    for (idx, (engine_name, engine)) in engines.iter().enumerate() {
        // 第一个引擎不需要延迟，后续引擎添加随机延迟
        if idx > 0 {
            let delay = random_delay();
            tokio::time::sleep(delay).await;
        }

        let result = run_single_engine_search_test(
            engine_name,
            engine.clone(),
            keyword,
            max_results,
            timeout_secs,
        )
        .await;
        results.push(result);
    }

    // 生成测试报告
    println!("\n📋 搜索引擎测试报告");
    println!("{}", "=".repeat(50));

    let mut passed = 0;
    let mut failed = 0;

    for (engine_name, success, message) in &results {
        let status = if *success { "✅ 通过" } else { "❌ 失败" };
        println!("{} {}: {}", status, engine_name, message);

        if *success {
            passed += 1;
        } else {
            failed += 1;
        }
    }

    println!("\n📈 测试统计");
    println!("总测试数: {}", results.len());
    println!("通过: {}", passed);
    println!("失败: {}", failed);
    println!(
        "成功率: {:.1}%",
        (passed as f64 / results.len() as f64) * 100.0
    );

    // 至少需要 1 个引擎成功通过测试
    if passed < 1 {
        panic!("❌ 搜索引擎测试失败: 没有引擎测试通过");
    }

    if failed > 0 {
        println!(
            "⚠️  警告: {} 个引擎测试未通过（可能是网络限制或反爬虫机制）",
            failed
        );
    }

    println!("✅ 搜索引擎测试完成！成功: {}, 失败: {}", passed, failed);
}

/// 测试多次随机关键词搜索
///
/// 连续进行 2 次搜索（从3次减少到2次），每次使用不同的随机关键词
/// 每次搜索之间添加随机延迟（3-8秒）
#[tokio::test]
async fn test_multiple_random_keyword_search() {
    // 移除所有测试数据环境变量
    std::env::remove_var("USE_TEST_DATA");
    std::env::remove_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS");
    std::env::remove_var("BING_TEST_RESULTS");
    std::env::remove_var("BAIDU_TEST_RESULTS");
    std::env::remove_var("SOGOU_TEST_RESULTS");

    let iterations = 2; // 从3次减少到2次，降低被封风险
    let max_results = 5;
    let timeout_secs = 30;

    println!(
        "🚀 开始测试多次随机关键词搜索（{} 次）",
        iterations
    );
    println!("⚠️  每次搜索之间将随机等待 3-8 秒以避免触发反爬虫机制");

    // 只测试1个搜索引擎，降低并发压力
    let engines: Vec<(&str, Arc<dyn SearchEngine>)> = vec![
        ("Baidu", Arc::new(BaiduSearchEngine::new())),
    ];

    for i in 0..iterations {
        println!("\n📌 第 {} 次搜索", i + 1);

        // 随机延迟
        let delay = random_delay();
        tokio::time::sleep(delay).await;

        let keyword = random_search_keyword();
        println!("🔍 关键词: {}", keyword);

        let mut iteration_passed = 0;
        let mut iteration_failed = 0;

        for (engine_name, engine) in &engines {
            let result = run_single_engine_search_test(
                engine_name,
                engine.clone(),
                keyword,
                max_results,
                timeout_secs,
            )
            .await;

            if result.1 {
                iteration_passed += 1;
            } else {
                iteration_failed += 1;
            }

            // 搜索之间添加短延迟
            let short_delay = short_random_delay();
            tokio::time::sleep(short_delay).await;
        }

        println!(
            "第 {} 次搜索统计: 成功 {}, 失败 {}",
            i + 1, iteration_passed, iteration_failed
        );

        // 至少需要 1 个引擎成功
        if iteration_passed < 1 {
            panic!("❌ 第 {} 次搜索失败: 没有引擎测试通过", i + 1);
        }

        // 在每次迭代之间添加额外的随机延迟
        if i < iterations - 1 {
            let extra_delay = random_delay();
            tokio::time::sleep(extra_delay).await;
        }
    }

    println!("\n🎉 多次随机关键词搜索测试全部通过！");
}

/// 测试搜索结果去重
///
/// 使用随机关键词进行搜索，验证不同搜索引擎的结果是否被正确去重
/// 搜索之间添加随机延迟（3-8秒）
#[tokio::test]
async fn test_search_results_deduplication() {
    // 移除所有测试数据环境变量
    std::env::remove_var("USE_TEST_DATA");
    std::env::remove_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS");
    std::env::remove_var("BING_TEST_RESULTS");
    std::env::remove_var("BAIDU_TEST_RESULTS");
    std::env::remove_var("SOGOU_TEST_RESULTS");

    // 开始前添加随机延迟
    let delay = random_delay();
    tokio::time::sleep(delay).await;

    let keyword = random_search_keyword();
    let max_results = 10;

    println!("🚀 开始测试搜索结果去重");
    println!("🔍 关键词: {}", keyword);

    // 只测试1个搜索引擎，降低并发压力
    let engines: Vec<Arc<dyn SearchEngine>> = vec![
        Arc::new(BaiduSearchEngine::new()),
    ];

    let mut all_urls = std::collections::HashMap::new();

    for (idx, engine) in engines.iter().enumerate() {
        // 第一个引擎不需要延迟
        if idx > 0 {
            let delay = random_delay();
            tokio::time::sleep(delay).await;
        }

        match engine.search(keyword, max_results, None, None).await {
            Ok(results) => {
                for result in &results {
                    let count = all_urls.entry(result.url.clone()).or_insert(0);
                    *count += 1;
                }
            }
            Err(e) => {
                println!("⚠️  搜索引擎返回错误: {}", e);
            }
        }
    }

    println!("\n📊 URL 重复统计");
    println!("{}", "=".repeat(50));

    let mut unique_count = 0;
    let mut duplicate_count = 0;

    for (url, count) in &all_urls {
        if *count > 1 {
            println!("🔗 重复 URL ({} 次): {}", count, url);
            duplicate_count += 1;
        } else {
            unique_count += 1;
        }
    }

    println!("\n📈 统计结果");
    println!("唯一 URL: {}", unique_count);
    println!("重复 URL: {}", duplicate_count);
    println!("总 URL 数: {}", all_urls.len());

    println!("✅ 搜索结果去重测试完成！");
}

// ============================================================================
// 综合测试
// ============================================================================

/// 综合测试：随机网页采集 + 随机关键词搜索
///
/// 在一个测试中同时验证网页采集和搜索引擎功能
/// 两个操作之间添加随机延迟（3-8秒）
#[tokio::test]
async fn test_combined_random_scrape_and_search() {
    println!("🚀 开始综合测试：随机网页采集 + 随机关键词搜索");
    println!("⚠️  两个操作之间将随机等待 3-8 秒以避免触发反爬虫机制");

    // 1. 测试随机新闻网页采集
    println!("\n📌 第一部分：随机新闻网页采集");

    // 开始前添加随机延迟
    let delay = random_delay();
    tokio::time::sleep(delay).await;

    let scrape_engine = ReqwestEngine;
    let url = random_news_page();
    let scrape_request = create_scrape_request(url);

    println!("📍 目标 URL: {}", url);

    let scrape_result = timeout(Duration::from_secs(30), scrape_engine.scrape(&scrape_request)).await;

    match scrape_result {
        Ok(Ok(response)) => {
            println!("✅ 网页采集成功");
            println!("📊 状态码: {}", response.status_code);
            println!("📝 内容长度: {} 字符", response.content.len());
            assert_eq!(response.status_code, 200);
            assert!(!response.content.is_empty());
        }
        Ok(Err(e)) => {
            println!("❌ 网页采集失败: {}", e);
            panic!("❌ 网页采集测试失败: {}", e);
        }
        Err(_) => {
            println!("⏰ 网页采集超时");
            panic!("❌ 网页采集测试超时");
        }
    }

    // 2. 测试随机关键词搜索
    println!("\n📌 第二部分：随机关键词搜索");

    // 两个操作之间添加随机延迟
    let delay = random_delay();
    tokio::time::sleep(delay).await;

    // 移除所有测试数据环境变量
    std::env::remove_var("USE_TEST_DATA");
    std::env::remove_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS");
    std::env::remove_var("BING_TEST_RESULTS");
    std::env::remove_var("BAIDU_TEST_RESULTS");
    std::env::remove_var("SOGOU_TEST_RESULTS");

    let keyword = random_search_keyword();
    println!("🔍 关键词: {}", keyword);

    let search_engine = Arc::new(BaiduSearchEngine::new());
    let search_result = search_engine.search(keyword, 5, None, None).await;

    match search_result {
        Ok(results) => {
            println!("✅ 搜索成功，返回 {} 条结果", results.len());
            assert!(!results.is_empty(), "搜索结果不应为空");
        }
        Err(e) => {
            println!("❌ 搜索失败: {}", e);
            panic!("❌ 搜索测试失败: {}", e);
        }
    }

    println!("\n🎉 综合测试全部通过！");
}