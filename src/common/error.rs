// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 统一错误类型定义
//!
//! 提供应用程序的统一错误类型，用于处理所有应用级别的错误

use std::fmt;

/// 应用程序错误类型
///
/// 统一所有应用级别的错误，提供清晰的错误分类和上下文信息
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// 数据库错误
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    /// 网络错误
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    /// 配置错误
    #[error("Configuration error: {0}")]
    Config(String),

    /// 验证错误
    #[error("Validation error: {0}")]
    Validation(String),

    /// 资源未找到
    #[error("Not found: {0}")]
    NotFound(String),

    /// 权限拒绝
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// 超时错误
    #[error("Timeout: {0}")]
    Timeout(String),

    /// IO 错误
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON 错误
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// 其他错误
    #[error("Error: {0}")]
    Other(String),
}

/// 应用程序结果类型
///
/// 使用统一的错误类型作为 Result 的错误变体
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = AppError::NotFound("User".to_string());
        assert_eq!(err.to_string(), "Not found: User");
    }

    #[test]
    fn test_error_debug() {
        let err = AppError::Validation("Invalid email".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Validation"));
    }

    #[test]
    fn test_app_result() {
        let result: AppResult<i32> = Ok(42);
        assert!(result.is_ok());

        let result: AppResult<i32> = Err(AppError::Config("Missing key".to_string()));
        assert!(result.is_err());
    }
}
