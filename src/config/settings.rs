// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 配置模块
//!
//! 使用 confers 库进行配置管理，支持：
//! - TOML 配置文件
//! - 环境变量覆盖 (CRAWLRS__ 前缀)
//! - 类型安全的配置解析
//! - 内置验证

use confers::Config;
use serde::{Deserialize, Serialize};
use validator::Validate;

// 重新导出子模块中的类型
pub use super::app::{
    ConcurrencySettings, DatabaseSettings, RateLimitingSettings, RedisSettings, ServerSettings,
};
pub use super::engines::{EngineSettings, FireCdpSettings, FireTlsSettings, FlareSolverrSettings};
pub use super::llm::LLMSettings;
pub use super::logging::{ConsoleLoggingSettings, FileLoggingSettings, LoggingSettings};
pub use super::search::{BingSearchSettings, SearchSettings};
pub use super::storage::{StorageSettings, WebhookSettings};

// =============================================================================
// 主配置结构
// =============================================================================

/// 应用程序配置设置
///
/// 包含数据库、Redis、服务器、速率限制和并发控制等所有配置项
///
/// # 使用示例
///
/// ```rust
/// use crawlrs::config::Settings;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let settings = Settings::load()?;
///     println!("Server will run on {}:{}", settings.server.host, settings.server.port);
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Config, Validate)]
#[config(env_prefix = "CRAWLRS__", validate)]
pub struct Settings {
    /// 服务器配置
    pub server: ServerSettings,

    /// 数据库配置
    pub database: DatabaseSettings,

    /// Redis 配置
    pub redis: RedisSettings,

    /// CORS 配置
    pub cors: CorsSettings,

    /// 速率限制配置
    pub rate_limiting: RateLimitingSettings,

    /// 并发控制配置
    pub concurrency: ConcurrencySettings,

    /// 存储配置
    pub storage: StorageSettings,

    /// Webhook 配置
    pub webhook: WebhookSettings,

    /// Bing Search API 配置
    pub bing_search: BingSearchSettings,

    /// 搜索配置 (包含 A/B 测试)
    pub search: SearchSettings,

    /// LLM 配置
    pub llm: LLMSettings,

    /// HTTP 代理配置
    pub proxy: ProxySettings,

    /// 引擎配置
    pub engines: EngineSettings,

    /// 日志配置
    pub logging: LoggingSettings,

    /// Worker 配置
    pub workers: WorkerSettings,

    /// 超时配置
    pub timeouts: TimeoutSettings,

    /// 缓存配置
    pub cache: CacheSettings,

    /// 可信代理配置
    pub trusted_proxies: TrustedProxySettings,
}

// =============================================================================
// CORS 配置
// =============================================================================

/// CORS 配置设置
///
/// 配置跨域资源共享（CORS）策略
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__CORS__")]
pub struct CorsSettings {
    /// 允许的跨域来源列表（逗号分隔）
    #[config(default = "*".to_string())]
    pub allowed_origins: String,
}

// =============================================================================
// 代理配置
// =============================================================================

/// HTTP代理配置设置
///
/// 配置HTTP代理参数，用于转发爬虫请求
#[derive(Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__PROXY__")]
pub struct ProxySettings {
    /// 代理服务器URL (可能包含认证信息)
    #[config(default = "http://localhost:10808".to_string())]
    pub(crate) url: String,

    /// 是否启用代理
    #[config(default = false)]
    pub enabled: bool,
}

impl std::fmt::Debug for ProxySettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxySettings")
            .field("url", &"***REDACTED***")
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl ProxySettings {
    /// 获取代理服务器URL
    pub fn url(&self) -> &str {
        &self.url
    }
}

// =============================================================================
// Worker 配置
// =============================================================================

/// Worker配置设置
///
/// 配置后台Worker进程的数量和类型
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__WORKERS__")]
pub struct WorkerSettings {
    /// Worker数量配置
    pub count: WorkerCount,
}

/// Worker数量配置
///
/// 支持固定数量或自动检测CPU核心数
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum WorkerCount {
    Auto(String),
    Fixed(usize),
}

impl Default for WorkerCount {
    fn default() -> Self {
        WorkerCount::Auto("auto".to_string())
    }
}

