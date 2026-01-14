// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;
use thiserror::Error;

use super::app::{
    ConcurrencySettings, DatabaseSettings, ProxySettings, RateLimitingSettings, RedisSettings,
    ServerSettings,
};
use super::llm::LLMSettings;
use super::search::{BingSearchSettings, GoogleSearchSettings, SearchSettings};
use super::storage::{StorageSettings, WebhookSettings};

/// 配置安全错误
#[derive(Error, Debug)]
pub enum ConfigSecurityError {
    #[error("CRITICAL: Webhook secret cannot be empty in production")]
    EmptyWebhookSecret,

    #[error("CRITICAL: Webhook secret uses weak default value '{0}'. Must be changed to a strong random secret!")]
    WeakWebhookSecret(String),

    #[error("CRITICAL: Webhook secret is too short ({0} bytes). Minimum 32 bytes required.")]
    ShortWebhookSecret(usize),

    #[error("CRITICAL: Database URL uses weak/default password. Use strong authentication in production!")]
    WeakDatabasePassword,

    #[error("SECURITY WARNING: S3 access key is not configured but storage type is 's3'")]
    MissingS3AccessKey,

    #[error(
        "WARNING: S3 secret key appears to be short (< 32 characters). Use a strong secret key."
    )]
    ShortS3SecretKey,

    #[error("WARNING: Rate limiting is disabled. This may expose the service to abuse.")]
    RateLimitingDisabled,
}

/// 应用程序配置设置
///
/// 包含数据库、Redis、服务器、速率限制和并发控制等所有配置项
///
/// # 字段说明
///
/// * `database` - 数据库连接和连接池配置
/// * `redis` - Redis 连接配置，用于缓存和速率限制
/// * `server` - HTTP 服务器监听地址和端口配置
/// * `rate_limiting` - API 速率限制配置
/// * `concurrency` - 并发控制和资源限制配置
/// * `storage` - 数据存储配置（本地文件系统或 S3）
/// * `webhook` - Webhook 功能配置
///
/// # 示例
///
/// ```rust
/// use crawlrs::config::Settings;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let settings = Settings::new()?;
///     println!("Server will run on {}:{}", settings.server.host, settings.server.port);
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    /// 数据库配置
    pub database: DatabaseSettings,
    /// Redis配置
    pub redis: RedisSettings,
    /// 服务器配置
    pub server: ServerSettings,
    /// 速率限制配置
    pub rate_limiting: RateLimitingSettings,
    /// 并发控制配置
    pub concurrency: ConcurrencySettings,
    /// 存储配置
    pub storage: StorageSettings,
    /// Webhook 配置
    pub webhook: WebhookSettings,
    /// Google Custom Search API 配置
    pub google_search: GoogleSearchSettings,
    /// Bing Search API 配置
    pub bing_search: BingSearchSettings,
    /// 搜索配置 (包含 A/B 测试)
    pub search: SearchSettings,
    /// LLM 配置
    pub llm: LLMSettings,
    /// HTTP 代理配置
    pub proxy: ProxySettings,
}

