// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 全引擎真实网站爬取测试
//!
//! 逐个测试所有网络引擎，验证能否爬取真实网站内容。
//! 测试目标：
//! - ReqwestEngine（基础 HTTP）
//! - PlaywrightEngine（浏览器渲染）
//! - FlareSolverrEngine Full 模式
//! - FlareSolverrEngine CDP 模式
//! - FlareSolverrEngine TLS 模式
//! - WreqEngine（TLS 指纹）
//!
//! 运行前提：
//! 1. Docker 服务已启动：PostgreSQL + FlareSolverr
//!    ```bash
//!    cd docker && docker compose --profile infrastructure --profile browser up -d
//!    ```
//! 2. 系统已安装 Chrome 浏览器（PlaywrightEngine 使用）
//!
//! 运行命令：
//! ```bash
//! cargo run --bin test_all_engines --features "engine-playwright,engine-flaresolverr,engine-tls-fingerprint"
//! ```

#![allow(unexpected_cfgs)]

use crawlrs::engines::engine_client::{InternalScrapeRequest, ScraperEngine};
use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::common::HttpMethod;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

// ===== 测试目标真实网站 =====
// 选择标准：
// - HTTP 引擎（reqwest/wreq）：SSRF resolve 覆盖保持原始 hostname，TLS SNI 和虚拟主机路由正常。
// - 浏览器引擎（playwright/flaresolverr）：使用 JS 渲染丰富的站点。
const TEST_URLS: &[(&str, &str, &str)] = &[
    ("https://github.com", "GitHub", "<title>GitHub"),
    ("https://en.wikipedia.org/wiki/Web_scraping", "Wikipedia", "Web scraping"),
    ("https://news.ycombinator.com", "Hacker News", "Hacker News"),
    ("https://www.rust-lang.org", "Rust Lang", "Rust"),
    ("https://stackoverflow.com", "StackOverflow", "Stack Overflow"),
];

/// 构造基础 InternalScrapeRequest
fn make_request(url: &str, needs_js: bool, skip_tls: bool) -> InternalScrapeRequest {
    InternalScrapeRequest {
        url: url.to_string(),
        method: HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(60),
        needs_js,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: skip_tls,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: false,
    }
}

/// 测试结果
struct TestResult {
    engine_name: String,
    url: String,
    site_name: String,
    success: bool,
    status_code: u16,
    content_len: usize,
    has_expected_content: bool,
    elapsed_ms: u64,
    error: Option<String>,
}

impl TestResult {
    fn display(&self) {
        let status_icon = if self.success && self.has_expected_content {
            "✅"
        } else if self.success {
            "⚠️"
        } else {
            "❌"
        };

        println!(
            "{} [{}] {} → {} | status={} | content={} bytes | {}ms",
            status_icon,
            self.engine_name,
            self.site_name,
            self.url,
            self.status_code,
            self.content_len,
            self.elapsed_ms,
        );

        if let Some(ref err) = self.error {
            println!("   └─ 错误: {}", err);
        } else if !self.has_expected_content {
            println!("   └─ 警告: 未找到预期内容标识");
        }
    }
}

/// 测试单个引擎对多个网站的爬取
async fn test_engine(
    engine_name: &str,
    engine: &dyn ScraperEngine,
    needs_js: bool,
    skip_tls: bool,
) -> Vec<TestResult> {
    let mut results = Vec::new();

    for (url, site_name, expected_content) in TEST_URLS {
        let request = make_request(url, needs_js, skip_tls);
        let start = Instant::now();

        let result = match engine.scrape(&request).await {
            Ok(response) => TestResult {
                engine_name: engine_name.to_string(),
                url: url.to_string(),
                site_name: site_name.to_string(),
                success: (200..300).contains(&response.status_code),
                status_code: response.status_code,
                content_len: response.content.len(),
                has_expected_content: response.content.contains(expected_content),
                elapsed_ms: start.elapsed().as_millis() as u64,
                error: None,
            },
            Err(e) => TestResult {
                engine_name: engine_name.to_string(),
                url: url.to_string(),
                site_name: site_name.to_string(),
                success: false,
                status_code: 0,
                content_len: 0,
                has_expected_content: false,
                elapsed_ms: start.elapsed().as_millis() as u64,
                error: Some(e.to_string()),
            },
        };

        result.display();
        results.push(result);
    }

    results
}

