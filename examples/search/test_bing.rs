// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Bing 搜索引擎真实搜索测试

use crate::test_harness::SearchTestHarness;
use crawlrs::search::client::bing::BingSearchEngine;

const TIMEOUT_SECS: u64 = 60;

#[tokio::main]
async fn main() {
    let harness = SearchTestHarness::new("Bing", TIMEOUT_SECS, 10);
    harness.run_full_test(BingSearchEngine::new()).await;
}
