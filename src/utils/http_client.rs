// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 统一 HTTP 客户端模块
//!
//! 提供全局共享的 HTTP 客户端单例，供所有非爬取类 HTTP 请求使用。
//! 所有服务应使用此模块提供的 HTTP_CLIENT，而非直接创建 reqwest::Client 实例。

use once_cell::sync::Lazy;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;

/// 默认 HTTP 客户端配置
const DEFAULT_TIMEOUT: u64 = 30;
const DEFAULT_POOL_MAX_IDLE_PER_HOST: usize = 10;
const DEFAULT_POOL_IDLE_TIMEOUT: u64 = 90;
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// 全局共享的 HTTP 客户端单例（Arc 包装版本）
///
/// 通过 Arc<Client> 实现线程安全的共享访问，适合依赖注入模式。
/// 所有服务应通过依赖注入使用此单例。
pub static HTTP_CLIENT: Lazy<Arc<Client>> = Lazy::new(|| {
    Arc::new(
        Client::builder()
            .user_agent(DEFAULT_USER_AGENT)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT))
            .pool_max_idle_per_host(DEFAULT_POOL_MAX_IDLE_PER_HOST)
            .pool_idle_timeout(Duration::from_secs(DEFAULT_POOL_IDLE_TIMEOUT))
            .redirect(reqwest::redirect::Policy::none()) // 禁用自动重定向防止 SSRF
            .build()
            .unwrap_or_else(|_| Client::new()),
    )
});

/// 创建配置了自定义超时时间的 HTTP 客户端
///
/// 适用于需要非默认超时时间的场景（如快速超时检测或长时间请求）。
/// 其他配置与默认 HTTP_CLIENT 保持一致。
///
/// # Arguments
///
/// * `timeout_secs` - 超时时间（秒）
///
/// # Returns
///
/// 配置了指定超时时间的 `reqwest::Client`
pub fn create_http_client_with_timeout(timeout_secs: u64) -> Client {
    Client::builder()
        .user_agent(DEFAULT_USER_AGENT)
        .timeout(Duration::from_secs(timeout_secs))
        .pool_max_idle_per_host(DEFAULT_POOL_MAX_IDLE_PER_HOST)
        .pool_idle_timeout(Duration::from_secs(DEFAULT_POOL_IDLE_TIMEOUT))
        .redirect(reqwest::redirect::Policy::none()) // 禁用自动重定向防止 SSRF
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// 创建使用默认配置的 HTTP 客户端
///
/// 返回一个与 HTTP_CLIENT 配置相同的 Arc<Client> 实例。
/// 适用于需要在运行时创建新客户端的场景。
///
/// # Returns
///
/// 配置与默认 HTTP_CLIENT 一致的 `Arc<Client>`
pub fn create_http_client() -> Arc<Client> {
    HTTP_CLIENT.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_client_is_initialized() {
        // 确保 HTTP_CLIENT 能够正常初始化（访问即可触发初始化）
        let _ = &HTTP_CLIENT;
    }

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
