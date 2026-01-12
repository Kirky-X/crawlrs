// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 共享的 HTTP 客户端模块
//!
//! 提供所有搜索引擎共用的 HTTP 客户端实例，避免重复创建。

use once_cell::sync::Lazy;
use reqwest::Client;
use std::time::Duration;

/// 共享的 HTTP 客户端实例
///
/// 使用 once_cell::sync::Lazy 确保在整个应用生命周期中只初始化一次。
/// 配置包括：
/// - 合理的超时时间
/// - 连接池配置
/// - 保持连接设置
pub static SHARED_HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .unwrap_or_else(|_| Client::new())
});

/// 创建配置了超时时间的 HTTP 客户端
///
/// 适用于需要自定义超时时间的场景。
pub fn create_http_client_with_timeout(timeout_secs: u64) -> Client {
    Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .unwrap_or_else(|_| Client::new())
}
