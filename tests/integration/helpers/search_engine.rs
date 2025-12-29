// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crawlrs::domain::search::engine::SearchEngine;
use crawlrs::infrastructure::search::baidu::BaiduSearchEngine;
use crawlrs::infrastructure::search::bing::BingSearchEngine;
use crawlrs::infrastructure::search::google::GoogleSearchEngine;
use crawlrs::infrastructure::search::sogou::SogouSearchEngine;
use std::sync::Arc;

#[allow(dead_code)]
pub fn create_search_engines() -> Vec<(&'static str, Arc<dyn SearchEngine>)> {
    vec![
        ("Google", Arc::new(GoogleSearchEngine::new())),
        ("Bing", Arc::new(BingSearchEngine::new())),
        ("Baidu", Arc::new(BaiduSearchEngine::new())),
        ("Sogou", Arc::new(SogouSearchEngine::new())),
    ]
}

#[allow(dead_code)]
pub fn create_single_engine(engine_name: &str) -> Option<Arc<dyn SearchEngine>> {
    match engine_name {
        "Google" => Some(Arc::new(GoogleSearchEngine::new())),
        "Bing" => Some(Arc::new(BingSearchEngine::new())),
        "Baidu" => Some(Arc::new(BaiduSearchEngine::new())),
        "Sogou" => Some(Arc::new(SogouSearchEngine::new())),
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
