// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 存储和安全配置
//!
//! 包含存储后端配置和 Webhook 安全配置

use confers::Config;
use serde::{Deserialize, Serialize};

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
/// * `s3_access_key` - S3 访问密钥，当 storage_type="s3" 时使用（敏感信息）
/// * `s3_secret_key` - S3 密钥，当 storage_type="s3" 时使用（敏感信息）
/// * `s3_endpoint` - S3 端点（可选），用于 MinIO 等兼容服务
///
/// # 安全提示
///
/// `s3_access_key` 和 `s3_secret_key` 字段包含 S3 访问凭据。
/// 外部模块应使用相应的 getter 方法访问。
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__STORAGE__")]
pub struct StorageSettings {
    /// 存储类型 (local, s3)
    #[config(default = "local".to_string())]
    pub storage_type: String,

    /// 本地存储路径 (当 type=local 时使用)
    #[config(default = Some("./storage".to_string()))]
    pub local_path: Option<String>,

    /// S3 区域
    pub s3_region: Option<String>,

    /// S3 存储桶名称
    pub s3_bucket: Option<String>,

    /// S3 访问密钥 (敏感信息)
    /// 注意：此字段包含敏感信息，仅 crate 内部可访问
    pub(crate) s3_access_key: Option<String>,

    /// S3 密钥 (敏感信息)
    /// 注意：此字段包含敏感信息，仅 crate 内部可访问
    pub(crate) s3_secret_key: Option<String>,

    /// S3 端点 (可选，用于 MinIO 等兼容服务)
    pub s3_endpoint: Option<String>,
}

impl StorageSettings {
    /// 获取 S3 访问密钥
    ///
    /// # 安全提示
    ///
    /// 此方法返回 S3 访问密钥，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn s3_access_key(&self) -> Option<&str> {
        self.s3_access_key.as_deref()
    }

    /// 获取 S3 密钥
    ///
    /// # 安全提示
    ///
    /// 此方法返回 S3 密钥，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn s3_secret_key(&self) -> Option<&str> {
        self.s3_secret_key.as_deref()
    }

    /// 创建本地存储配置
    pub fn local(path: impl Into<String>) -> Self {
        Self {
            storage_type: "local".to_string(),
            local_path: Some(path.into()),
            s3_region: None,
            s3_bucket: None,
            s3_access_key: None,
            s3_secret_key: None,
            s3_endpoint: None,
        }
    }

    /// 创建 S3 存储配置
    #[allow(clippy::too_many_arguments)]
    pub fn s3(
        region: impl Into<String>,
        bucket: impl Into<String>,
        access_key: Option<String>,
        secret_key: Option<String>,
        endpoint: Option<String>,
    ) -> Self {
        Self {
            storage_type: "s3".to_string(),
            local_path: None,
            s3_region: Some(region.into()),
            s3_bucket: Some(bucket.into()),
            s3_access_key: access_key,
            s3_secret_key: secret_key,
            s3_endpoint: endpoint,
        }
    }
}

/// Webhook配置设置
///
/// 配置 Webhook 功能的参数
///
/// # 字段说明
///
/// * `secret` - Webhook 签名密钥，用于验证请求真实性（敏感信息，仅 crate 可见）
///
/// # 安全提示
///
/// `secret` 字段包含 Webhook 签名密钥，泄露可能导致伪造请求。
/// 该字段仅对 crate 可见，外部模块应使用 `secret()` 方法访问。
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__WEBHOOK__")]
pub struct WebhookSettings {
    /// Webhook签名密钥 (敏感信息)
    /// 注意：此字段包含敏感信息，仅 crate 内部可访问
    pub(crate) secret: String,

    /// 最大重试次数
    #[config(default = 5)]
    pub max_retries: u32,

    /// 批处理大小
    #[config(default = 1000)]
    pub batch_size: usize,
}

impl WebhookSettings {
    /// 获取 Webhook 签名密钥
    ///
    /// # 安全提示
    ///
    /// 此方法返回 Webhook 签名密钥，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn secret(&self) -> &str {
        &self.secret
    }
}
