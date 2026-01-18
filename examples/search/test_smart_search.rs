// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(deprecated)]

//! SmartSearchEngine 智能搜索引擎演示
//!
//! 本示例演示了 SmartSearchEngine 的核心功能和用法，包括：
//! - 基本搜索引擎创建和使用
//! - 工厂方法创建引擎
//! - 自定义配置参数
//! - 测试数据模式

use crawlrs::engines::engine_client::EngineClient;
use crawlrs::search::engine_trait::SearchEngine;
use crawlrs::search::engine_trait::SearchRequest;
use crawlrs::search::factory::SearchEngineFactory;
use crawlrs::search::smart::{SmartSearchEngine, SmartSearchEngineConfig};
use crawlrs::search::types::SearchEngineType;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tracing::info;

const TEST_QUERY: &str = "rust programming language";
const TIMEOUT_SECS: u64 = 30;
const RESULT_LIMIT: u32 = 5;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(true)
        .init();

    info!("==========================================");
    info!("SmartSearchEngine 智能搜索引擎演示");
    info!("==========================================\n");

    demo_basic_usage().await;
    demo_factory_usage().await;
    demo_configuration().await;
    demo_test_data_mode().await;

    info!("\n==========================================");
    info!("演示完成");
    info!("==========================================");
}

async fn demo_basic_usage() {
    info!("📖 演示一：基本用法");
    info!("----------------------------------------");

    let client = create_test_client();
    let google_engine = create_google_smart_search(client);

    info!("✅ 已创建 Google 智能搜索引擎");

    let request = SearchRequest {
        query: TEST_QUERY.to_string(),
        limit: RESULT_LIMIT,
        ..Default::default()
    };

    match timeout(
        Duration::from_secs(TIMEOUT_SECS),
        google_engine.search(&request),
    )
    .await
    {
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

    let factory = SearchEngineFactory::new();

    let _google_engine = factory.create_google_smart_search();
    info!("✅ 已通过工厂创建 Google 智能搜索引擎");

    let _bing_engine = factory.create_bing_smart_search();
    info!("✅ 已通过工厂创建 Bing 智能搜索引擎");

    let _baidu_engine = factory.create_baidu_smart_search();
    info!("✅ 已通过工厂创建 Baidu 智能搜索引擎");

    info!("\n📊 工厂已注册的引擎:");
    let registered = factory.router().registered_engines();
    for engine in registered {
        info!("  - {}", engine);
    }
    info!("");
}

async fn demo_configuration() {
    info!("📖 演示三：自定义配置");
    info!("----------------------------------------");

    let client = create_test_client();

    let config = SmartSearchEngineConfig {
        engine_type: SearchEngineType::Google,
        rate_limiting_enabled: true,
        rate_limiting_service: None,
        timeout_seconds: 60,
        test_data_enabled: false,
        test_data_path: None,
        max_retries: 3,
        retry_delay_ms: 1000,
    };

    let _engine = Arc::new(SmartSearchEngine::new(client, config));
    info!("✅ 已创建带自定义配置的智能搜索引擎");
    info!("   - 超时时间: {} 秒", 60);
    info!("   - 最大重试次数: {}", 3);
    info!("   - 重试间隔: {} 毫秒", 1000);
    info!("   - 速率限制: 启用");
    info!("");
}

async fn demo_test_data_mode() {
    info!("📖 演示四：测试数据模式");
    info!("----------------------------------------");

    let client = create_test_client();

    let test_data_path = PathBuf::from("test-data/search-engines");

    let config = SmartSearchEngineConfig {
        engine_type: SearchEngineType::Google,
        rate_limiting_enabled: false,
        rate_limiting_service: None,
        timeout_seconds: 30,
        test_data_enabled: true,
        test_data_path: Some(test_data_path.clone()),
        max_retries: 1,
        retry_delay_ms: 100,
    };

    let _engine = Arc::new(SmartSearchEngine::new(client, config));
    info!("✅ 已创建启用测试数据模式的智能搜索引擎");
    info!("   测试数据路径: {:?}", test_data_path);

    if test_data_path.exists() {
        info!("   测试数据目录存在");
    } else {
        info!("   ⚠️ 测试数据目录不存在（这是正常的，如果未创建测试数据）");
    }
    info!("");
}

fn create_test_client() -> Arc<EngineClient> {
    Arc::new(EngineClient::new())
}

fn create_google_smart_search(client: Arc<EngineClient>) -> Arc<SmartSearchEngine> {
    let config = SmartSearchEngineConfig {
        engine_type: SearchEngineType::Google,
        rate_limiting_enabled: true,
        rate_limiting_service: None,
        timeout_seconds: 90,
        test_data_enabled: false,
        test_data_path: None,
        max_retries: 3,
        retry_delay_ms: 1000,
    };
    Arc::new(SmartSearchEngine::new(client, config))
}
