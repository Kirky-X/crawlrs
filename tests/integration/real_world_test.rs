// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 真实世界集成测试
//!
//! 测试真实网站抓取功能，需要以下之一：
//! 1. 运行中的 FlareSolverr 和 Chrome 容器
//! 2. 可用的 Playwright 环境
//!
//! 运行方式：
//! ```bash
//! # 启动浏览器服务
//! docker-compose --profile browser up -d flaresolverr chrome
//!
//! # 或跳过这些测试
//! export SKIP_BROWSER_TESTS=true
//! ```

use crate::common::constants::timeouts::{CRAWL_TASK_TIMEOUT, QUICK_TEST_TIMEOUT};
use crawlrs::engines::client::fire_cdp::FireEngineCdp;
use crawlrs::engines::client::fire_tls::FireEngineTls;
use crawlrs::engines::engine_client::{EngineClient, ScrapeOptions, ScrapeRequest};
use std::env;
use std::sync::Arc;
use std::time::Duration;
use testcontainers::{runners::AsyncRunner, GenericImage};
use tracing::info;

const TEST_URL: &str = "https://news.sina.com.cn/c/xl/2025-12-17/doc-inhcaekp2520228.shtml";

/// 检查是否应该跳过浏览器测试
fn should_skip_browser_tests() -> bool {
    env::var("SKIP_BROWSER_TESTS").is_ok()
}

/// 检查 FlareSolverr 是否可用
async fn is_flaresolverr_available() -> bool {
    let client = reqwest::Client::new();
    let endpoints = vec!["http://localhost:8191/v1/health", "http://localhost:8191/"];

    for endpoint in endpoints {
        match reqwest::get(endpoint).await {
            Ok(resp) if resp.status().is_success() => return true,
            _ => continue,
        }
    }
    false
}

fn create_base_request() -> ScrapeRequest {
    ScrapeRequest::new(TEST_URL).timeout(CRAWL_TASK_TIMEOUT) // Increased timeout for FlareSolverr
}

async fn wait_for_flaresolverr(base_url: &str) {
    let _client = reqwest::Client::new();

    // Try multiple endpoints that Flaresolverr might respond to
    let endpoints = vec![
        format!("{}/v1/health", base_url),
        format!("{}/", base_url),
        format!("{}/v1", base_url),
    ];

    info!("Checking FlareSolverr health at multiple endpoints");
    let mut found = false;

    for i in 0..30 {
        for endpoint in &endpoints {
            match reqwest::get(endpoint).await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        info!("FlareSolverr is ready! Responding to: {}", endpoint);
                        found = true;
                        break;
                    } else {
                        info!(
                            "FlareSolverr endpoint {} returned status: {}",
                            endpoint,
                            resp.status()
                        );
                    }
                }
                Err(e) => info!("Endpoint {} not ready: {:?}", endpoint, e),
            }
        }

        if found {
            break;
        }

        info!("Waiting for FlareSolverr... attempt {}", i);
        tokio::time::sleep(QUICK_TEST_TIMEOUT).await;
    }

    if !found {
        panic!("FlareSolverr failed to start after 30 seconds");
    }
}

