#![cfg(feature = "engine-mllm")]
//! MLLM 引擎活体验证（吸收点 F12：混合 DOM+AX 树快照喂 MLLM）。
//!
//! 前置：
//!   1. 本机 Ollama 并已拉取视觉模型：`ollama pull moondream`
//!   2. Chromium 可用（引擎下载器按需拉取，或复用 playwright 缓存）
//!
//! 运行：cargo test --features full,engine-mllm --test mllm_ax_tree_live -- --ignored --nocapture
//!
//! 覆盖两条 F12 通道：
//!   - 通道一（`ax_tree_capture_produces_outline`）：真实 Chromium 页面上的
//!     AX 树快照经 prune/serialize 产出 `[id] role: name` 文本大纲；
//!   - 通道二（`mllm_engine_end_to_end_with_ax_tree`）：全局浏览器池驱动
//!     `MllmEngine::scrape` 完整 agentic 导航循环，`ax_tree_enabled = true`
//!     时每轮视觉决策附 AX 大纲（`genai` 经 Ollama 适配器走本机端点）。

use futures::StreamExt;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crawlrs::domain::services::llm::vision_adapter::{GenaiVisionAdapter, VisionAdapterTrait};
use crawlrs::engines::client::mllm::MllmEngine;
use crawlrs::engines::client::playwright_pool::{init_global_pool, BrowserPoolConfig};
use crawlrs::engines::engine_client::{HttpMethod, ScraperEngine};
use crawlrs::engines::mllm::config::MllmEngineConfig;
use crawlrs::engines::types::InternalScrapeRequest;
use crawlrs::infrastructure::services::config_service::BrowserConfigComponent;

/// 视觉模型可用环境变量覆盖（如 CRAWLRS_TEST_VISION_MODEL=ollama:gemma4:e4b）
fn vision_model() -> String {
    std::env::var("CRAWLRS_TEST_VISION_MODEL").unwrap_or_else(|_| "ollama:moondream".to_string())
}

fn test_page_html() -> String {
    "<html><head><title>Aurora Dynamics — Corporate History</title></head><body>\
     <h1>Aurora Dynamics</h1>\
     <p id='intro'>Aurora Dynamics was founded in 1958 in Zurich.</p>\
     <a href='#about' id='about-link'>About us</a>\
     <button id='go'>Go</button>\
     </body></html>"
        .to_string()
}

async fn launch_page() -> (chromiumoxide::Browser, chromiumoxide::Page) {
    let config = chromiumoxide::BrowserConfig::builder()
        .no_sandbox()
        .arg("--disable-gpu")
        .arg("--disable-dev-shm-usage")
        .request_timeout(Duration::from_secs(30))
        .build()
        .expect("browser config");
    let (browser, mut handler) = chromiumoxide::Browser::launch(config)
        .await
        .expect("browser launch");
    tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            if let Err(e) = h {
                log::debug!("browser handler event: {e:?}");
            }
        }
    });
    let page = browser.new_page("about:blank").await.expect("new page");
    (browser, page)
}

/// F12 核心通道一：AX 树快照在真实页面上产出含页面语义的大纲。
#[tokio::test]
#[ignore = "需要本机 Chromium 与 Ollama 视觉模型（ollama pull moondream）"]
async fn ax_tree_capture_produces_outline() {
    let (mut browser, page) = launch_page().await;
    let url = format!("data:text/html,{}", test_page_html());
    page.goto(url.as_str()).await.expect("goto");
    tokio::time::sleep(Duration::from_millis(800)).await;

    let nodes = crawlrs::engines::mllm::ax_tree::capture(&page)
        .await
        .expect("ax tree capture");
    let outline = crawlrs::engines::mllm::ax_tree::outline(nodes, 2048);
    println!("=== AX outline ===\n{outline}");
    assert!(!outline.is_empty(), "AX 大纲不应为空");
    assert!(
        outline.contains("Aurora Dynamics") || outline.contains("heading"),
        "大纲应包含页面标题或 heading 角色"
    );
    browser.close().await.ok();
}

/// F12 端到端：全局浏览器池驱动 MllmEngine 完整导航循环。
#[tokio::test]
#[ignore = "需要本机 Ollama 视觉模型（ollama pull moondream）"]
async fn mllm_engine_end_to_end_with_ax_tree() {
    init_global_pool(
        BrowserPoolConfig::default(),
        Arc::new(BrowserConfigComponent::default()),
    );

    let config = MllmEngineConfig {
        vision_model: vision_model(),
        max_iterations: 2,
        screenshot_quality: 60,
        max_token_budget: 2048,
        mrt_seconds: 120,
        ax_tree_enabled: true,
        ..MllmEngineConfig::default()
    };
    let engine = MllmEngine::new(Arc::new(GenaiVisionAdapter::new()), config);

    let url = format!("data:text/html,{}", test_page_html());
    let request = InternalScrapeRequest {
        url,
        method: HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(120),
        needs_js: true,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: true,
    };

    let response = engine.scrape(&request).await.expect("mllm scrape");
    let head: String = response.content.chars().take(400).collect();
    println!("=== MLLM content (head) ===\n{head}");
    assert_eq!(response.status_code, 200, "status={}", response.status_code);
    assert!(!response.content.is_empty(), "内容不应为空");
}
