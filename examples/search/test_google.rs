// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(deprecated)]

//! Google 搜索引擎真实搜索测试

use crate::test_harness::SearchTestHarness;
use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::engines::engine_client::EngineClient;
use crawlrs::engines::traits::ScraperEngine;
use crawlrs::search::client::google::GoogleSearchEngine;
use std::sync::Arc;

const TIMEOUT_SECS: u64 = 90;

#[tokio::main]
async fn main() {
    let reqwest_engine = Arc::new(ReqwestEngine);
    let fire_engine_cdp = Arc::new(crawlrs::engines::client::fire_cdp::FireEngineCdp::new());
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine, fire_engine_cdp];
    let engine_client = Arc::new(EngineClient::with_engines(engines));

    let harness = SearchTestHarness::new("Google", TIMEOUT_SECS, 10);
    harness
        .run_full_test(GoogleSearchEngine::new(engine_client))
        .await;
}
