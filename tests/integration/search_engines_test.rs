// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crawlrs::domain::search::engine::SearchEngine;
use crawlrs::infrastructure::search::baidu::BaiduSearchEngine;
use crawlrs::infrastructure::search::bing::BingSearchEngine;
use crawlrs::infrastructure::search::google::GoogleSearchEngine;
use crawlrs::infrastructure::search::sogou::SogouSearchEngine;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{timeout, Duration};

async fn run_concurrent_search_tests(
    test_query: &str,
    max_results: u32,
    timeout_secs: u64,
    test_mode: TestMode,
) -> Vec<(String, bool, String)> {
    let timeout_duration = Duration::from_secs(timeout_secs);

    test_mode.apply();

    let engines: Vec<(&str, Arc<dyn SearchEngine>)> = vec![
        ("Google", Arc::new(GoogleSearchEngine::new())),
        ("Bing", Arc::new(BingSearchEngine::new())),
        ("Baidu", Arc::new(BaiduSearchEngine::new())),
        ("Sogou", Arc::new(SogouSearchEngine::new())),
    ];

    let semaphore = Arc::new(Semaphore::new(2));
    let mut handles = vec![];

    for (engine_name, engine) in engines {
        let engine_name = engine_name.to_string();
        let engine = Arc::clone(&engine);
        let semaphore = Arc::clone(&semaphore);
        let test_query = test_query.to_string();

        let handle = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            println!("🔍 开始测试 {} 搜索引擎...", engine_name);

            let search_future = engine.search(&test_query, max_results, None, None);
            let result = timeout(timeout_duration, search_future).await;

            match result {
                Ok(Ok(search_results)) => {
                    println!(
                        "✅ {} 搜索成功，返回 {} 条结果",
                        engine_name,
                        search_results.len()
                    );

                    if search_results.is_empty() {
                        println!("⚠️  {} 未返回任何搜索结果", engine_name);
                        return (engine_name.clone(), false, "无搜索结果".to_string());
                    }

                    let mut valid_results = 0;
                    let mut contains_gemini = 0;

                    for (idx, result) in search_results.iter().enumerate() {
                        if idx < 3 {
                            println!(
                                "  {} 结果 {}: {} - {}",
                                engine_name,
                                idx + 1,
                                result.title,
                                result.url
                            );
                        }

                        if !result.title.is_empty() && !result.url.is_empty() {
                            valid_results += 1;
                        }

                        let title_lower = result.title.to_lowercase();
                        let desc_lower = result
                            .description
                            .as_ref()
                            .map(|d| d.to_lowercase())
                            .unwrap_or_default();
                        if title_lower.contains("gemini") || desc_lower.contains("gemini") {
                            contains_gemini += 1;
                        }
                    }

                    println!(
                        "📊 {} 统计: 有效结果 {} 个，包含关键词 {} 个",
                        engine_name, valid_results, contains_gemini
                    );

                    if valid_results == 0 {
                        (engine_name.clone(), false, "无有效结果".to_string())
                    } else if contains_gemini == 0 {
                        (engine_name.clone(), false, "结果不包含关键词".to_string())
                    } else {
                        (
                            engine_name.clone(),
                            true,
                            format!("成功返回 {} 个相关结果", search_results.len()),
                        )
                    }
                }
                Ok(Err(search_error)) => {
                    println!("❌ {} 搜索失败: {}", engine_name, search_error);
                    (
                        engine_name.clone(),
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
                    (engine_name.clone(), false, "搜索超时".to_string())
                }
            }
        });

        handles.push(handle);
    }

    let mut results = vec![];
    for handle in handles {
        if let Ok(result) = handle.await {
            results.push(result);
        }
    }

    results
}

enum TestMode {
    Full,
    Simple,
    Real,
}

impl TestMode {
    fn apply(&self) {
        match self {
            TestMode::Full => {
                std::env::set_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS", "true");
                std::env::set_var("BING_TEST_RESULTS", "true");
                std::env::set_var("BAIDU_TEST_RESULTS", "true");
                std::env::set_var("SOGOU_TEST_RESULTS", "true");
            }
            TestMode::Simple => {
                std::env::set_var("USE_TEST_DATA", "1");
            }
            TestMode::Real => {
                std::env::remove_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS");
                std::env::remove_var("BING_TEST_RESULTS");
                std::env::remove_var("BAIDU_TEST_RESULTS");
                std::env::remove_var("SOGOU_TEST_RESULTS");
                std::env::remove_var("USE_TEST_DATA");
            }
        }
    }
}

