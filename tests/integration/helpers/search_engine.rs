// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(deprecated)]

use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::engines::engine_client::EngineClient;
#[cfg(feature = "engine-fire-cdp")]
use crawlrs::engines::ScraperEngine;
use crawlrs::search::client::baidu::BaiduSearchEngine;
use crawlrs::search::client::bing::BingSearchEngine;
use crawlrs::search::client::google::GoogleSearchEngine;
use crawlrs::search::client::sogou::SogouSearchEngine;
use crawlrs::search::engine_trait::SearchEngine;
use reqwest::Client;
use std::sync::Arc;

#[cfg(feature = "engine-fire-cdp")]
fn create_engine_client() -> Arc<EngineClient> {
    use crawlrs::engines::client::fire_cdp::FireEngineCdp;

    let reqwest_client = Arc::new(
        Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create reqwest client"),
    );
    let reqwest_engine = Arc::new(ReqwestEngine::new(reqwest_client.clone()));
    let fire_engine_cdp = Arc::new(FireEngineCdp::new());
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine, fire_engine_cdp];
    Arc::new(EngineClient::with_engines(engines))
}

#[cfg(not(feature = "engine-fire-cdp"))]
fn create_engine_client() -> Arc<EngineClient> {
    let reqwest_client = Arc::new(
        Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create reqwest client"),
    );
    let reqwest_engine = Arc::new(ReqwestEngine::new(reqwest_client));
    Arc::new(EngineClient::with_engines(vec![reqwest_engine]))
}

#[allow(dead_code)]
pub fn create_search_engines() -> Vec<(&'static str, Arc<dyn SearchEngine>)> {
    let engine_client = create_engine_client();
    vec![
        (
            "Google",
            Arc::new(GoogleSearchEngine::new(engine_client.clone())),
        ),
        (
            "Bing",
            Arc::new(BingSearchEngine::new(engine_client.clone())),
        ),
        (
            "Baidu",
            Arc::new(BaiduSearchEngine::new(engine_client.clone())),
        ),
        (
            "Sogou",
            Arc::new(SogouSearchEngine::new(engine_client.clone())),
        ),
    ]
}

#[allow(dead_code)]
pub fn create_single_engine(engine_name: &str) -> Option<Arc<dyn SearchEngine>> {
    let engine_client = create_engine_client();
    match engine_name {
        "Google" => Some(Arc::new(GoogleSearchEngine::new(engine_client))),
        "Bing" => Some(Arc::new(BingSearchEngine::new(engine_client))),
        "Baidu" => Some(Arc::new(BaiduSearchEngine::new(engine_client))),
        "Sogou" => Some(Arc::new(SogouSearchEngine::new(engine_client))),
        _ => None,
    }
}

#[allow(dead_code)]
pub fn enable_test_mode_full() {
    std::env::set_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS", "true");
    std::env::set_var("BING_TEST_RESULTS", "true");
    std::env::set_var("BAIDU_TEST_RESULTS", "true");
    std::env::set_var("SOGOU_TEST_RESULTS", "true");
}

#[allow(dead_code)]
pub fn enable_test_mode_simple() {
    std::env::set_var("USE_TEST_DATA", "1");
}

#[allow(dead_code)]
pub fn disable_test_mode() {
    std::env::remove_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS");
    std::env::remove_var("BING_TEST_RESULTS");
    std::env::remove_var("BAIDU_TEST_RESULTS");
    std::env::remove_var("SOGOU_TEST_RESULTS");
    std::env::remove_var("USE_TEST_DATA");
}

#[allow(dead_code)]
pub fn apply_test_mode(mode: &str) {
    match mode {
        "full" => enable_test_mode_full(),
        "simple" => enable_test_mode_simple(),
        "real" => disable_test_mode(),
        _ => {}
    }
}
