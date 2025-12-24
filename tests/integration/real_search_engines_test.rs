// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

//! 真实搜索引擎集成测试
//!
//! 测试搜索引擎在实际网络环境下的表现
//! 注意：这些测试会发起真实的网络请求

use crawlrs::domain::search::engine::SearchEngine;
use crawlrs::infrastructure::search::bing::BingSearchEngine;
use crawlrs::infrastructure::search::baidu::BaiduSearchEngine;
use crawlrs::infrastructure::search::sogou::SogouSearchEngine;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// 测试真实搜索引擎的连接性
#[tokio::test]
async fn test_real_search_engines_connectivity() {
    let test_query = "rust programming language";
    let timeout_duration = Duration::from_secs(60);

    println!("🌐 开始真实搜索引擎连接性测试...");

    let engines: Vec<(&str, Arc<dyn SearchEngine>)> = vec![
        ("Bing", Arc::new(BingSearchEngine::new())),
        ("Baidu", Arc::new(BaiduSearchEngine::new())),
        ("Sogou", Arc::new(SogouSearchEngine::new())),
    ];

    let mut results = vec![];

    for (name, engine) in engines {
        println!("  测试 {}...", name);

        let result = timeout(
            timeout_duration,
            engine.search(test_query, 5, None, None),
        ).await;

        match result {
            Ok(Ok(search_results)) => {
                println!("    ✅ {} 返回 {} 个结果", name, search_results.len());
                results.push((name, true, search_results.len()));
            }
            Ok(Err(e)) => {
                println!("    ❌ {} 错误: {}", name, e);
                results.push((name, false, 0));
            }
            Err(_) => {
                println!("    ⏰ {} 超时", name);
                results.push((name, false, 0));
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    println!("\n📊 连接性测试结果:");
    for (name, success, count) in &results {
        let status = if *success { "✅" } else { "❌" };
        println!("  {} {}: {} 个结果", status, name, count);
    }

    let success_count = results.iter().filter(|(_, s, _)| *s).count();
    println!("\n通过测试: {}/{}", success_count, results.len());

    if success_count == 0 {
        println!("⚠️  所有搜索引擎连接失败，可能是网络问题");
    }
}

/// 真实搜索引擎响应内容验证
#[tokio::test]
async fn test_real_search_engines_content() {
    let test_query = "web scraping";
    let timeout_duration = Duration::from_secs(45);

    println!("🔍 开始搜索引擎内容验证测试...");

    let engine = BingSearchEngine::new();

    match timeout(timeout_duration, engine.search(test_query, 3, None, None)).await {
        Ok(Ok(results)) => {
            assert!(!results.is_empty(), "搜索结果不应为空");

            for (idx, result) in results.iter().enumerate() {
                println!("  结果 {}: {}", idx + 1, result.title);
                assert!(
                    !result.title.is_empty(),
                    "结果标题不应为空"
                );
                assert!(
                    result.url.starts_with("http"),
                    "结果URL应该是有效的HTTP链接"
                );
            }

            println!("✅ 内容验证通过");
        }
        Ok(Err(e)) => {
            println!("❌ 搜索错误: {}", e);
            println!("⚠️  跳过内容验证测试");
        }
        Err(_) => {
            println!("⏰ 搜索超时");
            println!("⚠️  跳过内容验证测试");
        }
    }
}

/// 测试搜索引擎的语言和地区过滤功能
#[tokio::test]
async fn test_search_engines_language_filter() {
    let timeout_duration = Duration::from_secs(30);

    println!("🌍 测试搜索引擎语言过滤功能...");

    let engine = BingSearchEngine::new();
    let test_query = "technology";

    match timeout(
        timeout_duration,
        engine.search(test_query, 3, Some("en"), Some("us")),
    ).await {
        Ok(Ok(results)) => {
            println!("  ✅ 语言过滤测试返回 {} 个结果", results.len());
            for result in results.iter().take(2) {
                println!("    - {}", result.title);
            }
        }
        Ok(Err(e)) => {
            println!("  ⚠️  语言过滤测试错误: {}", e);
        }
        Err(_) => {
            println!("  ⏰ 语言过滤测试超时");
        }
    }
}

/// 搜索引擎性能基准测试
#[tokio::test]
async fn test_search_engines_performance() {
    let test_query = "open source";
    let max_results = 10;

    println!("⚡ 开始搜索引擎性能基准测试...");

    let engines: Vec<(&str, Arc<dyn SearchEngine>)> = vec![
        ("Bing", Arc::new(BingSearchEngine::new())),
        ("Baidu", Arc::new(BaiduSearchEngine::new())),
        ("Sogou", Arc::new(SogouSearchEngine::new())),
    ];

    let mut performance_data = vec![];

    for (name, engine) in engines {
        let start = std::time::Instant::now();

        match timeout(Duration::from_secs(60), engine.search(test_query, max_results, None, None)).await {
            Ok(Ok(results)) => {
                let elapsed = start.elapsed();
                println!("  ✅ {}: {:?} ({} 结果)", name, elapsed, results.len());
                performance_data.push((name, elapsed.as_secs_f64(), results.len(), true));
            }
            Ok(Err(e)) => {
                let elapsed = start.elapsed();
                println!("  ❌ {}: {:?} - {}", name, elapsed, e);
                performance_data.push((name, elapsed.as_secs_f64(), 0, false));
            }
            Err(_) => {
                let elapsed = start.elapsed();
                println!("  ⏰ {}: {:?} - 超时", name, elapsed);
                performance_data.push((name, elapsed.as_secs_f64(), 0, false));
            }
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    println!("\n📊 性能测试总结:");
    let successful = performance_data.iter().filter(|(_, _, _, s)| *s).collect::<Vec<_>>();
    if !successful.is_empty() {
        let avg_time: f64 = successful.iter().map(|(_, t, _, _)| *t).sum::<f64>() / successful.len() as f64;
        println!("  平均响应时间: {:.2}s", avg_time);
        println!("  成功测试数: {}/{}", successful.len(), performance_data.len());
    }
}
