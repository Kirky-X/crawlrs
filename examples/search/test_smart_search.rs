// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SmartSearchEngine 智能搜索引擎演示
//!
//! 本示例演示了 SmartSearchEngine 的核心功能和用法，包括：
//! - 基于 EngineClient 创建智能搜索引擎
//! - 工厂方法 `create_smart_search` 创建引擎
//! - 发起搜索并处理结果/错误/超时

use crawlrs::engines::client::reqwest::ReqwestEngine;
use crawlrs::engines::engine_client::{EngineClient, ScraperEngine};
use crawlrs::search::engine_trait::{SearchEngine, SearchRequest};
use crawlrs::search::smart::{create_smart_search, SmartSearchEngine};
use log::info;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

const TEST_QUERY: &str = "rust programming language";
const TIMEOUT_SECS: u64 = 30;
const RESULT_LIMIT: u32 = 5;

#[tokio::main]
async fn main() {
    env_logger::init();

    info!("==========================================");
    info!("SmartSearchEngine 智能搜索引擎演示");
    info!("==========================================\n");

    demo_basic_usage().await;
    demo_factory_usage().await;

    info!("\n==========================================");
    info!("演示完成");
    info!("==========================================");
}

async fn demo_basic_usage() {
    info!("📖 演示一：基本用法");
    info!("----------------------------------------");

    let client = create_test_client();
    let engine = Arc::new(SmartSearchEngine::new(client));

    info!("✅ 已创建智能搜索引擎");

    let request = SearchRequest {
        query: TEST_QUERY.to_string(),
        limit: RESULT_LIMIT,
        ..Default::default()
    };

    match timeout(Duration::from_secs(TIMEOUT_SECS), engine.search(&request)).await {
        Ok(Ok(results)) => {
            info!("✅ 搜索成功！找到 {} 个结果", results.items.len());
            for (i, result) in results.items.iter().enumerate().take(3) {
                info!("  {}. {}", i + 1, result.title);
            }
        }
        Ok(Err(e)) => {
            info!("⚠️ 搜索出错: {:?}", e);
        }
        Err(_) => {
            info!("⏱️ 搜索超时");
        }
    }
    info!("");
}

async fn demo_factory_usage() {
    info!("📖 演示二：工厂方法用法");
    info!("----------------------------------------");

    let client = create_test_client();
    let engine = create_smart_search(client);

    info!("✅ 已通过 create_smart_search 工厂创建搜索引擎");

    let request = SearchRequest::new(TEST_QUERY).with_limit(RESULT_LIMIT);
    match timeout(Duration::from_secs(TIMEOUT_SECS), engine.search(&request)).await {
        Ok(Ok(results)) => {
            info!("✅ 搜索成功！找到 {} 个结果", results.items.len());
        }
        Ok(Err(e)) => {
            info!("⚠️ 搜索出错: {:?}", e);
        }
        Err(_) => {
            info!("⏱️ 搜索超时");
        }
    }
    info!("");
}

fn create_test_client() -> Arc<EngineClient> {
    let reqwest_engine: Arc<dyn ScraperEngine> = Arc::new(ReqwestEngine::new_with_timeout(
        Arc::new(
            reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap(),
        ),
        60,
    ));
    Arc::new(EngineClient::with_engines(vec![reqwest_engine]))
}
