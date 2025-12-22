// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

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
/// use crawlrs::config::settings::Settings;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let settings = Settings::new()?;
///     println!("Server will run on {}:{}", settings.server.host, settings.server.port);
///     Ok(())
/// }
/// ```
#[derive(Debug, Deserialize)]
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
}

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
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Deserialize)]
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

/// 存储配置设置
///
/// 配置数据存储后端，支持本地文件系统和 S3 兼容存储
///
/// # 字段说明
///
/// * `storage_type` - 存储类型，支持 "local" 或 "s3"，默认 "local"
/// * `local_path` - 本地存储路径，当 storage_type="local" 时使用，默认 "./storage"
/// * `s3_region` - S3 区域，当 storage_type="s3" 时使用
/// * `s3_bucket` - S3 存储桶名称，当 storage_type="s3" 时使用
/// * `s3_access_key` - S3 访问密钥，当 storage_type="s3" 时使用
/// * `s3_secret_key` - S3 密钥，当 storage_type="s3" 时使用
/// * `s3_endpoint` - S3 端点（可选），用于 MinIO 等兼容服务
#[derive(Debug, Deserialize)]
pub struct StorageSettings {
    /// 存储类型 (local, s3)
    pub storage_type: String,
    /// 本地存储路径 (当 type=local 时使用)
    pub local_path: Option<String>,
    /// S3 区域
    pub s3_region: Option<String>,
    /// S3 存储桶名称
    pub s3_bucket: Option<String>,
    /// S3 访问密钥
    pub s3_access_key: Option<String>,
    /// S3 密钥
    pub s3_secret_key: Option<String>,
    /// S3 端点 (可选，用于 MinIO 等兼容服务)
    pub s3_endpoint: Option<String>,
}

/// Webhook配置设置
///
/// 配置 Webhook 功能的参数
///
/// # 字段说明
///
/// * `secret` - Webhook 签名密钥，用于验证请求真实性，默认 "your-secret-key"
#[derive(Debug, Deserialize)]
pub struct WebhookSettings {
    /// Webhook签名密钥
    pub secret: String,
}

/// Google Custom Search API 配置设置
///
/// 配置 Google Custom Search API 的参数
///
/// # 字段说明
///
/// * `api_key` - Google Search API 密钥
/// * `cx` - Google Custom Search Engine ID
#[derive(Debug, Deserialize)]
pub struct GoogleSearchSettings {
    /// Google Search API 密钥
    pub api_key: Option<String>,
    /// Google Custom Search Engine ID
    pub cx: Option<String>,
}

/// Bing Search API 配置设置
#[derive(Debug, Deserialize)]
pub struct BingSearchSettings {
    /// Bing Search API 密钥
    pub api_key: Option<String>,
}

/// 搜索配置设置
#[derive(Debug, Clone, Deserialize)]
pub struct SearchSettings {
    /// 是否启用 A/B 测试
    pub ab_test_enabled: bool,
    /// Variant B 的流量权重 (0.0 到 1.0)
    pub variant_b_weight: f64,
}

/// LLM 配置设置
///
/// 配置 LLM（大语言模型）服务的参数
///
/// # 字段说明
///
/// * `api_key` - LLM API 密钥
/// * `model` - 使用的模型名称，默认 "gpt-3.5-turbo"
/// * `api_base_url` - LLM API 基础 URL，默认 "https://api.openai.com/v1"
#[derive(Debug, Deserialize)]
pub struct LLMSettings {
    /// LLM API 密钥
    pub api_key: Option<String>,
    /// 使用的模型名称
    pub model: Option<String>,
    /// LLM API 基础 URL
    pub api_base_url: Option<String>,
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
    /// use crawlrs::config::settings::Settings;
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
        // 获取应用环境变量，默认为 "default"
        let env = std::env::var("APP_ENVIRONMENT").unwrap_or_else(|_| "default".to_string());

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
            // 6. 设置 Webhook 默认配置
            .set_default("webhook.secret", "your-secret-key")? // Webhook 签名密钥
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
            // 9. 加载配置文件（可选）
            .add_source(File::with_name("config/default").required(false)) // 加载默认配置
            .add_source(File::with_name(&format!("config/{}", env)).required(false)) // 加载环境特定配置
            // 10. 加载环境变量（最高优先级）
            .add_source(Environment::with_prefix("CRAWLRS").separator("__")); // 加载 CRAWLRS__ 前缀的环境变量

        // 构建并反序列化配置
        builder.build()?.try_deserialize()
    }
}
