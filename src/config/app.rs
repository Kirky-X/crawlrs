// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 应用核心配置
//!
//! 包含数据库、Redis、服务器、速率限制和并发控制等核心配置项

use serde::Deserialize;

/// 数据库配置设置
///
/// 配置数据库连接参数和连接池设置
///
/// # 字段说明
///
/// * `url` - PostgreSQL 数据库连接字符串（敏感信息，仅 crate 可见）
/// * `max_connections` - 连接池中最大连接数，默认 100
/// * `min_connections` - 连接池中最小连接数，默认 10
/// * `connect_timeout` - 连接超时时间（秒），默认 10 秒
/// * `idle_timeout` - 空闲连接超时时间（秒），默认 300 秒
///
/// # 安全提示
///
/// `url` 字段包含数据库连接字符串，可能包含敏感信息（密码等）。
/// 该字段仅对 crate 可见，外部模块应使用 `url()` 方法访问。
#[derive(Clone, Deserialize, Default)]
pub struct DatabaseSettings {
    /// 数据库连接URL (敏感信息)
    pub(crate) url: String,
    /// 最大连接数
    pub max_connections: Option<u32>,
    /// 最小连接数
    pub min_connections: Option<u32>,
    /// 连接超时时间（秒）
    pub connect_timeout: Option<u64>,
    /// 空闲连接超时时间（秒）
    pub idle_timeout: Option<u64>,
}

impl DatabaseSettings {
    /// 获取数据库连接URL
    ///
    /// # 安全提示
    ///
    /// 此方法返回包含敏感信息的连接字符串，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl std::fmt::Debug for DatabaseSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseSettings")
            .field("url", &"[REDACTED]")
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connect_timeout", &self.connect_timeout)
            .field("idle_timeout", &self.idle_timeout)
            .finish()
    }
}

/// Redis配置设置
///
/// 配置 Redis 连接参数
///
/// # 字段说明
///
/// * `url` - Redis 连接字符串，格式为 redis://host:port/db（敏感信息，仅 crate 可见）
///
/// # 安全提示
///
/// `url` 字段包含 Redis 连接字符串，可能包含敏感信息（密码等）。
/// 该字段仅对 crate 可见，外部模块应使用 `url()` 方法访问。
#[derive(Clone, Deserialize, Default)]
pub struct RedisSettings {
    /// Redis连接URL (敏感信息)
    pub(crate) url: String,
}

impl RedisSettings {
    /// 获取 Redis 连接URL
    ///
    /// # 安全提示
    ///
    /// 此方法返回包含敏感信息的连接字符串，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl std::fmt::Debug for RedisSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisSettings")
            .field("url", &"[REDACTED]")
            .finish()
    }
}

/// 服务器配置设置
///
/// 配置 HTTP 服务器的监听参数
///
/// # 字段说明
///
/// * `host` - 服务器监听的主机地址，通常为 "0.0.0.0" 或 "127.0.0.1"
/// * `port` - 服务器监听的端口号，默认 3000
/// * `enable_port_detection` - 是否开启端口嗅探功能
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ServerSettings {
    /// 服务器监听主机地址
    pub host: String,
    /// 服务器监听端口
    pub port: u16,
    /// 是否开启端口嗅探功能
    pub enable_port_detection: bool,
}

/// 速率限制配置设置
///
/// 控制 API 请求的速率限制参数
///
/// # 字段说明
///
/// * `enabled` - 是否启用速率限制，默认 true
/// * `default_rpm` - 默认每分钟请求数限制，默认 100
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RateLimitingSettings {
    /// 是否启用速率限制
    pub enabled: bool,
    /// 默认每分钟请求数限制
    pub default_rpm: u32,
}

/// 并发控制配置设置
///
/// 控制系统并发度和资源使用的参数
///
/// # 字段说明
///
/// * `default_team_limit` - 每个团队的最大并发任务数，默认 10
/// * `task_lock_duration_seconds` - 任务锁持续时间，防止重复处理，默认 300 秒（5 分钟）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConcurrencySettings {
    /// 默认团队并发限制
    pub default_team_limit: i64,
    /// 任务锁持续时间（秒）
    pub task_lock_duration_seconds: i64,
}

/// HTTP代理配置设置
///
/// 配置HTTP代理参数，用于转发爬虫请求
///
/// # 字段说明
///
/// * `url` - 代理服务器URL，支持 http、https、socks5 协议（敏感信息，仅 crate 可见）
/// * `enabled` - 是否启用代理，默认 false
///
/// # 安全提示
///
/// `url` 字段可能包含代理认证信息（用户名和密码）。
/// 该字段仅对 crate 可见，外部模块应使用 `url()` 方法访问。
///
/// # 示例
///
/// ```
/// # use serde::Deserialize;
/// # use std::collections::HashMap;
///
/// #[derive(Debug, Clone, Deserialize)]
/// pub struct ProxySettings {
///     pub(crate) url: String,
///     pub enabled: bool,
/// }
/// ```
#[derive(Clone, Deserialize, Default)]
pub struct ProxySettings {
    /// 代理服务器URL (可能包含认证信息)
    /// 格式: http://host:port, https://host:port, socks5://host:port
    /// 包含认证: http://user:pass@host:port
    pub(crate) url: String,
    /// 是否启用代理
    pub enabled: bool,
}

