// 临时验证 WreqEngine 对 tls.peet.ws 的 JA4 输出（T024 证据）。验证后删除。
use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::engines::client::wreq_engine::WreqEngine;
use crawlrs::engines::engine_client::{InternalScrapeRequest, ScraperEngine};
use crawlrs::utils::ua_pool::UaPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

fn mkreq(url: &str) -> InternalScrapeRequest {
    InternalScrapeRequest {
        url: url.to_string(),
        method: crawlrs::common::HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: true,
        needs_tls_fingerprint: true,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
    }
}

#[tokio::main]
async fn main() {
    // 对照：ReqwestEngine 是否也受 IP-rewrite + SNI 影响
    let rclient = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    match ReqwestEngine::new(Arc::new(rclient))
        .scrape(&mkreq("https://tls.peet.ws/api/all"))
        .await
    {
        Ok(r) => println!(
            "reqwest status={} body_head={:?}",
            r.status_code,
            r.content.chars().take(80).collect::<String>()
        ),
        Err(e) => println!("reqwest ERR: {e}"),
    }

    // WreqEngine
    let engine = WreqEngine::new(Arc::new(UaPool::new()), Duration::from_secs(15), 30).unwrap();
    match engine.scrape(&mkreq("https://tls.peet.ws/api/all")).await {
        Ok(resp) => {
            println!("status={}", resp.status_code);
            let v: serde_json::Value = serde_json::from_str(&resp.content).unwrap();
            println!("http_version={}", v["http_version"]);
            println!("user_agent={}", v["user_agent"]);
            println!("tls.tls_version_record={}", v["tls"]["tls_version_record"]);
            println!("ja4={}", v["tls"]["ja4"]);
            println!("ja3={}", v["tls"]["ja3"]);
        }
        Err(e) => println!("wreq ERR: {e}"),
    }
}