#[tokio::main]
async fn main() {
    println!("==========================================================");
    println!(" crawlrs 全引擎真实网站爬取测试");
    println!("==========================================================\n");

    let mut all_results: Vec<TestResult> = Vec::new();
    let http_client: Arc<reqwest::Client> = Arc::new(reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("Failed to build HTTP client"));

    // ================================================================
    // 1. ReqwestEngine（基础 HTTP 引擎，始终可用）
    // ================================================================
    println!("\n━━━ 1/5 ReqwestEngine（基础 HTTP）━━━");
    {
        let engine = ReqwestEngine::new_with_timeout_and_mrt(
            http_client.clone(),
            60,
            Duration::from_secs(30),
        );
        let results = test_engine("reqwest", &engine, false, false).await;
        all_results.extend(results);
    }

    // ================================================================
    // 2. PlaywrightEngine（浏览器渲染引擎）
    // ================================================================
    #[cfg(feature = "engine-playwright")]
    {
        println!("\n━━━ 2/5 PlaywrightEngine（浏览器渲染）━━━");
        use crawlrs::engines::client::playwright::PlaywrightEngine;
        let engine = PlaywrightEngine::new();
        let results = test_engine("playwright", &engine, true, false).await;
        all_results.extend(results);
    }
    #[cfg(not(feature = "engine-playwright"))]
    println!("\n━━━ 2/5 PlaywrightEngine — 跳过（未启用 engine-playwright feature）━━━");

    // ================================================================
    // 3. FlareSolverrEngine Full 模式
    // ================================================================
    #[cfg(feature = "engine-flaresolverr")]
    {
        println!("\n━━━ 3/5 FlareSolverrEngine Full 模式━━━");
        use crawlrs::engines::client::flare_solverr::FlareSolverrEngine;
        let engine = FlareSolverrEngine::with_url(
            http_client.clone(),
            "http://localhost:8191/v1",
        );
        let results = test_engine("flaresolverr_full", &engine, true, false).await;
        all_results.extend(results);
    }
    #[cfg(not(feature = "engine-flaresolverr"))]
    println!("\n━━━ 3/5 FlareSolverrEngine Full — 跳过（未启用 engine-flaresolverr feature）━━━");

    // ================================================================
    // 4. FlareSolverrEngine CDP 模式
    // ================================================================
    #[cfg(feature = "engine-flaresolverr")]
    {
        println!("\n━━━ 4/5 FlareSolverrEngine CDP 模式━━━");
        use crawlrs::engines::client::flare_solverr::FlareSolverrEngine;
        let engine = FlareSolverrEngine::with_cdp_mode(http_client.clone(), None);
        let results = test_engine("flaresolverr_cdp", &engine, true, false).await;
        all_results.extend(results);
    }
    #[cfg(not(feature = "engine-flaresolverr"))]
    println!("\n━━━ 4/5 FlareSolverrEngine CDP — 跳过（未启用 engine-flaresolverr feature）━━━");

    // ================================================================
    // 5. FlareSolverrEngine TLS 模式
    // ================================================================
    #[cfg(feature = "engine-flaresolverr")]
    {
        println!("\n━━━ 5/5 FlareSolverrEngine TLS 模式━━━");
        use crawlrs::engines::client::flare_solverr::FlareSolverrEngine;
        let engine = FlareSolverrEngine::with_tls_mode(http_client.clone(), None);
        let results = test_engine("flaresolverr_tls", &engine, false, false).await;
        all_results.extend(results);
    }
    #[cfg(not(feature = "engine-flaresolverr"))]
    println!("\n━━━ 5/5 FlareSolverrEngine TLS — 跳过（未启用 engine-flaresolverr feature）━━━");

    // ================================================================
    // 6. WreqEngine（TLS 指纹引擎）— 可选
    // ================================================================
    #[cfg(feature = "engine-tls-fingerprint")]
    {
        println!("\n━━━ 6/6 WreqEngine（TLS 指纹）━━━");
        use crawlrs::engines::client::wreq_engine::WreqEngine;
        use crawlrs::utils::ua_pool::UaPool;
        match WreqEngine::new(
            Arc::new(UaPool::new()),
            Duration::from_secs(15),
            15,
        ) {
            Ok(engine) => {
                let results = test_engine("wreq", &engine, false, false).await;
                all_results.extend(results);
            }
            Err(e) => {
                println!("❌ WreqEngine 初始化失败: {}", e);
            }
        }
    }
    #[cfg(not(feature = "engine-tls-fingerprint"))]
    println!("\n━━━ WreqEngine — 跳过（未启用 engine-tls-fingerprint feature）━━━");

    // ================================================================
    // 测试汇总
    // ================================================================
    println!("\n==========================================================");
    println!(" 测试汇总");
    println!("==========================================================");

    let total = all_results.len();
    let success_with_content = all_results.iter().filter(|r| r.success && r.has_expected_content).count();
    let success_no_content = all_results.iter().filter(|r| r.success && !r.has_expected_content).count();
    let failed = all_results.iter().filter(|r| !r.success).count();

    println!("总测试数: {}", total);
    println!("✅ 成功且内容正确: {}", success_with_content);
    println!("⚠️  成功但内容未验证: {}", success_no_content);
    println!("❌ 失败: {}", failed);

    // 按引擎分组统计
    println!("\n按引擎分组:");
    let engine_names: Vec<String> = {
        let mut names: Vec<String> = all_results.iter().map(|r| r.engine_name.clone()).collect();
        names.sort();
        names.dedup();
        names
    };

    for name in &engine_names {
        let engine_results: Vec<&TestResult> = all_results.iter().filter(|r| &r.engine_name == name).collect();
        let ok = engine_results.iter().filter(|r| r.success && r.has_expected_content).count();
        let total = engine_results.len();
        let avg_ms = if engine_results.is_empty() {
            0
        } else {
            engine_results.iter().map(|r| r.elapsed_ms).sum::<u64>() / engine_results.len() as u64
        };
        let status = if ok == total { "✅" } else if ok > 0 { "⚠️" } else { "❌" };
        println!("  {} {}: {}/{} 通过 (平均 {}ms)", status, name, ok, total, avg_ms);
    }

    println!("\n==========================================================");

    if failed > 0 {
        println!("⚠️  有 {} 个测试失败，请检查上方日志", failed);
        std::process::exit(1);
    } else {
        println!("🎉 所有测试通过！");
    }
}