impl ProxySettings {
    /// 获取代理服务器URL
    ///
    /// # 安全提示
    ///
    /// 此方法返回可能包含认证信息的代理 URL，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl std::fmt::Debug for ProxySettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxySettings")
            .field("url", &"[REDACTED]")
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// Worker配置设置
///
/// 配置后台Worker进程的数量和类型
///
/// # 字段说明
///
/// * `count` - Worker数量配置，可以是固定数字或"auto"自动检测
#[derive(Debug, Clone, Deserialize, Default)]
pub struct WorkerSettings {
    /// Worker数量配置
    pub count: WorkerCount,
}

/// Worker数量配置
///
/// 支持固定数量或自动检测CPU核心数
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WorkerCount {
    /// 自动检测CPU核心数
    Auto(String),
    /// 固定数量
    Fixed(usize),
}

impl Default for WorkerCount {
    fn default() -> Self {
        WorkerCount::Fixed(5)
    }
}

impl WorkerCount {
    /// 解析为实际的worker数量
    ///
    /// # 返回值
    ///
    /// * Auto模式: 返回逻辑核心数的1-2倍，限制在[2, 16]范围内
    /// * Fixed模式: 返回固定数值
    /// * 其他: 返回默认值5
    pub fn resolve(&self) -> usize {
        match self {
            WorkerCount::Auto(s) if s.eq_ignore_ascii_case("auto") => {
                let logical_cores = std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4);

                // 使用逻辑核心数的1-2倍，但限制在合理范围
                let workers = logical_cores * 2;
                workers.clamp(2, 16)
            }
            WorkerCount::Fixed(n) => *n,
            _ => 5, // 默认值
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_count_fixed() {
        let count = WorkerCount::Fixed(10);
        assert_eq!(count.resolve(), 10);
    }

    #[test]
    fn test_worker_count_auto_in_range() {
        let count = WorkerCount::Auto("auto".to_string());
        let resolved = count.resolve();
        // 验证在合理范围内
        assert!(resolved >= 2 && resolved <= 16);
    }

    #[test]
    fn test_worker_count_default() {
        let count = WorkerCount::default();
        assert_eq!(count.resolve(), 5);
    }
}

/// 超时配置设置
///
/// 配置各种操作的超时时间
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TimeoutSettings {
    /// Worker相关超时
    pub workers: WorkerTimeoutSettings,
    /// 引擎相关超时
    pub engines: EngineTimeoutSettings,
    /// 重试策略超时
    pub retry: RetryTimeoutSettings,
    /// 缓存TTL设置
    pub cache: CacheTimeoutSettings,
}

/// Worker超时设置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct WorkerTimeoutSettings {
    /// Webhook worker处理间隔（秒）
    #[serde(default = "default_webhook_interval")]
    pub webhook_interval_seconds: u64,

    /// Backlog worker处理间隔（秒）
    #[serde(default = "default_backlog_interval")]
    pub backlog_interval_seconds: u64,
}

/// 引擎超时设置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct EngineTimeoutSettings {
    /// 默认请求超时（秒）
    #[serde(default = "default_engine_timeout")]
    pub default_timeout_seconds: u64,

    /// Playwright引擎超时（秒）
    #[serde(default = "default_playwright_timeout")]
    pub playwright_timeout_seconds: u64,

    /// FlareSolverr超时（秒）
    #[serde(default = "default_flaresolverr_timeout")]
    pub flaresolverr_timeout_seconds: u64,
}

/// 重试超时设置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RetryTimeoutSettings {
    /// 初始退避时间（秒）
    #[serde(default = "default_initial_backoff")]
    pub initial_backoff_seconds: u64,

    /// 最大退避时间（秒）
    #[serde(default = "default_max_backoff")]
    pub max_backoff_seconds: u64,
}

/// 缓存超时设置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CacheTimeoutSettings {
    /// 默认TTL（秒）
    #[serde(default = "default_cache_ttl")]
    pub default_ttl_seconds: u64,

    /// 内存缓存TTL（秒）
    #[serde(default = "default_memory_ttl")]
    pub memory_ttl_seconds: u64,

    /// Redis缓存TTL（秒）
    #[serde(default = "default_redis_ttl")]
    pub redis_ttl_seconds: u64,
}

// 默认值函数
fn default_webhook_interval() -> u64 {
    5
}
fn default_backlog_interval() -> u64 {
    30
}
fn default_engine_timeout() -> u64 {
    30
}
fn default_playwright_timeout() -> u64 {
    30
}
fn default_flaresolverr_timeout() -> u64 {
    30
}
fn default_initial_backoff() -> u64 {
    1
}
fn default_max_backoff() -> u64 {
    60
}
fn default_cache_ttl() -> u64 {
    600 // 增加到 10 分钟
}
fn default_memory_ttl() -> u64 {
    600 // 增加到 10 分钟
}
fn default_redis_ttl() -> u64 {
    7200 // 增加到 2 小时
}
