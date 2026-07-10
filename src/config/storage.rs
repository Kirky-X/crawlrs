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
    #[config(default = "./storage".to_string())]
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

#[cfg(test)]
mod tests {
    use super::*;

    // ========== StorageSettings::default tests ==========

    #[test]
    fn test_storage_default_type_is_local() {
        let settings = StorageSettings::default();
        assert_eq!(
            settings.storage_type, "local",
            "default storage_type should be local"
        );
    }

    #[test]
    fn test_storage_default_local_path() {
        let settings = StorageSettings::default();
        assert_eq!(
            settings.local_path.as_deref(),
            Some("./storage"),
            "default local_path should be ./storage"
        );
    }

    #[test]
    fn test_storage_default_s3_fields_are_none() {
        let settings = StorageSettings::default();
        assert!(
            settings.s3_region.is_none(),
            "default s3_region should be None"
        );
        assert!(
            settings.s3_bucket.is_none(),
            "default s3_bucket should be None"
        );
        assert!(
            settings.s3_access_key().is_none(),
            "default s3_access_key should be None"
        );
        assert!(
            settings.s3_secret_key().is_none(),
            "default s3_secret_key should be None"
        );
        assert!(
            settings.s3_endpoint.is_none(),
            "default s3_endpoint should be None"
        );
    }

    // ========== StorageSettings::local() tests ==========

    #[test]
    fn test_storage_local_sets_type_and_path() {
        let settings = StorageSettings::local("/data/storage");
        assert_eq!(settings.storage_type, "local");
        assert_eq!(settings.local_path.as_deref(), Some("/data/storage"));
    }

    #[test]
    fn test_storage_local_clears_s3_fields() {
        let settings = StorageSettings::local("/tmp");
        assert!(settings.s3_region.is_none());
        assert!(settings.s3_bucket.is_none());
        assert!(settings.s3_access_key().is_none());
        assert!(settings.s3_secret_key().is_none());
        assert!(settings.s3_endpoint.is_none());
    }

    #[test]
    fn test_storage_local_accepts_string() {
        let path = String::from("/var/data");
        let settings = StorageSettings::local(path);
        assert_eq!(settings.local_path.as_deref(), Some("/var/data"));
    }

    // ========== StorageSettings::s3() tests ==========

    #[test]
    fn test_storage_s3_sets_type_and_fields() {
        let settings = StorageSettings::s3(
            "us-east-1",
            "my-bucket",
            Some("access-key".to_string()),
            Some("secret-key".to_string()),
            None,
        );
        assert_eq!(settings.storage_type, "s3");
        assert_eq!(settings.s3_region.as_deref(), Some("us-east-1"));
        assert_eq!(settings.s3_bucket.as_deref(), Some("my-bucket"));
        assert_eq!(settings.s3_access_key(), Some("access-key"));
        assert_eq!(settings.s3_secret_key(), Some("secret-key"));
        assert!(settings.s3_endpoint.is_none());
    }

    #[test]
    fn test_storage_s3_without_credentials() {
        let settings = StorageSettings::s3("eu-west-1", "bucket", None, None, None);
        assert_eq!(settings.storage_type, "s3");
        assert_eq!(settings.s3_region.as_deref(), Some("eu-west-1"));
        assert_eq!(settings.s3_bucket.as_deref(), Some("bucket"));
        assert!(settings.s3_access_key().is_none());
        assert!(settings.s3_secret_key().is_none());
    }

    #[test]
    fn test_storage_s3_with_endpoint() {
        let settings = StorageSettings::s3(
            "us-east-1",
            "bucket",
            None,
            None,
            Some("https://minio.local".to_string()),
        );
        assert_eq!(settings.s3_endpoint.as_deref(), Some("https://minio.local"));
    }

    #[test]
    fn test_storage_s3_clears_local_path() {
        let settings = StorageSettings::s3("r", "b", None, None, None);
        assert!(settings.local_path.is_none());
    }

    // ========== StorageSettings serde tests ==========

