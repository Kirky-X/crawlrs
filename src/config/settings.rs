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

use confers::{Config, ConfigBuilder, Environment, File};
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
// 错误类型
// =============================================================================

/// 配置错误类型
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("File error: {0}")]
    File(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("CRITICAL: Webhook secret cannot be empty in production")]
    EmptyWebhookSecret,

    #[error("CRITICAL: Webhook secret uses weak default value. Must be changed!")]
    WeakWebhookSecret,

    #[error("CRITICAL: Webhook secret is too short ({0} bytes). Minimum 32 bytes required.")]
    ShortWebhookSecret(usize),

    #[error("CRITICAL: Database URL uses weak/default password")]
    WeakDatabasePassword,

    #[error("CRITICAL: Database password is too short ({0} bytes). Minimum 16 bytes required in production!")]
    ShortDatabasePassword(usize),

    #[error("S3 bucket must be configured when storage_type is 's3'")]
    MissingS3Bucket,

    #[error("SECURITY WARNING: S3 access key is not configured but storage type is 's3'")]
    MissingS3AccessKey,

    #[error("SECURITY WARNING: S3 secret key appears to be short (< 32 characters)")]
    ShortS3SecretKey,

    #[error("Invalid port number: port must be between 1 and 65535")]
    InvalidPort,

    #[error("Invalid variant_b_weight: must be between 0.0 and 1.0")]
    InvalidVariantWeight,

    #[error("Invalid storage_type: must be 'local' or 's3'")]
    InvalidStorageType,

    #[error("Rate limiting is disabled. This may expose the service to abuse.")]
    RateLimitingDisabled,
}

impl From<confers::ConfigError> for ConfigError {
    fn from(e: confers::ConfigError) -> Self {
        ConfigError::Parse(e.to_string())
    }
}

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
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__", validate)]
pub struct Settings {
    /// 服务器配置
    #[config(default)]
    pub server: ServerSettings,

    /// 数据库配置
    #[config(default)]
    pub database: DatabaseSettings,

    /// Redis 配置
    #[config(default)]
    pub redis: RedisSettings,

    /// CORS 配置
    #[config(default)]
    pub cors: CorsSettings,

    /// 速率限制配置
    #[config(default)]
    pub rate_limiting: RateLimitingSettings,

    /// 并发控制配置
    #[config(default)]
    pub concurrency: ConcurrencySettings,

    /// 存储配置
    #[config(default)]
    pub storage: StorageSettings,

    /// Webhook 配置
    #[config(default)]
    pub webhook: WebhookSettings,

    /// Bing Search API 配置
    #[config(default)]
    pub bing_search: BingSearchSettings,

    /// 搜索配置 (包含 A/B 测试)
    #[config(default)]
    pub search: SearchSettings,

    /// LLM 配置
    #[config(default)]
    pub llm: LLMSettings,

    /// HTTP 代理配置
    #[config(default)]
    pub proxy: ProxySettings,

    /// 引擎配置
    #[config(default)]
    pub engines: EngineSettings,

    /// 日志配置
    #[config(default)]
    pub logging: LoggingSettings,

    /// Worker 配置
    #[config(default)]
    pub workers: WorkerSettings,

    /// 超时配置
    #[config(default)]
    pub timeouts: TimeoutSettings,
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
    #[config(default = "*")]
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
    #[config(default = "http://localhost:10808")]
    pub(crate) url: String,

    /// 是否启用代理
    #[config(default = false)]
    pub enabled: bool,
}

impl ProxySettings {
    /// 获取代理服务器URL
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
    #[config(default)]
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
    #[config(default)]
    pub workers: WorkerTimeoutSettings,

    /// 引擎相关超时
    #[config(default)]
    pub engines: EngineTimeoutSettings,

    /// 重试策略超时
    #[config(default)]
    pub retry: RetryTimeoutSettings,

