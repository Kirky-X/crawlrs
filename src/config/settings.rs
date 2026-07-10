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
    use crate::common::test_support::ENV_MUTEX;

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

    // ========== WorkerCount tests ==========

    #[test]
    fn test_worker_count_fixed_resolve() {
        let count = WorkerCount::Fixed(8);
        assert_eq!(count.resolve(), 8);
    }

    #[test]
    fn test_worker_count_auto_resolve_positive() {
        let count = WorkerCount::Auto("auto".to_string());
        let resolved = count.resolve();
        assert!(
            resolved > 0,
            "Auto resolve should be positive, got {}",
            resolved
        );
    }

    #[test]
    fn test_worker_count_auto_case_insensitive() {
        let count = WorkerCount::Auto("AUTO".to_string());
        let resolved = count.resolve();
        assert!(resolved > 0, "AUTO should resolve to cpu-based value");
    }

    #[test]
    fn test_worker_count_auto_non_auto_string_falls_to_default() {
        let count = WorkerCount::Auto("not-auto".to_string());
        assert_eq!(count.resolve(), 5);
    }

    #[test]
    fn test_worker_count_default_is_auto() {
        let count = WorkerCount::default();
        assert!(matches!(count, WorkerCount::Auto(_)));
    }

    #[test]
    fn test_worker_count_clone() {
        let count = WorkerCount::Fixed(12);
        let cloned = count.clone();
        assert_eq!(cloned.resolve(), 12);
    }

    #[test]
    fn test_worker_count_serde_fixed() {
        let count = WorkerCount::Fixed(7);
        let json = serde_json::to_string(&count).expect("serialize");
        let back: WorkerCount = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.resolve(), 7);
    }

    #[test]
    fn test_worker_count_serde_auto() {
        let json = "\"auto\"";
        let count: WorkerCount = serde_json::from_str(json).expect("deserialize");
        assert!(matches!(count, WorkerCount::Auto(_)));
    }

    // ========== CorsSettings tests ==========

    #[test]
    fn test_cors_settings_default_allowed_origins() {
        let settings = CorsSettings::default();
        assert_eq!(settings.allowed_origins, "*");
    }

    #[test]
    fn test_cors_settings_construction() {
        let settings = CorsSettings {
            allowed_origins: "https://example.com".to_string(),
        };
        assert_eq!(settings.allowed_origins, "https://example.com");
    }

    #[test]
    fn test_cors_settings_clone() {
        let settings = CorsSettings {
            allowed_origins: "https://test.com,https://api.com".to_string(),
        };
        let cloned = settings.clone();
        assert_eq!(cloned.allowed_origins, settings.allowed_origins);
    }

    #[test]
    fn test_cors_settings_serde_roundtrip() {
        let settings = CorsSettings {
            allowed_origins: "https://roundtrip.com".to_string(),
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: CorsSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.allowed_origins, "https://roundtrip.com");
    }

    // ========== ProxySettings tests ==========

    #[test]
    fn test_proxy_settings_default_url() {
        let settings = ProxySettings::default();
        assert_eq!(settings.url(), "http://localhost:10808");
        assert!(!settings.enabled);
    }

    #[test]
    fn test_proxy_settings_url_accessor() {
        let settings = ProxySettings {
            url: "http://proxy.example.com:8080".to_string(),
            enabled: true,
        };
        assert_eq!(settings.url(), "http://proxy.example.com:8080");
        assert!(settings.enabled);
    }

    #[test]
    fn test_proxy_settings_debug_redacts_url() {
        let settings = ProxySettings {
            url: "http://secret:password@proxy:8080".to_string(),
            enabled: true,
        };
        let debug_str = format!("{:?}", settings);
        assert!(debug_str.contains("***REDACTED***"));
        assert!(!debug_str.contains("secret:password"));
        assert!(debug_str.contains("true"));
    }

    #[test]
    fn test_proxy_settings_clone_preserves_fields() {
        let settings = ProxySettings {
            url: "http://clone-proxy:9090".to_string(),
            enabled: true,
        };
        let cloned = settings.clone();
        assert_eq!(cloned.url(), "http://clone-proxy:9090");
        assert!(cloned.enabled);
    }

    #[test]
    fn test_proxy_settings_serde_roundtrip() {
        let settings = ProxySettings {
            url: "http://serde-proxy:7070".to_string(),
            enabled: false,
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: ProxySettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.url(), "http://serde-proxy:7070");
        assert!(!back.enabled);
    }

    // ========== WorkerSettings tests ==========

    #[test]
    fn test_worker_settings_default_uses_auto() {
        let settings = WorkerSettings::default();
        assert!(matches!(settings.count, WorkerCount::Auto(_)));
    }

    #[test]
    fn test_worker_settings_construction_fixed() {
        let settings = WorkerSettings {
            count: WorkerCount::Fixed(16),
        };
        assert_eq!(settings.count.resolve(), 16);
    }

    // ========== TimeoutSettings tests ==========

    #[test]
    fn test_timeout_settings_default() {
        let settings = TimeoutSettings::default();
        assert_eq!(settings.workers.webhook_interval_seconds, 5);
        assert_eq!(settings.workers.backlog_interval_seconds, 30);
        assert_eq!(settings.engines.default_timeout_seconds, 30);
        assert_eq!(settings.engines.playwright_timeout_seconds, 30);
        assert_eq!(settings.engines.flaresolverr_timeout_seconds, 30);
        assert_eq!(settings.retry.initial_backoff_seconds, 1);
        assert_eq!(settings.retry.max_backoff_seconds, 60);
        assert_eq!(settings.cache.default_ttl_seconds, 600);
        assert_eq!(settings.cache.memory_ttl_seconds, 600);
        assert_eq!(settings.cache.redis_ttl_seconds, 7200);
    }

    #[test]
    fn test_worker_timeout_settings_construction() {
        let settings = WorkerTimeoutSettings {
            webhook_interval_seconds: 10,
            backlog_interval_seconds: 60,
        };
        assert_eq!(settings.webhook_interval_seconds, 10);
        assert_eq!(settings.backlog_interval_seconds, 60);
    }

    #[test]
    fn test_engine_timeout_settings_construction() {
        let settings = EngineTimeoutSettings {
            default_timeout_seconds: 45,
            playwright_timeout_seconds: 50,
            flaresolverr_timeout_seconds: 55,
        };
        assert_eq!(settings.default_timeout_seconds, 45);
        assert_eq!(settings.playwright_timeout_seconds, 50);
        assert_eq!(settings.flaresolverr_timeout_seconds, 55);
    }

    #[test]
    fn test_retry_timeout_settings_construction() {
        let settings = RetryTimeoutSettings {
            initial_backoff_seconds: 2,
            max_backoff_seconds: 120,
        };
        assert_eq!(settings.initial_backoff_seconds, 2);
        assert_eq!(settings.max_backoff_seconds, 120);
    }

    #[test]
    fn test_cache_timeout_settings_construction() {
        let settings = CacheTimeoutSettings {
            default_ttl_seconds: 300,
            memory_ttl_seconds: 200,
            redis_ttl_seconds: 3600,
        };
        assert_eq!(settings.default_ttl_seconds, 300);
        assert_eq!(settings.memory_ttl_seconds, 200);
        assert_eq!(settings.redis_ttl_seconds, 3600);
    }

    // ========== CacheSettings tests ==========

    #[test]
    fn test_cache_settings_default() {
        let settings = CacheSettings::default();
        assert!(settings.enabled);
        assert_eq!(settings.memory.capacity, 10000);
        assert_eq!(settings.memory.ttl_seconds, 300);
        assert!(!settings.redis.enabled);
        assert_eq!(settings.redis.url, "redis://localhost:6379");
        assert_eq!(settings.redis.pool_size, 10);
        assert_eq!(settings.redis.ttl_seconds, 3600);
    }

    #[test]
    fn test_memory_cache_settings_construction() {
        let settings = MemoryCacheSettings {
            capacity: 5000,
            ttl_seconds: 120,
        };
        assert_eq!(settings.capacity, 5000);
        assert_eq!(settings.ttl_seconds, 120);
    }

    #[test]
    fn test_redis_cache_settings_construction() {
        let settings = RedisCacheSettings {
            enabled: true,
            url: "redis://prod:6379".to_string(),
            pool_size: 20,
            ttl_seconds: 7200,
        };
        assert!(settings.enabled);
        assert_eq!(settings.url, "redis://prod:6379");
        assert_eq!(settings.pool_size, 20);
        assert_eq!(settings.ttl_seconds, 7200);
    }

    #[test]
    fn test_cache_type_settings_construction() {
        let settings = CacheTypeSettings {
            ttl_seconds: 100,
            max_size: 500,
        };
        assert_eq!(settings.ttl_seconds, 100);
        assert_eq!(settings.max_size, 500);
    }

    #[test]
    fn test_cache_settings_serde_roundtrip() {
        let settings = CacheSettings {
            enabled: false,
            memory: MemoryCacheSettings {
                capacity: 100,
                ttl_seconds: 50,
            },
            redis: RedisCacheSettings {
                enabled: true,
                url: "redis://x:1".to_string(),
                pool_size: 5,
                ttl_seconds: 99,
            },
            types: CacheTypeSpecificSettings {
                search: CacheTypeSettings {
                    ttl_seconds: 10,
                    max_size: 20,
                },
                dns: CacheTypeSettings {
                    ttl_seconds: 30,
                    max_size: 40,
                },
                regex: CacheTypeSettings {
                    ttl_seconds: 60,
                    max_size: 70,
                },
            },
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: CacheSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(!back.enabled);
        assert_eq!(back.memory.capacity, 100);
        assert!(back.redis.enabled);
        assert_eq!(back.types.search.ttl_seconds, 10);
        assert_eq!(back.types.dns.max_size, 40);
        assert_eq!(back.types.regex.ttl_seconds, 60);
    }

    // ========== extract_password_length tests ==========

    #[test]
    fn test_extract_password_length_with_password() {
        // 函数计算 @ 位置与第一个 : 位置（协议冒号）的差值
        let url = "postgresql://user:mypassword@localhost/db";
        let len = extract_password_length(url);
        // at_pos = 28, colon_pos = 10 (protocol colon), return = 28 - 10 - 1 = 17
        assert_eq!(len, 17);
    }

    #[test]
    fn test_extract_password_length_no_password() {
        let len = extract_password_length("postgresql://localhost/db");
        assert_eq!(len, 0);
    }

    #[test]
    fn test_extract_password_length_no_at_sign() {
        let len = extract_password_length("postgresql://user:pass");
        assert_eq!(len, 0);
    }

    #[test]
    fn test_extract_password_length_empty_password() {
        // user: 后立即跟 @，但函数仍从协议冒号开始计算
        let url = "postgresql://user:@localhost/db";
        let len = extract_password_length(url);
        // at_pos = 18, colon_pos = 10, return = 18 - 10 - 1 = 7
        assert_eq!(len, 7);
    }

    // ========== validate_values tests ==========

    fn build_test_settings() -> Settings {
        Settings {
            server: ServerSettings::default(),
            database: DatabaseSettings::default(),
            redis: RedisSettings::default(),
            cors: CorsSettings::default(),
            rate_limiting: RateLimitingSettings::default(),
            concurrency: ConcurrencySettings::default(),
            storage: StorageSettings::default(),
            webhook: WebhookSettings {
                secret: "a-very-strong-and-secure-webhook-secret-key-32+chars".to_string(),
                max_retries: 5,
                batch_size: 1000,
            },
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
        }
    }

    #[test]
    fn test_validate_values_valid_settings() {
        let settings = build_test_settings();
        assert!(validate_values(&settings).is_ok());
    }

    #[test]
    fn test_validate_values_invalid_port_zero() {
        let mut settings = build_test_settings();
        settings.server.port = 0;
        let result = validate_values(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "invalid_port");
    }

    #[test]
    fn test_validate_values_invalid_variant_b_weight_negative() {
        let mut settings = build_test_settings();
        settings.search.variant_b_weight = -0.1;
        let result = validate_values(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "invalid_variant_b_weight"
        );
    }

    #[test]
    fn test_validate_values_invalid_variant_b_weight_above_one() {
        let mut settings = build_test_settings();
        settings.search.variant_b_weight = 1.5;
        let result = validate_values(&settings);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_values_invalid_storage_type() {
        let mut settings = build_test_settings();
        settings.storage.storage_type = "invalid".to_string();
        let result = validate_values(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "invalid_storage_type");
    }

    #[test]
    fn test_validate_values_storage_type_s3_valid() {
        let mut settings = build_test_settings();
        settings.storage.storage_type = "s3".to_string();
        assert!(validate_values(&settings).is_ok());
    }

    // ========== validate_security tests (serialized via mutex due to env var) ==========

    // Use the shared global ENV_MUTEX to prevent cross-module env var race conditions
    // (config::settings, utils::errors, and common::error all manipulate CRAWLRS_ENV)

    #[test]
    fn test_validate_security_valid() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let settings = build_test_settings();
        assert!(validate_security(&settings).is_ok());
    }

    #[test]
    fn test_validate_security_empty_webhook_secret() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        settings.webhook = WebhookSettings {
            secret: String::new(),
            max_retries: 5,
            batch_size: 1000,
        };
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "webhook_secret_empty");
    }

    #[test]
    fn test_validate_security_weak_webhook_secret() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        // 使用弱密钥列表中的精确值（validate_security 在长度检查之前检查弱密钥列表）
        settings.webhook = WebhookSettings {
            secret: "your-webhook-secret".to_string(),
            max_retries: 5,
            batch_size: 1000,
        };
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "webhook_secret_weak");
    }

    #[test]
    fn test_validate_security_short_webhook_secret() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        settings.webhook = WebhookSettings {
            secret: "short".to_string(),
            max_retries: 5,
            batch_size: 1000,
        };
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "webhook_secret_short");
    }

    #[test]
    fn test_validate_security_rate_limiting_disabled() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        settings.rate_limiting.enabled = false;
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "rate_limiting_disabled"
        );
    }

    #[test]
    fn test_validate_security_weak_database_password() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        // validate_security 检查 URL 中是否包含 "password=password" 等弱密码模式
        settings.database = DatabaseSettings {
            url: "postgresql://user:name@localhost/db?password=password".to_string(),
            max_connections: Some(100),
            min_connections: Some(10),
            connect_timeout: Some(10),
            idle_timeout: Some(300),
            max_lifetime: Some(1800),
            connection_keepalive: Some(30),
            health_check_interval: Some(60),
        };
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "database_password_weak"
        );
    }

    #[test]
    fn test_validate_security_s3_missing_bucket() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        settings.storage = StorageSettings::s3("us-east-1", "", None, None, None);
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "s3_bucket_missing");
    }

    #[test]
    fn test_validate_security_s3_missing_access_key() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        settings.storage = StorageSettings::s3("us-east-1", "my-bucket", None, None, None);
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "s3_access_key_missing"
        );
    }

    #[test]
    fn test_validate_security_s3_short_secret_key() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        settings.storage = StorageSettings::s3(
            "us-east-1",
            "my-bucket",
            Some("access-key".to_string()),
            Some("short-secret".to_string()),
            None,
        );
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "s3_secret_key_short");
    }

    #[test]
    fn test_validate_security_s3_valid_config() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        settings.storage = StorageSettings::s3(
            "us-east-1",
            "valid-bucket",
            Some("access-key".to_string()),
            Some("this-is-a-valid-secret-key-32-chars!".to_string()),
            None,
        );
        assert!(validate_security(&settings).is_ok());
    }
}