impl WorkerCount {
    /// 解析为实际的worker数量
    pub fn resolve(&self) -> usize {
        match self {
            WorkerCount::Auto(s) if s.eq_ignore_ascii_case("auto") => {
                let logical_cores = std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4);
                logical_cores * 2
            }
            WorkerCount::Fixed(n) => *n,
            _ => 5,
        }
    }
}

// =============================================================================
// 超时配置
// =============================================================================

/// 超时配置设置
///
/// 配置各种操作的超时时间
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__TIMEOUTS__")]
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
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__TIMEOUTS__WORKERS__")]
pub struct WorkerTimeoutSettings {
    /// Webhook worker处理间隔（秒）
    #[config(default = 5)]
    pub webhook_interval_seconds: u64,

    /// Backlog worker处理间隔（秒）
    #[config(default = 30)]
    pub backlog_interval_seconds: u64,
}

/// 引擎超时设置
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__TIMEOUTS__ENGINES__")]
pub struct EngineTimeoutSettings {
    /// 默认请求超时（秒）
    #[config(default = 30)]
    pub default_timeout_seconds: u64,

    /// Playwright引擎超时（秒）
    #[config(default = 30)]
    pub playwright_timeout_seconds: u64,

    /// FlareSolverr超时（秒）
    #[config(default = 30)]
    pub flaresolverr_timeout_seconds: u64,
}

/// 重试超时设置
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__TIMEOUTS__RETRY__")]
pub struct RetryTimeoutSettings {
    /// 初始退避时间（秒）
    #[config(default = 1)]
    pub initial_backoff_seconds: u64,

    /// 最大退避时间（秒）
    #[config(default = 60)]
    pub max_backoff_seconds: u64,
}

/// 缓存超时设置
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__TIMEOUTS__CACHE__")]
pub struct CacheTimeoutSettings {
    /// 默认TTL（秒）
    #[config(default = 600)]
    pub default_ttl_seconds: u64,

    /// 内存缓存TTL（秒）
    #[config(default = 600)]
    pub memory_ttl_seconds: u64,

    /// Redis缓存TTL（秒）
    #[config(default = 7200)]
    pub redis_ttl_seconds: u64,
}

// =============================================================================
// Cache Configuration
// =============================================================================

/// 缓存类型配置
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__CACHE__TYPES__")]
pub struct CacheTypeSettings {
    #[config(default = 300)]
    pub ttl_seconds: u64,
    #[config(default = 10000)]
    pub max_size: u64,
}

/// 统一缓存配置（oxcache）
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__CACHE__")]
pub struct CacheSettings {
    /// 是否启用缓存
    #[config(default = true)]
    pub enabled: bool,

    /// L1 内存缓存配置
    pub memory: MemoryCacheSettings,

    /// L2 Redis 缓存配置
    pub redis: RedisCacheSettings,

    /// 各缓存类型特定配置
    pub types: CacheTypeSpecificSettings,
}

/// L1 内存缓存配置（Moka）
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__CACHE__MEMORY__")]
pub struct MemoryCacheSettings {
    /// 最大容量
    #[config(default = 10000)]
    pub capacity: u64,
    /// TTL（秒）
    #[config(default = 300)]
    pub ttl_seconds: u64,
}

/// L2 Redis 缓存配置
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__CACHE__REDIS__")]
pub struct RedisCacheSettings {
    /// 是否启用 Redis 缓存
    #[config(default = false)]
    pub enabled: bool,

    /// Redis URL
    #[config(default = "redis://localhost:6379".to_string())]
    pub url: String,

    /// 连接池大小
    #[config(default = 10)]
    pub pool_size: u32,

    /// TTL（秒）
    #[config(default = 3600)]
    pub ttl_seconds: u64,
}

/// 各缓存类型特定配置
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__CACHE__TYPES__")]
pub struct CacheTypeSpecificSettings {
    /// 搜索结果缓存配置
    pub search: CacheTypeSettings,

    /// DNS 缓存配置
    pub dns: CacheTypeSettings,

    /// 正则缓存配置
    pub regex: CacheTypeSettings,
}

// =============================================================================
// 可信代理配置
// =============================================================================