    /// 缓存TTL设置
    #[config(default)]
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
// 配置加载 API
// =============================================================================

impl Settings {
    /// 创建新的配置实例（同步）
    ///
    /// 从配置文件加载配置，并应用环境变量覆盖。
    /// 支持的配置加载顺序：
    /// 1. 配置文件 (config/default.toml)
    /// 2. 环境变量 (CRAWLRS__ 前缀)
    /// 3. 默认值
    ///
    /// # 返回值
    ///
    /// * `Ok(Settings)` - 成功加载的配置
    /// * `Err(ConfigError)` - 配置加载或验证失败
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let settings = Settings::new()?;
    /// ```
    pub fn new() -> Result<Self, ConfigError> {
        // 使用 ConfigBuilder 加载配置
        let settings: Settings = ConfigBuilder::new()
            .add_source(File::with_name("config/default.toml").required(false))
            .add_source(Environment::with_prefix("CRAWLRS").separator("__"))
            .build()?;

        // 运行自定义验证
        settings.validate_security()?;
        settings.validate()?;

        Ok(settings)
    }

    /// 从指定路径加载配置
    ///
    /// # 参数
    ///
    /// * `path` - 配置文件路径
    pub fn load_with_path(path: &str) -> Result<Self, ConfigError> {
        let settings: Settings = ConfigBuilder::new()
            .add_source(File::with_name(path).required(false))
            .add_source(Environment::with_prefix("CRAWLRS").separator("__"))
            .build()?;

        settings.validate_security()?;
        settings.validate()?;

        Ok(settings)
    }

    /// 验证配置安全性（公共方法）
    pub fn validate_security(&self) -> Result<(), ConfigError> {
        // 检查 webhook secret 是否使用默认值
        let weak_secrets = [
            "your-webhook-secret",
            "your-secret-key",
            "secret",
            "webhook-secret",
            "change-me",
            "password",
        ];

        if self.webhook.secret().is_empty() {
            return Err(ConfigError::EmptyWebhookSecret);
        } else if weak_secrets.contains(&self.webhook.secret()) {
            return Err(ConfigError::WeakWebhookSecret);
        } else if self.webhook.secret().len() < 32 {
            return Err(ConfigError::ShortWebhookSecret(self.webhook.secret().len()));
        }

        // 检查 S3 凭据安全性
        if self.storage.storage_type == "s3" {
            if self.storage.s3_access_key().is_none()
                || self.storage.s3_access_key().is_some_and(|s| s.is_empty())
            {
                return Err(ConfigError::MissingS3AccessKey);
            }

            if self.storage.s3_secret_key().is_some_and(|s| s.len() < 32) {
                return Err(ConfigError::ShortS3SecretKey);
            }
        }

        // 检查速率限制是否禁用
        if !self.rate_limiting.enabled {
            return Err(ConfigError::RateLimitingDisabled);
        }

        // 检查数据库密码
        let weak_patterns = ["password=password", "password=postgres", "password=admin"];
        if weak_patterns
            .iter()
            .any(|p| self.database.url().contains(p))
        {
            return Err(ConfigError::WeakDatabasePassword);
        }

        // 生产环境密码长度验证
        let env = std::env::var("APP_ENVIRONMENT")
            .or_else(|_| std::env::var("CRAWLRS_ENV"))
            .unwrap_or_else(|_| "development".to_string());
        let is_production =
            env.eq_ignore_ascii_case("production") || env.eq_ignore_ascii_case("prod");

        if is_production {
            let password_length = Self::extract_password_length(self.database.url());
            if password_length > 0 && password_length < 16 {
                return Err(ConfigError::ShortDatabasePassword(password_length));
            }
        }

        Ok(())
    }

    /// 验证配置值有效性（公共方法）
    pub fn validate(&self) -> Result<(), ConfigError> {
        // 验证端口范围
        if self.server.port == 0 {
            return Err(ConfigError::InvalidPort);
        }

        // 验证 A/B 测试权重范围
        if self.search.variant_b_weight < 0.0 || self.search.variant_b_weight > 1.0 {
            return Err(ConfigError::InvalidVariantWeight);
        }

        // 验证存储类型
        if self.storage.storage_type != "local" && self.storage.storage_type != "s3" {
            return Err(ConfigError::InvalidStorageType);
        }

        // 验证 S3 配置完整性
        if self.storage.storage_type == "s3"
            && (self.storage.s3_bucket.is_none()
                || self
                    .storage
                    .s3_bucket
                    .as_ref()
                    .is_some_and(|b| b.is_empty()))
        {
            return Err(ConfigError::MissingS3Bucket);
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
}
