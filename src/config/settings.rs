// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

/// 应用程序配置设置
///
/// 包含数据库、Redis、服务器、速率限制和并发控制等所有配置项
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
}

/// 数据库配置设置
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
#[derive(Debug, Deserialize)]
pub struct RedisSettings {
    /// Redis连接URL
    pub url: String,
}

/// 服务器配置设置
#[derive(Debug, Deserialize)]
pub struct ServerSettings {
    /// 服务器监听主机地址
    pub host: String,
    /// 服务器监听端口
    pub port: u16,
}

/// 速率限制配置设置
#[derive(Debug, Deserialize)]
pub struct RateLimitingSettings {
    /// 是否启用速率限制
    pub enabled: bool,
    /// 默认每分钟请求数限制
    pub default_rpm: u32,
}

/// 并发控制配置设置
#[derive(Debug, Deserialize)]
pub struct ConcurrencySettings {
    /// 默认团队并发限制
    pub default_team_limit: i64,
}

/// 存储配置设置
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
#[derive(Debug, Deserialize)]
pub struct WebhookSettings {
    /// Webhook签名密钥
    pub secret: String,
}

impl Settings {
    /// 创建新的配置实例
    ///
    /// 从环境变量加载配置，支持默认值
    ///
    /// # Returns
    ///
    /// * `Ok(Settings)` - 成功加载的配置
    /// * `Err(ConfigError)` - 配置加载失败
    pub fn new() -> Result<Self, ConfigError> {
        let env = std::env::var("APP_ENVIRONMENT").unwrap_or_else(|_| "default".to_string());
        let builder = Config::builder()
            // Start with default settings
            .set_default("server.host", "0.0.0.0")?
            .set_default("server.port", 3000)?
            // Default DB pool settings
            .set_default("database.max_connections", 100)?
            .set_default("database.min_connections", 10)?
            .set_default("database.connect_timeout", 10)?
            .set_default("database.idle_timeout", 300)?
            // Default Storage settings
            .set_default("storage.storage_type", "local")?
            .set_default("storage.local_path", "./storage")?
            // Default Rate Limiting settings
            .set_default("rate_limiting.enabled", true)?
            .set_default("rate_limiting.default_rpm", 100)?
            // Default Concurrency settings
            .set_default("concurrency.default_team_limit", 10)?
            // Default Webhook settings
            .set_default("webhook.secret", "your-secret-key")?
            .add_source(File::with_name("config/default").required(false))
            .add_source(File::with_name(&format!("config/{}", env)).required(false))
            .add_source(Environment::with_prefix("CRAWLRS").separator("__"));

        builder.build()?.try_deserialize()
    }
}