    #[test]
    fn test_storage_serde_roundtrip_local() {
        let settings = StorageSettings::local("/data");
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: StorageSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.storage_type, settings.storage_type);
        assert_eq!(back.local_path, settings.local_path);
    }

    #[test]
    fn test_storage_serde_roundtrip_s3() {
        let settings = StorageSettings::s3(
            "ap-northeast-1",
            "bucket-x",
            Some("ak".to_string()),
            Some("sk".to_string()),
            Some("https://endpoint".to_string()),
        );
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: StorageSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.storage_type, "s3");
        assert_eq!(back.s3_region.as_deref(), Some("ap-northeast-1"));
        assert_eq!(back.s3_bucket.as_deref(), Some("bucket-x"));
        assert_eq!(back.s3_access_key(), Some("ak"));
        assert_eq!(back.s3_secret_key(), Some("sk"));
        assert_eq!(back.s3_endpoint.as_deref(), Some("https://endpoint"));
    }

    #[test]
    fn test_storage_clone_preserves_fields() {
        let settings = StorageSettings::s3(
            "r",
            "b",
            Some("ak".to_string()),
            Some("sk".to_string()),
            Some("ep".to_string()),
        );
        let cloned = settings.clone();
        assert_eq!(cloned.storage_type, settings.storage_type);
        assert_eq!(cloned.s3_region, settings.s3_region);
        assert_eq!(cloned.s3_bucket, settings.s3_bucket);
        assert_eq!(cloned.s3_access_key(), settings.s3_access_key());
        assert_eq!(cloned.s3_secret_key(), settings.s3_secret_key());
    }

    #[test]
    fn test_storage_debug_does_not_panic() {
        let settings = StorageSettings::default();
        let debug = format!("{:?}", settings);
        assert!(
            debug.contains("StorageSettings"),
            "Debug should contain struct name"
        );
    }

    // ========== WebhookSettings tests ==========

    #[test]
    fn test_webhook_secret_returns_value() {
        let settings = WebhookSettings {
            secret: "my-secret".to_string(),
            max_retries: 3,
            batch_size: 100,
        };
        assert_eq!(settings.secret(), "my-secret");
    }

    #[test]
    fn test_webhook_secret_empty_string() {
        let settings = WebhookSettings {
            secret: String::new(),
            max_retries: 0,
            batch_size: 1,
        };
        assert_eq!(settings.secret(), "");
    }

    #[test]
    fn test_webhook_default_max_retries() {
        let settings = WebhookSettings {
            secret: "s".to_string(),
            max_retries: 5,
            batch_size: 1000,
        };
        assert_eq!(settings.max_retries, 5);
    }

    #[test]
    fn test_webhook_default_batch_size() {
        let settings = WebhookSettings {
            secret: "s".to_string(),
            max_retries: 5,
            batch_size: 1000,
        };
        assert_eq!(settings.batch_size, 1000);
    }

    #[test]
    fn test_webhook_clone_preserves_secret() {
        let settings = WebhookSettings {
            secret: "secret-xyz".to_string(),
            max_retries: 7,
            batch_size: 500,
        };
        let cloned = settings.clone();
        assert_eq!(cloned.secret(), "secret-xyz");
        assert_eq!(cloned.max_retries, 7);
        assert_eq!(cloned.batch_size, 500);
    }

    #[test]
    fn test_webhook_serde_roundtrip() {
        let settings = WebhookSettings {
            secret: "roundtrip-secret".to_string(),
            max_retries: 4,
            batch_size: 200,
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: WebhookSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.secret(), "roundtrip-secret");
        assert_eq!(back.max_retries, 4);
        assert_eq!(back.batch_size, 200);
    }

    #[test]
    fn test_webhook_debug_does_not_panic() {
        let settings = WebhookSettings {
            secret: "debug-secret".to_string(),
            max_retries: 1,
            batch_size: 1,
        };
        let debug = format!("{:?}", settings);
        assert!(
            debug.contains("WebhookSettings"),
            "Debug should contain struct name"
        );
    }
}
