// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 应用核心配置
//!
//! 包含数据库、Redis、服务器、速率限制和并发控制等核心配置项

use confers::Config;
use serde::{Deserialize, Serialize};

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
/// * `max_lifetime` - 连接最大存活时间（秒），默认 1800 秒（30 分钟）
/// * `connection_keepalive` - 连接存活检查间隔（秒），默认 30 秒
/// * `health_check_interval` - 空闲连接健康检查间隔（秒），默认 60 秒
///
/// # 安全提示
///
/// `url` 字段包含数据库连接字符串，可能包含敏感信息（密码等）。
/// 该字段仅对 crate 可见，外部模块应使用 `url()` 方法访问。
#[derive(Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__DATABASE__")]
pub struct DatabaseSettings {
    /// 数据库连接URL (敏感信息)
    /// 注意：此字段包含敏感信息，仅 crate 内部可访问
    pub(crate) url: String,

    /// 最大连接数
    #[config(default = 150)]
    pub max_connections: Option<u32>,

    /// 最小连接数
    #[config(default = 20)]
    pub min_connections: Option<u32>,

    /// 连接超时时间（秒）
    #[config(default = 15)]
    pub connect_timeout: Option<u64>,

    /// 空闲连接超时时间（秒）
    #[config(default = 300)]
    pub idle_timeout: Option<u64>,

    /// 连接最大存活时间（秒）
    #[config(default = 1800)]
    pub max_lifetime: Option<u64>,

    /// 连接存活检查间隔（秒）
    #[config(default = 30)]
    pub connection_keepalive: Option<u64>,

    /// 健康检查间隔（秒）
    #[config(default = 60)]
    pub health_check_interval: Option<u64>,
}

impl std::fmt::Debug for DatabaseSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseSettings")
            .field("url", &"***REDACTED***")
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connect_timeout", &self.connect_timeout)
            .field("idle_timeout", &self.idle_timeout)
            .field("max_lifetime", &self.max_lifetime)
            .field("connection_keepalive", &self.connection_keepalive)
            .field("health_check_interval", &self.health_check_interval)
            .finish()
    }
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

/// Redis配置设置
///
/// 配置 Redis 连接参数和连接池设置
///
/// # 字段说明
///
/// * `url` - Redis 连接字符串，格式为 redis://host:port/db（敏感信息，仅 crate 可见）
/// * `max_connections` - 连接池中最大连接数，默认 20
/// * `min_connections` - 连接池中最小连接数，默认 5
/// * `connection_timeout` - 连接超时时间（秒），默认 10 秒
/// * `idle_timeout` - 空闲连接超时时间（秒），默认 300 秒
///
/// # 安全提示
///
/// `url` 字段包含 Redis 连接字符串，可能包含敏感信息（密码等）。
/// 该字段仅对 crate 可见，外部模块应使用 `url()` 方法访问。
#[derive(Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__REDIS__")]
pub struct RedisSettings {
    /// Redis连接URL (敏感信息)
    /// 注意：此字段包含敏感信息，仅 crate 内部可访问
    pub(crate) url: String,

    /// 最大连接数
    #[config(default = 20)]
    pub max_connections: Option<u32>,

    /// 最小连接数（连接池预热）
    #[config(default = 5)]
    pub min_connections: Option<u32>,

    /// 连接超时时间（秒）
    #[config(default = 10)]
    pub connection_timeout: Option<u64>,

    /// 空闲连接超时时间（秒）
    #[config(default = 300)]
    pub idle_timeout: Option<u64>,
}

impl std::fmt::Debug for RedisSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisSettings")
            .field("url", &"***REDACTED***")
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connection_timeout", &self.connection_timeout)
            .field("idle_timeout", &self.idle_timeout)
            .finish()
    }
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

    /// 获取最大连接数
    pub fn max_connections(&self) -> u32 {
        self.max_connections.unwrap_or(20)
    }

    /// 获取最小连接数
    pub fn min_connections(&self) -> u32 {
        self.min_connections.unwrap_or(5)
    }

    /// 获取连接超时时间（秒）
    pub fn connection_timeout(&self) -> u64 {
        self.connection_timeout.unwrap_or(10)
    }

    /// 获取空闲连接超时时间（秒）
    pub fn idle_timeout(&self) -> u64 {
        self.idle_timeout.unwrap_or(300)
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
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__SERVER__")]
pub struct ServerSettings {
    /// 服务器监听主机地址
    #[config(default = "0.0.0.0".to_string())]
    pub host: String,

    /// 服务器监听端口
    #[config(default = 8899)]
    pub port: u16,

    /// 是否开启端口嗅探功能
    #[config(default = true)]
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
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__RATE_LIMITING__")]
pub struct RateLimitingSettings {
    /// 是否启用速率限制
    #[config(default = true)]
    pub enabled: bool,

    /// 默认每分钟请求数限制
    #[config(default = 100)]
    pub default_rpm: u32,

    /// 默认速率限制（别名，兼容旧代码）
    #[config(default = 100)]
    pub default_limit: u32,

    /// 突发请求数大小
    #[config(default = 20)]
    pub burst_size: u32,
}

/// 并发控制配置设置
///
/// 控制系统并发度和资源使用的参数
///
/// # 字段说明
///
/// * `default_team_limit` - 每个团队的最大并发任务数，默认 10
/// * `task_lock_duration_seconds` - 任务锁持续时间，防止重复处理，默认 300 秒（5 分钟）
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__CONCURRENCY__")]
pub struct ConcurrencySettings {
    /// 默认团队并发限制
    #[config(default = 10)]
    pub default_team_limit: i64,

    /// 任务锁持续时间（秒）
    #[config(default = 300)]
    pub task_lock_duration_seconds: i64,
}

#[cfg(test)]
mod tests {
    use crate::config::WorkerCount;

    #[test]
    fn test_worker_count_fixed() {
        let count = WorkerCount::Fixed(10);
        assert_eq!(count.resolve(), 10);
    }

    #[test]
    fn test_worker_count_auto_returns_positive() {
        let count = WorkerCount::Auto("auto".to_string());
        let resolved = count.resolve();
        // Auto 模式应该返回大于 0 的值（基于 CPU 核心数）
        assert!(
            resolved > 0,
            "Auto mode should return positive value, got {}",
            resolved
        );
    }

    #[test]
    fn test_worker_count_default_is_auto() {
        let count = WorkerCount::default();
        // 默认应该是 Auto 模式
        match count {
            WorkerCount::Auto(_) => {} // 正确
            WorkerCount::Fixed(n) => panic!("Expected Auto, got Fixed({})", n),
        }
    }
}
