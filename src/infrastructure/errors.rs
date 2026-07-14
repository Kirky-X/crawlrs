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
}
