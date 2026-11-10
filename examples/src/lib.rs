// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 示例共享库
//!
//! 提供示例程序复用的工具模块，避免各示例二进制重复实现相同逻辑。

pub mod test_harness;

/// 构造带默认 Reqwest 引擎的 [`EngineClient`]，供示例统一使用。
///
/// `EngineClient::new()` 注册零引擎（生产由 DI 注入），示例若直接使用会
/// 恒返回 "All engines failed"。此入口保持示例开箱可跑。
pub fn default_engine_client() -> crawlrs::engines::engine_client::EngineClient {
    use std::sync::Arc;
    use std::time::Duration;

    use crawlrs::engines::client::reqwest::ReqwestEngine;
    use crawlrs::engines::engine_client::{EngineClient, ScraperEngine};

    let http_client = Arc::new(
        reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("build example http client"),
    );
    let engine: Arc<dyn ScraperEngine> = Arc::new(ReqwestEngine::new(http_client));
    EngineClient::with_engines(vec![engine])
}
