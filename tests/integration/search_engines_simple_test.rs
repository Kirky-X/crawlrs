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
use tokio::time::{timeout, Duration};

/// 简化版搜索引擎测试套件
/// 测试所有可用的搜索引擎（包括使用 FlareSolverr 的 Google）
#[tokio::test]
async fn test_available_search_engines_with_gemini() {
    // Enable test mode for all search engines using the common USE_TEST_DATA environment variable
    // This ensures consistent test data loading across Bing, Baidu, Sogou, and Google
    std::env::set_var("USE_TEST_DATA", "1");

    let test_query = "gemini";
    let max_results = 10;
    let timeout_duration = Duration::from_secs(30);

    println!("🚀 开始测试可用搜索引擎，关键词: {}", test_query);

    // 创建可用的搜索引擎实例（包括Google）
    let engines: Vec<(&str, Arc<dyn SearchEngine>)> = vec![
        ("Bing", Arc::new(BingSearchEngine::new())),
        ("Baidu", Arc::new(BaiduSearchEngine::new())),
        ("Sogou", Arc::new(SogouSearchEngine::new())),
        ("Google", Arc::new(GoogleSearchEngine::new())),
    ];

    // 使用信号量限制并发数，避免触发反爬虫机制
    let semaphore = Arc::new(tokio::sync::Semaphore::new(2));
    let mut handles = vec![];

    for (engine_name, engine) in engines {
        let engine_name = engine_name.to_string();
        let engine = Arc::clone(&engine);
        let semaphore = Arc::clone(&semaphore);
        let test_query = test_query.to_string();

        let handle = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            println!("🔍 开始测试 {} 搜索引擎...", engine_name);

            // 使用超时机制防止测试挂起
            let search_future = engine.search(&test_query, max_results, None, None);
            let result = timeout(timeout_duration, search_future).await;

            match result {
                Ok(Ok(search_results)) => {
                    println!(
                        "✅ {} 搜索成功，返回 {} 条结果",
                        engine_name,
                        search_results.len()
                    );

                    // 验证搜索结果
                    if search_results.is_empty() {
                        println!("⚠️  {} 未返回任何搜索结果", engine_name);
                        return (engine_name.clone(), false, "无搜索结果".to_string());
                    }

                    // 检查搜索结果质量
                    let mut valid_results = 0;
                    let mut contains_gemini = 0;

                    for (idx, result) in search_results.iter().enumerate() {
                        if idx < 3 {
                            // 只打印前3个结果
                            println!(
                                "  {} 结果 {}: {} - {}",
                                engine_name,
                                idx + 1,
                                result.title,
                                result.url
                            );
                        }

                        // 验证结果完整性
                        if !result.title.is_empty() && !result.url.is_empty() {
                            valid_results += 1;
                        }

                        // 检查是否包含关键词
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

    // 收集所有测试结果
    let mut results = vec![];
    for handle in handles {
        if let Ok(result) = handle.await {
            results.push(result);
        }
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

    // 如果有失败的测试，让整个测试失败
    if failed > 0 {
        panic!("❌ 搜索引擎测试失败: {} 个引擎测试未通过", failed);
    }

    println!("🎉 所有搜索引擎测试通过！");
}

/// 测试三个搜索引擎的性能对比
#[tokio::test]
async fn test_available_engines_performance() {
    let test_query = "gemini";
    let max_results = 5;

    println!("⚡ 开始可用搜索引擎性能测试...");

    let engines: Vec<(&str, Arc<dyn SearchEngine>)> = vec![
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

        // 在测试之间添加延迟，避免过于频繁的请求
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // 性能报告
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

/// 快速测试 - 只测试一个搜索引擎
#[tokio::test]
async fn test_single_engine_quick() {
    let test_query = "gemini";
    let max_results = 3;

    println!("🚀 快速测试 Bing 搜索引擎，关键词: {}", test_query);

    let engine = BingSearchEngine::new();

    match engine.search(test_query, max_results, None, None).await {
        Ok(results) => {
            println!("✅ Bing 搜索成功，返回 {} 条结果", results.len());

            for (idx, result) in results.iter().enumerate() {
                println!("  结果 {}: {}", idx + 1, result.title);
                println!("         {}", result.url);
                if let Some(desc) = &result.description {
                    println!("         {}", desc.chars().take(100).collect::<String>());
                }
                println!();
            }
        }
        Err(error) => {
            println!("❌ Bing 搜索失败: {}", error);
            panic!("搜索引擎测试失败");
        }
    }
}
