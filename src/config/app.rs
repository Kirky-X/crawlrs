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
/// * `url` - PostgreSQL 数据库连接字符串
/// * `max_connections` - 连接池中最大连接数，默认 100
/// * `min_connections` - 连接池中最小连接数，默认 10
/// * `connect_timeout` - 连接超时时间（秒），默认 10 秒
/// * `idle_timeout` - 空闲连接超时时间（秒），默认 300 秒
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseSettings {
    /// 数据库连接URL
    pub url: String,
    /// 最大连接数
    pub max_connections: Option<u32>,
    /// 最小连接数
    pub min_connections: Option<u32>,
    /// 连接超时时间（秒）
    pub connect_timeout: Option<u64>,
    /// 空闲连接超时时间（秒）
    pub idle_timeout: Option<u64>,
}

/// Redis配置设置
///
/// 配置 Redis 连接参数
///
/// # 字段说明
///
/// * `url` - Redis 连接字符串，格式为 redis://host:port/db
#[derive(Debug, Clone, Deserialize)]
pub struct RedisSettings {
    /// Redis连接URL
    pub url: String,
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
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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
/// * `url` - 代理服务器URL，支持 http、https、socks5 协议
/// * `enabled` - 是否启用代理，默认 false
///
/// # 示例
///
/// ```
/// # use serde::Deserialize;
/// # use std::collections::HashMap;
///
/// #[derive(Debug, Clone, Deserialize)]
/// pub struct ProxySettings {
///     pub url: String,
///     pub enabled: bool,
/// }
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct ProxySettings {
    /// 代理服务器URL
    /// 格式: http://host:port, https://host:port, socks5://host:port
    /// 包含认证: http://user:pass@host:port
    pub url: String,
    /// 是否启用代理
    pub enabled: bool,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            url: "http://localhost:10808".to_string(),
            enabled: false,
        }
    }
}
