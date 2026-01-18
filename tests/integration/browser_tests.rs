// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(dead_code)]
#![allow(deprecated)]

use super::helpers::browser_helpers::create_scrape_request;
use super::helpers::google_helpers::{get_chrome_ws_url, set_chrome_ws_url};
use crate::common::constants::timeouts::QUICK_TEST_TIMEOUT;
#[cfg(feature = "engine-playwright")]
use crawlrs::engines::client::playwright::PlaywrightEngine;

pub async fn test_simple_http_page() -> bool {
    println!("\n1. 测试访问 httpbin.org...");
    let request = create_scrape_request("https://httpbin.org/html".to_string(), true, 15);

    match PlaywrightEngine.scrape(&request).await {
        Ok(response) => {
            println!("✅ 成功访问页面");
            println!("状态码: {:?}", response.status_code);
            println!("内容长度: {} 字符", response.content.len());
            if response.content.len() > 100 {
                println!("前100个字符: {}", &response.content[..100]);
            }
            true
        }
        Err(e) => {
            println!("❌ 访问失败: {:?}", e);
            false
        }
    }
}

pub async fn test_example_com(needs_js: bool, timeout_secs: u64) -> bool {
    println!("\n测试访问 example.com（不需要JS）...");
    let request = create_scrape_request("https://example.com".to_string(), needs_js, timeout_secs);

    match PlaywrightEngine.scrape(&request).await {
        Ok(response) => {
            println!("✅ 成功访问 example.com");
            println!("状态码: {:?}", response.status_code);
            println!("内容长度: {} 字符", response.content.len());
            if response.content.len() > 100 {
                println!("前100个字符: {}", &response.content[..100]);
            }
            true
        }
        Err(e) => {
            println!("❌ 访问失败: {:?}", e);
            false
        }
    }
}

#[allow(dead_code)]
pub async fn test_google_search(needs_js: bool, timeout_secs: u64) -> bool {
    println!("\n测试访问 Google（需要JS）...");
    let request =
        create_scrape_request("https://www.google.com".to_string(), needs_js, timeout_secs);

    match PlaywrightEngine.scrape(&request).await {
        Ok(response) => {
            println!("✅ 成功访问 Google");
            println!("状态码: {:?}", response.status_code);
            println!("内容长度: {} 字符", response.content.len());
            if response.content.len() > 200 {
                println!("前200个字符: {}", &response.content[..200]);
            }
            true
        }
        Err(e) => {
            println!("❌ Google 访问失败: {:?}", e);
            false
        }
    }
}

#[allow(dead_code)]
pub async fn test_google_search_with_query(query: &str, timeout_secs: u64) -> bool {
    let url = format!(
        "https://www.google.com/search?q={}",
        query.replace(' ', "+")
    );
    println!("\n测试访问Google搜索: {}...", query);
    let request = create_scrape_request(url, true, timeout_secs);

    match PlaywrightEngine.scrape(&request).await {
        Ok(response) => {
            println!("✅ 成功访问Google搜索");
            println!("状态码: {:?}", response.status_code);
            println!("内容长度: {} 字符", response.content.len());
            if response.content.len() > 200 {
                println!("前200个字符: {}", &response.content[..200]);
            }
            true
        }
        Err(e) => {
            println!("❌ Google搜索访问失败: {:?}", e);
            false
        }
    }
}

/// 测试简单浏览器连接
///
/// 注意：此测试需要Chrome浏览器运行在 localhost:9222。
/// 如需运行此测试，请使用: cargo test --test integration_tests -- test_browser_connection_simple -- --include-ignored
#[ignore]
#[tokio::test]
async fn test_browser_connection_simple() {
    println!("=== 浏览器连接测试 ===");

    // Set environment variable to avoid browser reuse conflicts
    std::env::set_var("CRAWLRS_TEST_NO_BROWSER_REUSE", "1");

    set_chrome_ws_url("http://localhost:9222");

    // Add a delay to ensure browser connection is stable
    tokio::time::sleep(QUICK_TEST_TIMEOUT).await;

    let result = test_simple_http_page().await;
    assert!(result, "简单HTTP页面访问测试失败");

    println!("🎉 简单浏览器连接测试通过！");
}

#[tokio::test]
#[ignore] // Ignoring this test because it requires Chrome at localhost:9222
async fn test_browser_connection_debug() {
    println!("=== 浏览器连接调试测试 ===");

    // Set environment variable to avoid browser reuse conflicts
    std::env::set_var("CRAWLRS_TEST_NO_BROWSER_REUSE", "1");

    set_chrome_ws_url("http://localhost:9222");

    // Add a delay to ensure browser connection is stable
    tokio::time::sleep(QUICK_TEST_TIMEOUT).await;

    // 只测试需要JS渲染的情况，因为Playwright引擎只在needs_js=true时执行
    let result = test_example_com(true, 15).await;

    assert!(result, "example.com JS访问测试失败");

    println!("🎉 浏览器连接调试测试通过！");
}

/// 直接测试Playwright连接
///
/// 注意：此测试需要Chrome DevTools Protocol可用（http://localhost:9222）。
/// 如需运行此测试，请使用: cargo test --test integration_tests -- test_playwright_direct -- --include-ignored
#[ignore]
#[tokio::test]
async fn test_playwright_direct() {
    println!("=== 直接测试Playwright连接 ===");

    // Set environment variable to avoid browser reuse conflicts
    std::env::set_var("CRAWLRS_TEST_NO_BROWSER_REUSE", "1");

    set_chrome_ws_url("http://localhost:9222");

    // Add a delay to ensure browser connection is stable
    tokio::time::sleep(QUICK_TEST_TIMEOUT).await;

    let result = test_example_com(true, 30).await;

    assert!(result, "example.com 访问测试失败");

    println!("🎉 Playwright直接测试通过！");
}

/// 使用远程Chrome测试浏览器功能
///
/// 注意：此测试需要远程Chrome浏览器可用（通过 CHROMIUM_REMOTE_DEBUGGING_URL 环境变量指定）。
/// 如需运行此测试，请使用: cargo test --test integration_tests -- test_browser_with_remote_chrome -- --include-ignored
#[ignore]
#[tokio::test]
async fn test_browser_with_remote_chrome() {
    println!("=== 使用远程Chrome测试浏览器功能 ===");

    // Set environment variable to avoid browser reuse conflicts
    std::env::set_var("CRAWLRS_TEST_NO_BROWSER_REUSE", "1");

    let ws_url = get_chrome_ws_url();
    println!("使用远程Chrome: {}", ws_url);
    set_chrome_ws_url(&ws_url);

    // Add a delay to ensure browser connection is stable
    tokio::time::sleep(QUICK_TEST_TIMEOUT).await;

    let result = test_example_com(true, 30).await;
    assert!(result, "使用远程Chrome访问example.com失败");

    println!("🎉 远程Chrome浏览器测试通过！");
}
