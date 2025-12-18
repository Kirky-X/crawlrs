// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crawlrs::engines::fire_engine_cdp::FireEngineCdp;
use crawlrs::engines::fire_engine_tls::FireEngineTls;
use crawlrs::engines::reqwest_engine::ReqwestEngine;
use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
use std::collections::HashMap;
use std::time::Duration;
use testcontainers::{runners::AsyncRunner, GenericImage};
use tracing::info;

const TEST_URL: &str = "https://news.sina.com.cn/c/xl/2025-12-17/doc-inhcaekp2520228.shtml";

fn create_base_request() -> ScrapeRequest {
    ScrapeRequest {
        url: TEST_URL.to_string(),
        headers: HashMap::new(),
        timeout: Duration::from_secs(60), // Increased timeout for Flaresolverr
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
    }
}

async fn wait_for_flaresolverr(base_url: &str) {
    let _client = reqwest::Client::new();
    
    // Try multiple endpoints that Flaresolverr might respond to
    let endpoints = vec![
        format!("{}/v1/health", base_url),
        format!("{}/", base_url),
        format!("{}/v1", base_url),
    ];
    
    info!("Checking Flaresolverr health at multiple endpoints");
    let mut found = false;
    
    for i in 0..30 {
        for endpoint in &endpoints {
            match reqwest::get(endpoint).await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        info!("Flaresolverr is ready! Responding to: {}", endpoint);
                        found = true;
                        break;
                    } else {
                        info!("Flaresolverr endpoint {} returned status: {}", endpoint, resp.status());
                    }
                }
                Err(e) => info!("Endpoint {} not ready: {:?}", endpoint, e),
            }
        }
        
        if found {
            break;
        }
        
        info!("Waiting for Flaresolverr... attempt {}", i);
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    
    if !found {
        panic!("Flaresolverr failed to start after 30 seconds");
    }
}

#[tokio::test]
async fn test_real_world_reqwest_engine() {
    let engine = ReqwestEngine;
    let request = create_base_request();

    info!("Testing ReqwestEngine with URL: {}", TEST_URL);
    let result = engine.scrape(&request).await;

    match result {
        Ok(response) => {
            info!("=== ReqwestEngine 抓取结果 ===");
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
            panic!("ReqwestEngine failed: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_real_world_playwright_engine() {
    use crawlrs::engines::playwright_engine::PlaywrightEngine;

    info!("Testing PlaywrightEngine with Docker-based Chromium setup...");

    // Use Docker to run a container with Chromium pre-installed and remote debugging enabled
    info!("Starting Chromium container with remote debugging...");
    let output = std::process::Command::new("docker")
        .args(&[
            "run",
            "-d",
            "--rm",
            "--name",
            "chromium-test",
            "-p",
            "9222:9222",
            "--cap-add=SYS_ADMIN",
            "zenika/alpine-chrome",
            "chromium-browser",
            "--headless",
            "--disable-gpu",
            "--disable-dev-shm-usage",
            "--remote-debugging-address=0.0.0.0",
            "--remote-debugging-port=9222",
            "--no-sandbox",
        ])
        .output()
        .expect("Failed to start Docker container");

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        panic!("Failed to start Chromium container: {}", error);
    }

    let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    info!("Started Chromium container: {}", container_id);

    // Wait for container to be ready
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Set environment variable to use remote debugging
    std::env::set_var("CHROMIUM_REMOTE_DEBUGGING_URL", "http://localhost:9222");

    let engine = PlaywrightEngine;
    let mut request = create_base_request();
    request.needs_js = true;

    info!("Testing PlaywrightEngine with URL: {}", TEST_URL);
    let result = engine.scrape(&request).await;

    // Clean up container
    std::env::remove_var("CHROMIUM_REMOTE_DEBUGGING_URL");
    std::process::Command::new("docker")
        .args(&["stop", &container_id])
        .output()
        .ok();

    match result {
        Ok(response) => {
            info!("=== PlaywrightEngine 抓取结果 ===");
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
            panic!("PlaywrightEngine failed: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_real_world_fire_engine_cdp() {
    info!("Starting Flaresolverr container for CDP test...");
    let flaresolverr = GenericImage::new("ghcr.io/flaresolverr/flaresolverr", "latest")
        .with_exposed_port(testcontainers::core::ContainerPort::Tcp(8191))
        .start()
        .await
        .expect("Failed to start flaresolverr");

    let port = flaresolverr.get_host_port_ipv4(8191).await.expect("port");
    let base_url = format!("http://127.0.0.1:{}", port);
    let api_url = format!("{}/v1", base_url);
    info!("Flaresolverr started at {}", base_url);

    // Wait for Flaresolverr to be ready
    wait_for_flaresolverr(&base_url).await;

    std::env::set_var("FIRE_ENGINE_CDP_URL", &api_url);

    let engine = FireEngineCdp::new();
    let mut request = create_base_request();
    request.needs_js = true;
    request.use_fire_engine = true;

    info!("Testing FireEngineCdp with URL: {}", TEST_URL);
    let result = engine.scrape(&request).await;

    std::env::remove_var("FIRE_ENGINE_CDP_URL");

    match result {
        Ok(response) => {
            info!("=== FireEngineCdp 抓取结果 ===");
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
            panic!("FireEngineCdp failed: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_real_world_fire_engine_tls() {
    info!("Starting Flaresolverr container for TLS test...");
    let flaresolverr = GenericImage::new("ghcr.io/flaresolverr/flaresolverr", "latest")
        .with_exposed_port(testcontainers::core::ContainerPort::Tcp(8191))
        .start()
        .await
        .expect("Failed to start flaresolverr");

    let port = flaresolverr.get_host_port_ipv4(8191).await.expect("port");
    let base_url = format!("http://127.0.0.1:{}", port);
    let api_url = format!("{}/v1", base_url);
    info!("Flaresolverr started at {}", base_url);

    // Wait for Flaresolverr to be ready
    wait_for_flaresolverr(&base_url).await;

    std::env::set_var("FIRE_ENGINE_TLS_URL", &api_url);

    let engine = FireEngineTls::new();
    let mut request = create_base_request();
    request.needs_tls_fingerprint = true;
    request.use_fire_engine = true;

    info!("Testing FireEngineTls with URL: {}", TEST_URL);
    let result = engine.scrape(&request).await;

    std::env::remove_var("FIRE_ENGINE_TLS_URL");

    match result {
        Ok(response) => {
            info!("=== FireEngineTls 抓取结果 ===");
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
            panic!("FireEngineTls failed: {:?}", e);
        }
    }
}