fn generate_test_report(results: &[(String, bool, String)]) {
    println!("\n📋 搜索引擎测试报告");
    println!("{}", "=".repeat(50));

    let mut passed = 0;
    let mut failed = 0;

    for (engine_name, success, message) in results {
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
}

/// 测试所有搜索引擎（关键词: gemini）
///
/// 注意：此测试需要真实搜索引擎连接能力。
/// 如需运行此测试，请使用: cargo test --test integration_tests -- test_all_search_engines_with_gemini -- --include-ignored
#[ignore]
#[tokio::test]
async fn test_all_search_engines_with_gemini() {
    println!("🚀 开始测试四个搜索引擎，关键词: gemini");

    let results = run_concurrent_search_tests("gemini", 10, 30, TestMode::Full).await;
    generate_test_report(&results);

    let failed_count = results.iter().filter(|(_, success, _)| !success).count();
    if failed_count > 0 {
        panic!("❌ 搜索引擎测试失败: {} 个引擎测试未通过", failed_count);
    }

    println!("🎉 所有搜索引擎测试通过！");
}

#[tokio::test]
#[ignore] // Ignoring this test because it requires real search engine connectivity
async fn test_search_engines_simple_mode() {
    println!("🚀 开始测试搜索引擎（简化模式），关键词: gemini");

    let results = run_concurrent_search_tests("gemini", 10, 30, TestMode::Full).await;
    generate_test_report(&results);

    let failed_count = results.iter().filter(|(_, success, _)| !success).count();
    if failed_count > 0 {
        panic!("❌ 搜索引擎测试失败: {} 个引擎测试未通过", failed_count);
    }

    println!("🎉 搜索引擎简化模式测试通过！");
}

#[tokio::test]
#[ignore] // Ignoring this test because it requires real search engine connectivity
async fn test_real_search_engines_connectivity() {
    println!("🚀 开始真实搜索引擎连接性测试，关键词: rust programming language");

    let results =
        run_concurrent_search_tests("rust programming language", 5, 60, TestMode::Real).await;
    generate_test_report(&results);

    println!("✅ 真实搜索引擎连接性测试完成");
}

/// 测试搜索引擎性能
///
/// 注意：此测试需要真实搜索引擎连接能力。
/// 如需运行此测试，请使用: cargo test --test integration_tests -- test_search_engine_performance -- --include-ignored
#[ignore]
#[tokio::test]
async fn test_search_engine_performance() {
    let test_query = "gemini";
    let max_results = 5;

    println!("⚡ 开始搜索引擎性能测试...");

    let engines: Vec<(&str, Arc<dyn SearchEngine>)> = vec![
        ("Google", Arc::new(GoogleSearchEngine::new())),
        ("Bing", Arc::new(BingSearchEngine::new())),
        ("Baidu", Arc::new(BaiduSearchEngine::new())),
        ("Sogou", Arc::new(SogouSearchEngine::new())),
    ];

    let mut performance_results = vec![];

    for (engine_name, engine) in engines {
        println!("🔍 测试 {} 性能...", engine_name);

        let start_time = std::time::Instant::now();
        let result = engine.search(test_query, max_results, None, None).await;
        let duration = start_time.elapsed();

        match result {
            Ok(search_results) => {
                println!(
                    "✅ {} 性能测试完成，耗时: {:?}，返回 {} 条结果",
                    engine_name,
                    duration,
                    search_results.len()
                );
                performance_results.push((engine_name, duration, search_results.len(), true));
            }
            Err(error) => {
                println!(
                    "❌ {} 性能测试失败: {:?}，耗时: {:?}",
                    engine_name, error, duration
                );
                performance_results.push((engine_name, duration, 0, false));
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    println!("\n⚡ 搜索引擎性能报告");
    println!("{}", "=".repeat(60));

    for (engine_name, duration, result_count, success) in performance_results {
        let status = if success { "✅" } else { "❌" };
        println!(
            "{} {}: 耗时 {:?}，返回 {} 条结果",
            status, engine_name, duration, result_count
        );
    }

    println!("\n📊 性能分析完成");
}

#[tokio::test]
async fn test_search_engine_error_handling() {
    println!("🧪 测试搜索引擎错误处理...");

    let engines: Vec<(&str, Arc<dyn SearchEngine>)> = vec![
        ("Google", Arc::new(GoogleSearchEngine::new())),
        ("Bing", Arc::new(BingSearchEngine::new())),
        ("Baidu", Arc::new(BaiduSearchEngine::new())),
        ("Sogou", Arc::new(SogouSearchEngine::new())),
    ];

    for (engine_name, engine) in &engines {
        println!("🔍 测试 {} 空查询处理...", engine_name);

        match engine.search("", 10, None, None).await {
            Ok(results) => {
                println!("✅ {} 空查询返回 {} 条结果", engine_name, results.len());
            }
            Err(error) => {
                println!("✅ {} 空查询正确处理: {}", engine_name, error);
            }
        }
    }

    for (engine_name, engine) in &engines {
        println!("🔍 测试 {} 特殊字符查询...", engine_name);

        match engine.search("gemini!@#$%^&*()", 5, None, None).await {
            Ok(results) => {
                println!(
                    "✅ {} 特殊字符查询返回 {} 条结果",
                    engine_name,
                    results.len()
                );
            }
            Err(error) => {
                println!("✅ {} 特殊字符查询处理: {}", engine_name, error);
            }
        }
    }

    println!("✅ 错误处理测试完成");
}

#[tokio::test]
async fn test_search_results_comparison() {
    std::env::set_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS", "true");
    std::env::set_var("BING_TEST_RESULTS", "true");
    std::env::set_var("BAIDU_TEST_RESULTS", "true");
    std::env::set_var("SOGOU_TEST_RESULTS", "true");

    let test_query = "gemini";
    let max_results = 5;

    println!("🔍 比较不同搜索引擎的结果...");

    let engines: Vec<(&str, Arc<dyn SearchEngine>)> = vec![
        ("Google", Arc::new(GoogleSearchEngine::new())),
        ("Bing", Arc::new(BingSearchEngine::new())),
        ("Baidu", Arc::new(BaiduSearchEngine::new())),
        ("Sogou", Arc::new(SogouSearchEngine::new())),
    ];

    let mut all_results = std::collections::HashMap::new();

    for (engine_name, engine) in engines {
        println!("🔍 获取 {} 的搜索结果...", engine_name);

        match engine.search(test_query, max_results, None, None).await {
            Ok(results) => {
                all_results.insert(engine_name, results);
            }
            Err(error) => {
                println!("❌ {} 搜索失败: {}", engine_name, error);
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    println!("\n📊 搜索结果对比分析");
    println!("{}", "=".repeat(50));

    for (engine_name, results) in &all_results {
        println!("🔍 {}: {} 条结果", engine_name, results.len());

        let mut domains = std::collections::HashSet::new();
        for result in results {
            if let Ok(url) = url::Url::parse(&result.url) {
                if let Some(domain) = url.domain() {
                    domains.insert(domain.to_string());
                }
            }
        }

        println!("  涉及域名: {}", domains.len());
        for domain in domains.iter().take(3) {
            println!("  - {}", domain);
        }
    }

    if all_results.len() >= 2 {
        println!("\n🔗 查找共同出现的URL...");

        let engine_names: Vec<_> = all_results.keys().cloned().collect();
        let first_engine = engine_names[0];
        let first_results = &all_results[first_engine];

        for result in first_results {
            let mut found_in = vec![first_engine];

            for other_engine in engine_names.iter().skip(1) {
                let other_results = &all_results[other_engine];
                if other_results.iter().any(|r| r.url == result.url) {
                    found_in.push(other_engine);
                }
            }

            if found_in.len() >= 2 {
                println!(
                    "✅ 共同结果: {} (出现在: {})",
                    result.title,
                    found_in.join(", ")
                );
            }
        }
    }

    println!("✅ 结果对比分析完成");
}
