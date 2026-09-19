// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

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
    // platform 门控：sea-orm 仅随 platform 编译（2026-09-19 审计）；该变体在
    // errors.rs 外无引用，门控安全。
    #[cfg(feature = "platform")]
    #[error("Database connection failed: {0}")]
    DatabaseConnection(#[from] dbnexus::sea_orm::DbErr),

    #[error("Database migration failed: {message}")]
    DatabaseMigration { message: String },

    #[error("Record not found: {table} where {condition}")]
    RecordNotFound { table: String, condition: String },

    #[error("Record already exists: {table}, key: {key}")]
    DuplicateRecord { table: String, key: String },

    // ==================== 缓存错误 ====================
    #[error("Cache miss: {key}")]
    CacheMiss { key: String },

    #[error("Cache serialization failed: {0}")]
    CacheSerialization(#[from] serde_json::Error),

    // ==================== 网络错误 ====================
    #[error("HTTP request failed: {url}, status: {status}")]
    HttpRequestFailed { url: String, status: u16 },

    #[error("HTTP client error: {0}")]
    HttpClient(#[from] reqwest::Error),

    #[error("Connection timeout: {host}, timeout: {timeout_seconds}s")]
    ConnectionTimeout { host: String, timeout_seconds: u64 },

    // ==================== 配置错误 ====================
    #[error("Missing configuration: {key}")]
    ConfigMissing { key: String },

    #[error("Invalid configuration: {key} = {value}")]
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
        assert!(msg.contains("Cache miss"));
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

    // 变体随 platform 存在，测试同步门控
    #[test]
    #[cfg(feature = "platform")]
    fn test_database_connection_from_db_err() {
        let db_err = dbnexus::sea_orm::DbErr::Custom("connection refused".to_string());
        let error: InfrastructureError = db_err.into();
        let msg = error.to_string();
        assert!(msg.contains("Database connection failed"));
        assert!(msg.contains("connection refused"));
    }

    #[test]
    fn test_database_migration_display() {
        let error = InfrastructureError::DatabaseMigration {
            message: "schema mismatch".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("Database migration failed"));
        assert!(msg.contains("schema mismatch"));
    }

    #[test]
    fn test_database_migration_empty_message_boundary() {
        let error = InfrastructureError::DatabaseMigration {
            message: String::new(),
        };
        let msg = error.to_string();
        assert!(msg.contains("Database migration failed"));
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_record_not_found_display() {
        let error = InfrastructureError::RecordNotFound {
            table: "tasks".to_string(),
            condition: "id = 42".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("Record not found"));
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
        assert!(msg.contains("Record already exists"));
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
        assert!(msg.contains("Cache miss"));
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_cache_serialization_from_serde_json_error() {
        let result: Result<i32, _> = serde_json::from_str("not valid json");
        let json_err = result.unwrap_err();
        let error: InfrastructureError = json_err.into();
        let msg = error.to_string();
        assert!(msg.contains("Cache serialization failed"));
    }

    #[test]
    fn test_http_request_failed_display() {
        let error = InfrastructureError::HttpRequestFailed {
            url: "https://api.example.com".to_string(),
            status: 500,
        };
        let msg = error.to_string();
        assert!(msg.contains("HTTP request failed"));
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
        assert!(msg.contains("status: 0"));
    }

    #[test]
    fn test_http_client_from_reqwest_error() {
        // 通过尝试构建无效 URL 的请求触发 reqwest::Error
        let result = reqwest::Client::new().get("ht!tp://bad url").build();
        let req_err = result.unwrap_err();
        let error: InfrastructureError = req_err.into();
        let msg = error.to_string();
        assert!(msg.contains("HTTP client error"));
    }

    #[test]
    fn test_connection_timeout_display() {
        let error = InfrastructureError::ConnectionTimeout {
            host: "db.internal".to_string(),
            timeout_seconds: 30,
        };
        let msg = error.to_string();
        assert!(msg.contains("Connection timeout"));
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
        assert!(msg.contains("timeout: 0s"));
    }

    #[test]
    fn test_config_missing_display() {
        let error = InfrastructureError::ConfigMissing {
            key: "DATABASE_URL".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("Missing configuration"));
        assert!(msg.contains("DATABASE_URL"));
    }

    #[test]
    fn test_config_invalid_display() {
        let error = InfrastructureError::ConfigInvalid {
            key: "PORT".to_string(),
            value: "not-a-number".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("Invalid configuration"));
        assert!(msg.contains("PORT"));
        assert!(msg.contains("not-a-number"));
    }

    #[test]
    fn test_all_variants_implement_std_error() {
        fn assert_error<T: std::error::Error>(_: &T) {}
        #[cfg(feature = "platform")]
        let db_err = dbnexus::sea_orm::DbErr::Custom("e".to_string());
        let json_err: serde_json::Error = serde_json::from_str::<i32>("x").unwrap_err();
        let req_err = reqwest::Client::new()
            .get("ht!tp://bad")
            .build()
            .unwrap_err();
        let errors: Vec<InfrastructureError> = vec![
            #[cfg(feature = "platform")]
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
                assert!(message.contains("Missing configuration"));
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