/// 可信代理配置设置
///
/// 用于安全地提取客户端真实 IP 地址。
/// 仅当请求来自可信代理时才信任 X-Forwarded-For 等请求头。
///
/// # 安全说明
///
/// 如果不配置可信代理，攻击者可以伪造 X-Forwarded-For 头来绕过
/// 基于 IP 的安全控制（如速率限制、访问控制等）。
///
/// # 配置示例
///
/// ```toml
/// [trusted_proxies]
/// enabled = true
/// proxies = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "127.0.0.1"]
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__TRUSTED_PROXIES__")]
pub struct TrustedProxySettings {
    /// 是否启用可信代理验证
    ///
    /// - true: 仅当请求来自可信代理时才信任转发头
    /// - false: 总是信任转发头（不安全，仅用于开发环境）
    #[config(default = true)]
    pub enabled: bool,

    /// 可信代理 IP 地址列表
    ///
    /// 支持 CIDR 格式（如 "10.0.0.0/8"）和单个 IP（如 "127.0.0.1"）
    ///
    /// 默认包含常见的私有 IP 地址范围：
    /// - 10.0.0.0/8 (Class A 私有网络)
    /// - 172.16.0.0/12 (Class B 私有网络)
    /// - 192.168.0.0/16 (Class C 私有网络)
    /// - 127.0.0.1 (本地回环)
    /// - ::1 (IPv6 本地回环)
    #[config(default = vec![
        "10.0.0.0/8".to_string(),
        "172.16.0.0/12".to_string(),
        "192.168.0.0/16".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ])]
    pub proxies: Vec<String>,
}

impl TrustedProxySettings {
    /// 检查 IP 地址是否在可信代理列表中
    ///
    /// # 参数
    ///
    /// * `ip` - 要检查的 IP 地址
    ///
    /// # 返回值
    ///
    /// 如果 IP 在可信代理列表中返回 true，否则返回 false
    pub fn is_trusted(&self, ip: &std::net::IpAddr) -> bool {
        use std::net::IpAddr;
        use std::str::FromStr;

        for proxy in &self.proxies {
            // 尝试解析为 CIDR
            if let Ok(network) = ipnetwork::IpNetwork::from_str(proxy) {
                if network.contains(*ip) {
                    return true;
                }
            } else {
                // 尝试解析为单个 IP
                if let Ok(trusted_ip) = proxy.parse::<IpAddr>() {
                    if trusted_ip == *ip {
                        return true;
                    }
                }
            }
        }
        false
    }
}

// =============================================================================
// 自定义验证函数
// =============================================================================

/// 安全验证函数
///
/// 验证配置的安全性要求，包括 webhook secret、数据库密码、S3 凭据等
pub fn validate_security(settings: &Settings) -> Result<(), validator::ValidationError> {
    // 检查 webhook secret 是否为空
    if settings.webhook.secret().is_empty() {
        return Err(validator::ValidationError::new("webhook_secret_empty"));
    }

    // 检查 webhook secret 是否使用默认值
    let weak_secrets = [
        "your-webhook-secret",
        "your-secret-key",
        "secret",
        "webhook-secret",
        "change-me",
        "password",
    ];
    if weak_secrets.contains(&settings.webhook.secret()) {
        return Err(validator::ValidationError::new("webhook_secret_weak"));
    }

    // 检查 webhook secret 长度
    if settings.webhook.secret().len() < 32 {
        return Err(validator::ValidationError::new("webhook_secret_short"));
    }

    // 检查速率限制是否禁用
    if !settings.rate_limiting.enabled {
        return Err(validator::ValidationError::new("rate_limiting_disabled"));
    }

    // 检查数据库密码
    let weak_patterns = ["password=password", "password=postgres", "password=admin"];
    if weak_patterns
        .iter()
        .any(|p| settings.database.url().contains(p))
    {
        return Err(validator::ValidationError::new("database_password_weak"));
    }

    // 生产环境密码长度验证
    let env = std::env::var("APP_ENVIRONMENT")
        .or_else(|_| std::env::var("CRAWLRS_ENV"))
        .unwrap_or_else(|_| "development".to_string());
    let is_production = env.eq_ignore_ascii_case("production") || env.eq_ignore_ascii_case("prod");

    if is_production {
        let password_length = extract_password_length(settings.database.url());
        if password_length > 0 && password_length < 16 {
            return Err(validator::ValidationError::new(
                "database_password_short_production",
            ));
        }
    }

    // 检查 S3 配置
    if settings.storage.storage_type == "s3" {
        // 检查 S3 bucket
        if settings.storage.s3_bucket.is_none()
            || settings
                .storage
                .s3_bucket
                .as_ref()
                .is_some_and(|b| b.is_empty())
        {
            return Err(validator::ValidationError::new("s3_bucket_missing"));
        }

        // 检查 S3 access key
        if settings.storage.s3_access_key().is_none()
            || settings
                .storage
                .s3_access_key()
                .is_some_and(|s| s.is_empty())
        {
            return Err(validator::ValidationError::new("s3_access_key_missing"));
        }

        // 检查 S3 secret key 长度
        if settings
            .storage
            .s3_secret_key()
            .is_some_and(|s| s.len() < 32)
        {
            return Err(validator::ValidationError::new("s3_secret_key_short"));
        }
    }

    Ok(())
}