impl Settings {
    /// 创建新的配置实例
    ///
    /// 从环境变量加载配置，支持默认值。配置加载顺序：
    /// 1. 设置默认值
    /// 2. 加载 `config/default` 文件（可选）
    /// 3. 加载 `config/{APP_ENVIRONMENT}` 文件（可选）
    /// 4. 加载以 `CRAWLRS__` 为前缀的环境变量
    ///
    /// # 参数
    ///
    /// 无
    ///
    /// # 返回值
    ///
    /// * `Ok(Settings)` - 成功加载的配置
    /// * `Err(ConfigError)` - 配置加载失败
    ///
    /// # 示例
    ///
    /// ```rust
    /// use crawlrs::config::Settings;
    ///
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let settings = Settings::new()?;
    ///     println!("Database URL: {}", settings.database.url);
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # 错误
    ///
    /// 可能在以下情况下返回错误：
    /// - 配置文件格式错误
    /// - 环境变量解析失败
    /// - 必需的配置项缺失
    ///
    /// # Panics
    ///
    /// 此函数不会 panic
    pub fn new() -> Result<Self, ConfigError> {
        /// 常量定义 - 应用程序环境
        const APP_ENVIRONMENT_DEFAULT: &str = "default";

        // 获取应用环境变量，默认为 "default"
        let env = std::env::var("APP_ENVIRONMENT")
            .unwrap_or_else(|_| APP_ENVIRONMENT_DEFAULT.to_string());

        // 构建配置构建器，按优先级顺序加载配置
        let builder = Config::builder()
            // 1. 设置服务器默认配置
            .set_default("server.host", "0.0.0.0")? // 默认监听所有网络接口
            .set_default("server.port", 8899)? // 默认端口 8899
            .set_default("server.enable_port_detection", true)? // 默认启用端口嗅探
            // 2. 设置数据库连接池默认配置
            .set_default("database.max_connections", 100)? // 最大连接数 100
            .set_default("database.min_connections", 10)? // 最小连接数 10
            .set_default("database.connect_timeout", 10)? // 连接超时 10 秒
            .set_default("database.idle_timeout", 300)? // 空闲超时 300 秒（5 分钟）
            // 3. 设置存储默认配置
            .set_default("storage.storage_type", "local")? // 默认使用本地存储
            .set_default("storage.local_path", "./storage")? // 本地存储路径
            // 4. 设置速率限制默认配置
            .set_default("rate_limiting.enabled", true)? // 默认启用速率限制
            .set_default("rate_limiting.default_rpm", 100)? // 默认每分钟 100 请求
            // 5. 设置并发控制默认配置
            .set_default("concurrency.default_team_limit", 10)? // 默认团队并发限制 10
            .set_default("concurrency.task_lock_duration_seconds", 300)? // 任务锁持续 300 秒（5 分钟）
            // 7. 设置搜索 A/B 测试默认配置
            .set_default("search.ab_test_enabled", false)? // 默认关闭 A/B 测试
            .set_default("search.variant_b_weight", 0.1)? // 默认分配 10% 流量到 B 变体
            // 8. 设置 Google Custom Search API 默认配置
            .set_default("google_search.api_key", "")? // Google Search API 密钥
            .set_default("google_search.cx", "")? // Google Custom Search Engine ID
            // 8. 设置 Bing Search API 默认配置
            .set_default("bing_search.api_key", "")?
            // 9. 设置 LLM 默认配置
            .set_default("llm.api_key", "")? // LLM API 密钥
            .set_default("llm.model", "gpt-3.5-turbo")? // 默认模型
            .set_default("llm.api_base_url", "https://api.openai.com/v1")? // 默认 API 基础 URL
            // 10. 设置代理默认配置
            .set_default("proxy.url", "http://localhost:10808")? // 默认代理地址
            .set_default("proxy.enabled", false)? // 默认禁用代理
            // 11. 加载配置文件（可选）
            .add_source(File::with_name("config/default").required(false)) // 加载默认配置
            .add_source(File::with_name(&format!("config/{}", env)).required(false)) // 加载环境特定配置
            // 10. 加载环境变量（最高优先级）
            .add_source(Environment::with_prefix("CRAWLRS").separator("__")); // 加载 CRAWLRS__ 前缀的环境变量

        // 构建并反序列化配置
        let settings: Settings = builder.build()?.try_deserialize()?;

        // 验证配置值
        settings.validate()?;

        Ok(settings)
    }

