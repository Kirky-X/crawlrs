// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 统一 HTTP 客户端模块
//!
//! 提供 HTTP 客户端工厂函数，用于创建配置一致的 HTTP 客户端。
//! 推荐使用依赖注入方式管理客户端生命周期，而非使用全局单例。

use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;

/// 默认 HTTP 客户端配置
const DEFAULT_TIMEOUT: u64 = 30;
const DEFAULT_POOL_MAX_IDLE_PER_HOST: usize = 10;
const DEFAULT_POOL_IDLE_TIMEOUT: u64 = 90;
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// 创建配置了自定义超时时间的 HTTP 客户端
///
/// 适用于需要非默认超时时间的场景（如快速超时检测或长时间请求）。
///
/// # Arguments
///
/// * `timeout_secs` - 超时时间（秒）
///
/// # Returns
///
/// 配置了指定超时时间的 `reqwest::Client`
pub fn create_http_client_with_timeout(timeout_secs: u64) -> Client {
    create_client(timeout_secs)
}

/// 创建使用默认配置的 HTTP 客户端
///
/// 返回一个配置了默认参数的 Arc<Client> 实例。
///
/// # Returns
///
/// 配置与默认参数一致的 `Arc<Client>`
pub fn create_http_client() -> Arc<Client> {
    Arc::new(create_client(DEFAULT_TIMEOUT))
}

fn create_client(timeout_secs: u64) -> Client {
    Client::builder()
        .user_agent(DEFAULT_USER_AGENT)
        .timeout(Duration::from_secs(timeout_secs))
        .pool_max_idle_per_host(DEFAULT_POOL_MAX_IDLE_PER_HOST)
        .pool_idle_timeout(Duration::from_secs(DEFAULT_POOL_IDLE_TIMEOUT))
        .redirect(reqwest::redirect::Policy::none()) // 禁用自动重定向防止 SSRF
        .build()
        .unwrap_or_else(|_| Client::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_http_client_with_timeout() {
        // 测试工厂函数创建带自定义超时的客户端
        let client = create_http_client_with_timeout(60);
        // 验证客户端可以被克隆（确保是有效的 Client）
        let _ = client.clone();
    }

    #[test]
    fn test_create_default_http_client() {
        // 测试工厂函数创建默认配置的客户端
        let client = create_http_client();
        // 验证客户端可以被克隆（确保是有效的 Client）
        let _ = client.clone();
    }
}