/// 值验证函数
///
/// 验证配置值的有效性
pub fn validate_values(settings: &Settings) -> Result<(), validator::ValidationError> {
    // 验证端口范围
    if settings.server.port == 0 {
        return Err(validator::ValidationError::new("invalid_port"));
    }

    // 验证 A/B 测试权重范围
    if settings.search.variant_b_weight < 0.0 || settings.search.variant_b_weight > 1.0 {
        return Err(validator::ValidationError::new("invalid_variant_b_weight"));
    }

    // 验证存储类型
    if settings.storage.storage_type != "local" && settings.storage.storage_type != "s3" {
        return Err(validator::ValidationError::new("invalid_storage_type"));
    }

    Ok(())
}

fn extract_password_length(url: &str) -> usize {
    if let Some(at_pos) = url.find('@') {
        if let Some(colon_pos) = url[..at_pos].find(':') {
            return at_pos - colon_pos - 1;
        }
    }
    0
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_structure() {
        let settings = Settings {
            server: ServerSettings::default(),
            database: DatabaseSettings::default(),
            redis: RedisSettings::default(),
            cors: CorsSettings::default(),
            rate_limiting: RateLimitingSettings::default(),
            concurrency: ConcurrencySettings::default(),
            storage: StorageSettings::default(),
            webhook: WebhookSettings::default(),
            bing_search: BingSearchSettings::default(),
            search: SearchSettings::default(),
            llm: LLMSettings::default(),
            proxy: ProxySettings::default(),
            engines: EngineSettings::default(),
            logging: LoggingSettings::default(),
            workers: WorkerSettings::default(),
            timeouts: TimeoutSettings::default(),
            cache: CacheSettings::default(),
            trusted_proxies: TrustedProxySettings::default(),
        };

        assert_eq!(settings.server.port, 8899);
        assert!(settings.rate_limiting.enabled);
        assert!(settings.trusted_proxies.enabled);
    }

    #[test]
    fn test_trusted_proxy_settings_default() {
        let settings = TrustedProxySettings::default();
        assert!(settings.enabled);
        assert!(!settings.proxies.is_empty());

        // 测试私有 IP 地址
        let ip: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        let ip: std::net::IpAddr = "172.16.0.1".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        let ip: std::net::IpAddr = "192.168.1.1".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        let ip: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        // 测试公网 IP
        let ip: std::net::IpAddr = "8.8.8.8".parse().unwrap();
        assert!(!settings.is_trusted(&ip));
    }

    #[test]
    fn test_trusted_proxy_settings_ipv6() {
        let settings = TrustedProxySettings::default();

        // 测试 IPv6 本地回环
        let ip: std::net::IpAddr = "::1".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        // 测试 IPv6 公网地址
        let ip: std::net::IpAddr = "2001:db8::1".parse().unwrap();
        assert!(!settings.is_trusted(&ip));
    }

    #[test]
    fn test_trusted_proxy_settings_custom_cidr() {
        let settings = TrustedProxySettings {
            enabled: true,
            proxies: vec!["203.0.113.0/24".to_string(), "198.51.100.1".to_string()],
        };

        // 测试 CIDR 范围内的 IP
        let ip: std::net::IpAddr = "203.0.113.100".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        // 测试单个 IP
        let ip: std::net::IpAddr = "198.51.100.1".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        // 测试不在范围内的 IP
        let ip: std::net::IpAddr = "203.0.114.1".parse().unwrap();
        assert!(!settings.is_trusted(&ip));
    }
}
