// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Fallback 搜索引擎真实 API 验证
//!
//! 验证 Exa、Parallel、Tavily 三个 fallback 搜索引擎在真实网络环境下
//! 能够正常返回搜索结果。
//!
//! 运行方式：
//! ```bash
//! cargo run --example test_fallback_engines --features default
//! ```

use crawlrs::search::client::exa::{ExaConfig, ExaSearchEngine};
use crawlrs::search::client::parallel::{ParallelConfig, ParallelSearchEngine};
use crawlrs::search::client::tavily::{TavilyConfig, TavilySearchEngine};
use crawlrs::search::engine_trait::{SearchEngine, SearchRequest};
use reqwest::Client;
use std::time::Duration;

fn create_http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client")
}

async fn test_exa() -> bool {
    println!("═══════════════════════════════════════════");
    println!("  测试 Exa 搜索引擎（MCP JSON-RPC 2.0）");
    println!("═══════════════════════════════════════════");

    let client = create_http_client();
    let config = ExaConfig::default();
    let engine = ExaSearchEngine::new(client, config);

    let request = SearchRequest::new("Rust programming language").with_limit(3);
    match engine.search(&request).await {
        Ok(response) => {
            println!("  ✓ 搜索成功！返回 {} 条结果", response.items.len());
            for (i, item) in response.items.iter().enumerate().take(3) {
                println!("  [{}] {}", i + 1, item.title);
                println!("      URL: {}", item.url);
                let desc = &item.description[..item.description.len().min(100)];
                println!("      描述: {}...", desc);
            }
            !response.items.is_empty()
        }
        Err(e) => {
            println!("  ✗ 搜索失败: {:?}", e);
            false
        }
    }
}

async fn test_parallel() -> bool {
    println!("\n═══════════════════════════════════════════");
    println!("  测试 Parallel 搜索引擎（MCP JSON-RPC 2.0）");
    println!("═══════════════════════════════════════════");

    let client = create_http_client();
    let config = ParallelConfig::default();
    let engine = ParallelSearchEngine::new(client, config);

    let request = SearchRequest::new("What is Rust programming language").with_limit(3);
    match engine.search(&request).await {
        Ok(response) => {
            println!("  ✓ 搜索成功！返回 {} 条结果", response.items.len());
            for (i, item) in response.items.iter().enumerate().take(3) {
                println!("  [{}] {}", i + 1, item.title);
                println!("      URL: {}", item.url);
                let desc = &item.description[..item.description.len().min(100)];
                println!("      描述: {}...", desc);
            }
            !response.items.is_empty()
        }
        Err(e) => {
            println!("  ✗ 搜索失败: {:?}", e);
            false
        }
    }
}

async fn test_tavily() -> bool {
    println!("\n═══════════════════════════════════════════");
    println!("  测试 Tavily 搜索引擎（REST API, keyless）");
    println!("═══════════════════════════════════════════");

    let client = create_http_client();
    let config = TavilyConfig::default();
    let engine = TavilySearchEngine::new(client, config);

    let request = SearchRequest::new("Rust programming language").with_limit(3);
    match engine.search(&request).await {
        Ok(response) => {
            println!("  ✓ 搜索成功！返回 {} 条结果", response.items.len());
            for (i, item) in response.items.iter().enumerate().take(3) {
                println!("  [{}] {}", i + 1, item.title);
                println!("      URL: {}", item.url);
                let desc = &item.description[..item.description.len().min(100)];
                println!("      描述: {}...", desc);
            }
            !response.items.is_empty()
        }
        Err(e) => {
            println!("  ✗ 搜索失败: {:?}", e);
            false
        }
    }
}

#[tokio::main]
async fn main() {
    println!("╔═══════════════════════════════════════════╗");
    println!("║  Fallback 搜索引擎真实 API 验证          ║");
    println!("╚═══════════════════════════════════════════╝\n");

    let exa_ok = test_exa().await;
    let parallel_ok = test_parallel().await;
    let tavily_ok = test_tavily().await;

    println!("\n╔═══════════════════════════════════════════╗");
    println!("║  测试结果汇总                            ║");
    println!("╠═══════════════════════════════════════════╣");
    println!(
        "║  Exa:      {}                        ║",
        if exa_ok { "✓ PASS" } else { "✗ FAIL" }
    );
    println!(
        "║  Parallel: {}                        ║",
        if parallel_ok { "✓ PASS" } else { "✗ FAIL" }
    );
    println!(
        "║  Tavily:   {}                        ║",
        if tavily_ok { "✓ PASS" } else { "✗ FAIL" }
    );
    println!("╚═══════════════════════════════════════════╝");

    if !exa_ok || !parallel_ok || !tavily_ok {
        std::process::exit(1);
    }
}