    /// 验证配置安全性
    ///
    /// 检查是否存在弱默认配置，并返回错误（会阻止启动）
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 配置安全
    /// * `Err(ConfigSecurityError)` - 发现安全风险
    pub fn validate_security(&self) -> Result<(), ConfigSecurityError> {
        // 检查 webhook secret 是否使用默认值
        let weak_secrets = [
            "your-webhook-secret",
            "your-secret-key",
            "secret",
            "webhook-secret",
            "change-me",
            "password",
        ];

        if self.webhook.secret.is_empty() {
            return Err(ConfigSecurityError::EmptyWebhookSecret);
        } else if weak_secrets.contains(&self.webhook.secret.as_str()) {
            return Err(ConfigSecurityError::WeakWebhookSecret(
                self.webhook.secret.clone(),
            ));
        } else if self.webhook.secret.len() < 32 {
            return Err(ConfigSecurityError::ShortWebhookSecret(
                self.webhook.secret.len(),
            ));
        }

        // 检查 S3 凭据安全性
        if self.storage.storage_type == "s3" {
            if self
                .storage
                .s3_access_key
                .as_ref()
                .map(|s| s.is_empty())
                .unwrap_or(true)
            {
                return Err(ConfigSecurityError::MissingS3AccessKey);
            }

            if self
                .storage
                .s3_secret_key
                .as_ref()
                .map(|s| s.len() < 32)
                .unwrap_or(false)
            {
                return Err(ConfigSecurityError::ShortS3SecretKey);
            }
        }

        // 检查速率限制是否禁用
        if !self.rate_limiting.enabled {
            return Err(ConfigSecurityError::RateLimitingDisabled);
        }

        if self.database.url.contains("password=password")
            || self.database.url.contains("password=postgres")
            || self.database.url.contains("password=admin")
        {
            return Err(ConfigSecurityError::WeakDatabasePassword);
        }

        Ok(())
    }

    /// 验证配置值有效性
    ///
    /// 检查配置值是否在合理范围内
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 配置有效
    /// * `Err(ConfigError)` - 配置无效
    pub fn validate(&self) -> Result<(), ConfigError> {
        // 验证端口范围
        if self.server.port == 0 {
            return Err(ConfigError::Message(ERROR_INVALID_PORT.to_string()));
        }

        // 验证 A/B 测试权重范围
        if self.search.variant_b_weight < 0.0 || self.search.variant_b_weight > 1.0 {
            return Err(ConfigError::Message(
                ERROR_INVALID_VARIANT_WEIGHT.to_string(),
            ));
        }

        // 验证数据库连接池配置
        if let Some(max_conn) = self.database.max_connections {
            if max_conn == 0 {
                return Err(ConfigError::Message(
                    ERROR_INVALID_MAX_CONNECTIONS.to_string(),
                ));
            }
        }

        if let Some(min_conn) = self.database.min_connections {
            if min_conn == 0 {
                return Err(ConfigError::Message(
                    ERROR_INVALID_MIN_CONNECTIONS.to_string(),
                ));
            }
        }

        /// 常量定义 - 配置验证消息
        const ERROR_INVALID_PORT: &str = "Invalid port number: port must be between 1 and 65535";
        const ERROR_INVALID_VARIANT_WEIGHT: &str =
            "Invalid variant_b_weight: must be between 0.0 and 1.0";
        const ERROR_INVALID_MAX_CONNECTIONS: &str =
            "Invalid max_connections: must be greater than 0";
        const ERROR_INVALID_MIN_CONNECTIONS: &str =
            "Invalid min_connections: must be greater than 0";
        const ERROR_INVALID_STORAGE_TYPE: &str = "Invalid storage_type: must be 'local' or 's3'";
        const ERROR_MISSING_S3_BUCKET: &str =
            "S3 bucket must be configured when storage_type is 's3'";

        // 验证存储类型
        const STORAGE_TYPE_LOCAL: &str = "local";
        const STORAGE_TYPE_S3: &str = "s3";

        if self.storage.storage_type != STORAGE_TYPE_LOCAL
            && self.storage.storage_type != STORAGE_TYPE_S3
        {
            return Err(ConfigError::Message(ERROR_INVALID_STORAGE_TYPE.to_string()));
        }

        // 验证 S3 配置完整性
        if self.storage.storage_type == STORAGE_TYPE_S3
            && (self.storage.s3_bucket.is_none()
                || self
                    .storage
                    .s3_bucket
                    .as_ref()
                    .is_some_and(|b| b.is_empty()))
        {
            return Err(ConfigError::Message(ERROR_MISSING_S3_BUCKET.to_string()));
        }

        Ok(())
    }
}
