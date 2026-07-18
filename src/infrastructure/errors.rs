// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Infrastructure层错误类型
//!
//! 定义外部系统交互的错误类型

use thiserror::Error;

/// Infrastructure层错误类型
///
/// 表示外部系统交互失败，如数据库、缓存等
#[derive(Error, Debug)]
pub enum InfrastructureError {
    // ==================== 数据库错误 ====================
    #[error("数据库连接失败: {0}")]
    DatabaseConnection(#[from] sea_orm::DbErr),

    #[error("数据库迁移失败: {message}")]
    DatabaseMigration { message: String },

    #[error("记录未找到: {table} where {condition}")]
    RecordNotFound { table: String, condition: String },

    #[error("记录已存在: {table}, key: {key}")]
    DuplicateRecord { table: String, key: String },

    // ==================== 缓存错误 ====================
    #[error("缓存未命中: {key}")]
    CacheMiss { key: String },

    #[error("缓存序列化失败: {0}")]
    CacheSerialization(#[from] serde_json::Error),

    // ==================== 网络错误 ====================
    #[error("HTTP请求失败: {url}, 状态码: {status}")]
    HttpRequestFailed { url: String, status: u16 },

    #[error("HTTP客户端错误: {0}")]
    HttpClient(#[from] reqwest::Error),

    #[error("连接超时: {host}, 超时时间: {timeout_seconds}秒")]
    ConnectionTimeout { host: String, timeout_seconds: u64 },

    // ==================== 配置错误 ====================
    #[error("配置缺失: {key}")]
    ConfigMissing { key: String },

    #[error("配置无效: {key} = {value}")]
    ConfigInvalid { key: String, value: String },
}

// 辅助构造函数
impl InfrastructureError {
    pub fn cache_miss(key: impl Into<String>) -> Self {
        InfrastructureError::CacheMiss { key: key.into() }
    }
}

// 从InfrastructureError到DomainError的转换
impl From<InfrastructureError> for crate::domain::errors::DomainError {
    fn from(err: InfrastructureError) -> Self {
        match err {
            InfrastructureError::RecordNotFound { .. } => {
                crate::domain::errors::DomainError::TaskNotFound {
                    task_id: uuid::Uuid::nil(),
                }
            }
            _ => crate::domain::errors::DomainError::crawl_config(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_miss() {
        let error = InfrastructureError::cache_miss("test_key");

        let msg = error.to_string();
        assert!(msg.contains("缓存未命中"));
        assert!(msg.contains("test_key"));
    }

    #[test]
    fn test_conversion_to_domain_error() {
        let infra_err = InfrastructureError::CacheMiss {
            key: "test".to_string(),
        };

        let domain_err: crate::domain::errors::DomainError = infra_err.into();
        assert!(matches!(
            domain_err,
            crate::domain::errors::DomainError::CrawlConfigError { .. }
        ));
    }

    #[test]
    fn test_conversion_to_domain_error_record_not_found() {
        let infra_err = InfrastructureError::RecordNotFound {
            table: "tasks".to_string(),
            condition: "id = 1".to_string(),
        };

        let domain_err: crate::domain::errors::DomainError = infra_err.into();
        assert!(matches!(
            domain_err,
            crate::domain::errors::DomainError::TaskNotFound { .. }
        ));
    }

    #[test]
    fn test_database_connection_from_db_err() {
        let db_err = sea_orm::DbErr::Custom("connection refused".to_string());
        let error: InfrastructureError = db_err.into();
        let msg = error.to_string();
        assert!(msg.contains("数据库连接失败"));
        assert!(msg.contains("connection refused"));
    }

    #[test]
    fn test_database_migration_display() {
        let error = InfrastructureError::DatabaseMigration {
            message: "schema mismatch".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("数据库迁移失败"));
        assert!(msg.contains("schema mismatch"));
    }

    #[test]
    fn test_database_migration_empty_message_boundary() {
        let error = InfrastructureError::DatabaseMigration {
            message: String::new(),
        };
        let msg = error.to_string();
        assert!(msg.contains("数据库迁移失败"));
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_record_not_found_display() {
        let error = InfrastructureError::RecordNotFound {
            table: "tasks".to_string(),
            condition: "id = 42".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("记录未找到"));
        assert!(msg.contains("tasks"));
        assert!(msg.contains("id = 42"));
    }

    #[test]
    fn test_duplicate_record_display() {
        let error = InfrastructureError::DuplicateRecord {
            table: "users".to_string(),
            key: "user-001".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("记录已存在"));
        assert!(msg.contains("users"));
        assert!(msg.contains("user-001"));
    }

    #[test]
    fn test_cache_miss_helper_constructor() {
        let error = InfrastructureError::cache_miss("session:abc");
        match error {
            InfrastructureError::CacheMiss { key } => {
                assert_eq!(key, "session:abc");
            }
            _ => panic!("expected CacheMiss variant"),
        }
    }

    #[test]
    fn test_cache_miss_empty_key_boundary() {
        let error = InfrastructureError::cache_miss("");
        let msg = error.to_string();
        assert!(msg.contains("缓存未命中"));
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_cache_serialization_from_serde_json_error() {
        let result: Result<i32, _> = serde_json::from_str("not valid json");
        let json_err = result.unwrap_err();
        let error: InfrastructureError = json_err.into();
        let msg = error.to_string();
        assert!(msg.contains("缓存序列化失败"));
    }

    #[test]
    fn test_http_request_failed_display() {
        let error = InfrastructureError::HttpRequestFailed {
            url: "https://api.example.com".to_string(),
            status: 500,
        };
        let msg = error.to_string();
        assert!(msg.contains("HTTP请求失败"));
        assert!(msg.contains("https://api.example.com"));
        assert!(msg.contains("500"));
    }

    #[test]
    fn test_http_request_failed_boundary_status() {
        let error = InfrastructureError::HttpRequestFailed {
            url: "https://api.example.com".to_string(),
            status: 0,
        };
        let msg = error.to_string();
        assert!(msg.contains("状态码: 0"));
    }

    #[test]
    fn test_http_client_from_reqwest_error() {
        // 通过尝试构建无效 URL 的请求触发 reqwest::Error
        let result = reqwest::Client::new().get("ht!tp://bad url").build();
        let req_err = result.unwrap_err();
        let error: InfrastructureError = req_err.into();
        let msg = error.to_string();
        assert!(msg.contains("HTTP客户端错误"));
    }

    #[test]
    fn test_connection_timeout_display() {
        let error = InfrastructureError::ConnectionTimeout {
            host: "db.internal".to_string(),
            timeout_seconds: 30,
        };
        let msg = error.to_string();
        assert!(msg.contains("连接超时"));
        assert!(msg.contains("db.internal"));
        assert!(msg.contains("30"));
    }

    #[test]
    fn test_connection_timeout_boundary_zero() {
        let error = InfrastructureError::ConnectionTimeout {
            host: String::new(),
            timeout_seconds: 0,
        };
        let msg = error.to_string();
        assert!(msg.contains("超时时间: 0秒"));
    }

    #[test]
    fn test_config_missing_display() {
        let error = InfrastructureError::ConfigMissing {
            key: "DATABASE_URL".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("配置缺失"));
        assert!(msg.contains("DATABASE_URL"));
    }

    #[test]
    fn test_config_invalid_display() {
        let error = InfrastructureError::ConfigInvalid {
            key: "PORT".to_string(),
            value: "not-a-number".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("配置无效"));
        assert!(msg.contains("PORT"));
        assert!(msg.contains("not-a-number"));
    }

    #[test]
    fn test_all_variants_implement_std_error() {
        fn assert_error<T: std::error::Error>(_: &T) {}
        let db_err = sea_orm::DbErr::Custom("e".to_string());
        let json_err: serde_json::Error = serde_json::from_str::<i32>("x").unwrap_err();
        let req_err = reqwest::Client::new()
            .get("ht!tp://bad")
            .build()
            .unwrap_err();
        let errors: Vec<InfrastructureError> = vec![
            InfrastructureError::DatabaseConnection(db_err),
            InfrastructureError::DatabaseMigration {
                message: "m".into(),
            },
            InfrastructureError::RecordNotFound {
                table: "t".into(),
                condition: "c".into(),
            },
            InfrastructureError::DuplicateRecord {
                table: "t".into(),
                key: "k".into(),
            },
            InfrastructureError::CacheMiss { key: "k".into() },
            InfrastructureError::CacheSerialization(json_err),
            InfrastructureError::HttpRequestFailed {
                url: "u".into(),
                status: 500,
            },
            InfrastructureError::HttpClient(req_err),
            InfrastructureError::ConnectionTimeout {
                host: "h".into(),
                timeout_seconds: 1,
            },
            InfrastructureError::ConfigMissing { key: "k".into() },
            InfrastructureError::ConfigInvalid {
                key: "k".into(),
                value: "v".into(),
            },
        ];
        for err in &errors {
            assert_error(err);
        }
    }

    #[test]
    fn test_conversion_to_domain_error_preserves_message_for_non_record_not_found() {
        // 非 RecordNotFound 的所有变体都应转为 CrawlConfigError 且包含原始错误描述
        let infra_err = InfrastructureError::ConfigMissing {
            key: "MISSING_KEY".to_string(),
        };
        let domain_err: crate::domain::errors::DomainError = infra_err.into();
        match domain_err {
            crate::domain::errors::DomainError::CrawlConfigError { message, .. } => {
                assert!(message.contains("配置缺失"));
                assert!(message.contains("MISSING_KEY"));
            }
            _ => panic!("expected CrawlConfigError for non-RecordNotFound variant"),
        }
    }

    #[test]
    fn test_conversion_to_domain_error_duplicate_record() {
        let infra_err = InfrastructureError::DuplicateRecord {
            table: "tasks".into(),
            key: "k1".into(),
        };
        let domain_err: crate::domain::errors::DomainError = infra_err.into();
        // DuplicateRecord 不应映射到 TaskNotFound (nil Uuid)，应走默认分支
        assert!(matches!(
            domain_err,
            crate::domain::errors::DomainError::CrawlConfigError { .. }
        ));
    }
}
