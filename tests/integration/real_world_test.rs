use crawlrs::engines::reqwest_engine::ReqwestEngine;
use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
use std::collections::HashMap;
use std::time::Duration;
use tokio;
use testcontainers::{runners::AsyncRunner, GenericImage};
use crawlrs::engines::fire_engine_cdp::FireEngineCdp;
use crawlrs::engines::fire_engine_tls::FireEngineTls;

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
    let client = reqwest::Client::new();
    let health_url = base_url; // Assuming base_url is like http://ip:port
    println!("Checking Flaresolverr health at {}", health_url);
    
    for i in 0..15 {
        match client.get(health_url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    println!("Flaresolverr is ready!");
                    return;
                } else {
                    println!("Flaresolverr returned status: {}", resp.status());
                }
            }
            Err(e) => println!("Waiting for Flaresolverr... ({}) {:?}", i, e),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    panic!("Flaresolverr failed to start after 30 seconds");
}

#[tokio::test]
async fn test_real_world_reqwest_engine() {
    let engine = ReqwestEngine;
    let request = create_base_request();

    println!("Testing ReqwestEngine with URL: {}", TEST_URL);
    let result = engine.scrape(&request).await;
    
    match result {
        Ok(response) => {
            println!("=== ReqwestEngine 抓取结果 ===");
            println!("状态码: {}", response.status_code);
            println!("内容长度: {} 字符", response.content.len());
            println!("响应内容预览 (前500字符):");
            println!("{}", &response.content[..response.content.len().min(500)]);
            println!("=== 结束 ===");
            
            assert_eq!(response.status_code, 200, "Expected status code 200");
            assert!(!response.content.is_empty(), "Response content should not be empty");
        },
        Err(e) => {
            panic!("ReqwestEngine failed: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_real_world_playwright_engine() {
    use crawlrs::engines::playwright_engine::PlaywrightEngine;
    
    println!("Testing PlaywrightEngine with Docker-based Chromium setup...");
    
    // Use Docker to run a container with Chromium pre-installed and remote debugging enabled
    println!("Starting Chromium container with remote debugging...");
    let output = std::process::Command::new("docker")
        .args(&[
            "run", "-d", "--rm",
            "--name", "chromium-test",
            "-p", "9222:9222",
            "--cap-add=SYS_ADMIN",
            "zenika/alpine-chrome",
            "chromium-browser",
            "--headless",
            "--disable-gpu",
            "--disable-dev-shm-usage",
            "--remote-debugging-address=0.0.0.0",
            "--remote-debugging-port=9222",
            "--no-sandbox"
        ])
        .output()
        .expect("Failed to start Docker container");
    
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        panic!("Failed to start Chromium container: {}", error);
    }
    
    let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    println!("Started Chromium container: {}", container_id);
    
    // Wait for container to be ready
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    // Set environment variable to use remote debugging
    std::env::set_var("CHROMIUM_REMOTE_DEBUGGING_URL", "http://localhost:9222");
    
    let engine = PlaywrightEngine;
    let mut request = create_base_request();
    request.needs_js = true; 

    println!("Testing PlaywrightEngine with URL: {}", TEST_URL);
    let result = engine.scrape(&request).await;
    
    // Clean up container
    std::env::remove_var("CHROMIUM_REMOTE_DEBUGGING_URL");
    std::process::Command::new("docker")
        .args(&["stop", &container_id])
        .output()
        .ok();
    
    match result {
        Ok(response) => {
            println!("=== PlaywrightEngine 抓取结果 ===");
            println!("状态码: {}", response.status_code);
            println!("内容长度: {} 字符", response.content.len());
            println!("响应内容预览 (前500字符):");
            println!("{}", &response.content[..response.content.len().min(500)]);
            println!("=== 结束 ===");
            
            assert_eq!(response.status_code, 200, "Expected status code 200");
            assert!(!response.content.is_empty(), "Response content should not be empty");
        },
        Err(e) => {
            panic!("PlaywrightEngine failed: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_real_world_fire_engine_cdp() {
    println!("Starting Flaresolverr container for CDP test...");
    let flaresolverr = GenericImage::new("ghcr.io/flaresolverr/flaresolverr", "latest")
        .with_exposed_port(8191)
        .start()
        .await
        .expect("Failed to start flaresolverr");
        
    let port = flaresolverr.get_host_port_ipv4(8191).await.expect("port");
    let base_url = format!("http://127.0.0.1:{}", port);
    let api_url = format!("{}/v1", base_url);
    println!("Flaresolverr started at {}", base_url);
    
    // Wait for Flaresolverr to be ready
    wait_for_flaresolverr(&base_url).await;
    
    std::env::set_var("FIRE_ENGINE_CDP_URL", &api_url);
    
    let engine = FireEngineCdp::new();
    let mut request = create_base_request();
    request.needs_js = true;
    request.use_fire_engine = true;

    println!("Testing FireEngineCdp with URL: {}", TEST_URL);
    let result = engine.scrape(&request).await;
    
    std::env::remove_var("FIRE_ENGINE_CDP_URL");

    match result {
        Ok(response) => {
            println!("=== FireEngineCdp 抓取结果 ===");
            println!("状态码: {}", response.status_code);
            println!("内容长度: {} 字符", response.content.len());
            println!("响应内容预览 (前500字符):");
            println!("{}", &response.content[..response.content.len().min(500)]);
            println!("=== 结束 ===");
            
            assert_eq!(response.status_code, 200, "Expected status code 200");
            assert!(!response.content.is_empty(), "Response content should not be empty");
        },
        Err(e) => {
             panic!("FireEngineCdp failed: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_real_world_fire_engine_tls() {
    println!("Starting Flaresolverr container for TLS test...");
    let flaresolverr = GenericImage::new("ghcr.io/flaresolverr/flaresolverr", "latest")
        .with_exposed_port(8191)
        .start()
        .await
        .expect("Failed to start flaresolverr");
        
    let port = flaresolverr.get_host_port_ipv4(8191).await.expect("port");
    let base_url = format!("http://127.0.0.1:{}", port);
    let api_url = format!("{}/v1", base_url);
    println!("Flaresolverr started at {}", base_url);
    
    // Wait for Flaresolverr to be ready
    wait_for_flaresolverr(&base_url).await;
    
    std::env::set_var("FIRE_ENGINE_TLS_URL", &api_url);
    
    let engine = FireEngineTls::new();
    let mut request = create_base_request();
    request.needs_tls_fingerprint = true;
    request.use_fire_engine = true;

    println!("Testing FireEngineTls with URL: {}", TEST_URL);
    let result = engine.scrape(&request).await;
    
    std::env::remove_var("FIRE_ENGINE_TLS_URL");

    match result {
        Ok(response) => {
            println!("=== FireEngineTls 抓取结果 ===");
            println!("状态码: {}", response.status_code);
            println!("内容长度: {} 字符", response.content.len());
            println!("响应内容预览 (前500字符):");
            println!("{}", &response.content[..response.content.len().min(500)]);
            println!("=== 结束 ===");
            
            assert_eq!(response.status_code, 200, "Expected status code 200");
            assert!(!response.content.is_empty(), "Response content should not be empty");
        },
        Err(e) => {
             panic!("FireEngineTls failed: {:?}", e);
        }
    }
}