#[tokio::test]
async fn test_real_world_reqwest_engine() {
    if should_skip_browser_tests() {
        println!("⚠️  Browser tests skipped - SKIP_BROWSER_TESTS is set");
        return;
    }

    // Create EngineClient with ReqwestEngine registered
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(ReqwestEngine::new())];
    let client = EngineClient::with_engines(engines);
    let request = create_base_request();

    info!("Testing EngineClient (reqwest) with URL: {}", TEST_URL);
    let result = client.scrape(&request).await;

    match result {
        Ok(response) => {
            info!("=== EngineClient (reqwest) 抓取结果 ===");
            info!("状态码: {}", response.status_code);
            info!("内容长度: {} 字符", response.content.len());
            info!("响应内容预览 (前500字符):");
            info!("{}", &response.content[..response.content.len().min(500)]);
            info!("=== 结束 ===");

            assert_eq!(response.status_code, 200, "Expected status code 200");
            assert!(
                !response.content.is_empty(),
                "Response content should not be empty"
            );
        }
        Err(e) => {
            panic!("EngineClient (reqwest) failed: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_real_world_playwright_engine() {
    if should_skip_browser_tests() {
        println!("⚠️  Browser tests skipped - SKIP_BROWSER_TESTS is set");
        return;
    }

    info!("Testing EngineClient (playwright) with existing Chrome container...");

    // Check if Chrome is available via environment variable
    let chrome_url = std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL")
        .unwrap_or_else(|_| "http://localhost:9222".to_string());

    info!("Using Chrome at: {}", chrome_url);

    // Create EngineClient with PlaywrightEngine registered
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(
        crawlrs::engines::client::playwright::PlaywrightEngine,
    )];
    let client = EngineClient::with_engines(engines);
    let request = create_base_request().needs_js();

    info!("Testing EngineClient (playwright) with URL: {}", TEST_URL);

    let result = crawlrs::engines::client::playwright::REMOTE_URL_OVERRIDE
        .scope(chrome_url.clone(), async { client.scrape(&request).await })
        .await;

    match result {
        Ok(response) => {
            info!("=== EngineClient (playwright) 抓取结果 ===");
            info!("状态码: {}", response.status_code);
            info!("内容长度: {} 字符", response.content.len());
            info!("响应内容预览 (前500字符):");
            info!("{}", &response.content[..response.content.len().min(500)]);
            info!("=== 结束 ===");

            assert_eq!(response.status_code, 200, "Expected status code 200");
            assert!(
                !response.content.is_empty(),
                "Response content should not be empty"
            );
        }
        Err(e) => {
            panic!("EngineClient (playwright) failed: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_real_world_fire_engine_cdp() {
    info!("Starting FlareSolverr container for CDP test...");
    let flaresolverr = GenericImage::new("ghcr.io/flaresolverr/flaresolverr", "latest")
        .with_exposed_port(testcontainers::core::ContainerPort::Tcp(8191))
        .start()
        .await
        .expect("Failed to start flaresolverr");

    let port = flaresolverr.get_host_port_ipv4(8191).await.expect("port");
    let base_url = format!("http://127.0.0.1:{}", port);
    let api_url = format!("{}/v1", base_url);
    info!("FlareSolverr started at {}", base_url);

    // Wait for FlareSolverr to be ready
    wait_for_flaresolverr(&base_url).await;

    std::env::set_var("FIRE_ENGINE_CDP_URL", &api_url);

    // Create EngineClient with FireEngineCdp registered
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(FireEngineCdp::new())];
    let client = EngineClient::with_engines(engines);
    let options = ScrapeOptions::builder()
        .needs_js(true)
        .use_fire_engine(true)
        .timeout(CRAWL_TASK_TIMEOUT)
        .build();
    let request = ScrapeRequest::new(TEST_URL).with_options(options);

    info!(
        "Testing EngineClient (FireEngineCdp) with URL: {}",
        TEST_URL
    );
    let result = client.scrape(&request).await;

    std::env::remove_var("FIRE_ENGINE_CDP_URL");

    match result {
        Ok(response) => {
            info!("=== EngineClient (FireEngineCdp) 抓取结果 ===");
            info!("状态码: {}", response.status_code);
            info!("内容长度: {} 字符", response.content.len());
            info!("响应内容预览 (前500字符):");
            info!("{}", &response.content[..response.content.len().min(500)]);
            info!("=== 结束 ===");

            assert_eq!(response.status_code, 200, "Expected status code 200");
            assert!(
                !response.content.is_empty(),
                "Response content should not be empty"
            );
        }
        Err(e) => {
            panic!("EngineClient (FireEngineCdp) failed: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_real_world_fire_engine_tls() {
    if should_skip_browser_tests() {
        println!("⚠️  Browser tests skipped - SKIP_BROWSER_TESTS is set");
        return;
    }

    info!("Starting FlareSolverr container for TLS test...");
    let flaresolverr = GenericImage::new("ghcr.io/flaresolverr/flaresolverr", "latest")
        .with_exposed_port(testcontainers::core::ContainerPort::Tcp(8191))
        .start()
        .await
        .expect("Failed to start flaresolverr");

    let port = flaresolverr.get_host_port_ipv4(8191).await.expect("port");
    let base_url = format!("http://127.0.0.1:{}", port);
    let api_url = format!("{}/v1", base_url);
    info!("FlareSolverr started at {}", base_url);

    // Wait for FlareSolverr to be ready
    wait_for_flaresolverr(&base_url).await;

    std::env::set_var("FIRE_ENGINE_TLS_URL", &api_url);

    // Create EngineClient with FireEngineTls registered
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(FireEngineTls::new())];
    let client = EngineClient::with_engines(engines);
    let options = ScrapeOptions::builder()
        .needs_tls_fingerprint(true)
        .use_fire_engine(true)
        .timeout(CRAWL_TASK_TIMEOUT)
        .build();
    let request = ScrapeRequest::new(TEST_URL).with_options(options);

    info!(
        "Testing EngineClient (FireEngineTls) with URL: {}",
        TEST_URL
    );
    let result = client.scrape(&request).await;

    std::env::remove_var("FIRE_ENGINE_TLS_URL");

    match result {
        Ok(response) => {
            info!("=== EngineClient (FireEngineTls) 抓取结果 ===");
            info!("状态码: {}", response.status_code);
            info!("内容长度: {} 字符", response.content.len());
            info!("响应内容预览 (前500字符):");
            info!("{}", &response.content[..response.content.len().min(500)]);
            info!("=== 结束 ===");

            assert_eq!(response.status_code, 200, "Expected status code 200");
            assert!(
                !response.content.is_empty(),
                "Response content should not be empty"
            );
        }
        Err(e) => {
            panic!("EngineClient (FireEngineTls) failed: {:?}", e);
        }
    }
}
