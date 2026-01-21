// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 统一错误类型定义
//!
//! 提供应用程序的统一错误类型，用于处理所有应用级别的错误

use axum::http::StatusCode;
use serde::Serialize;

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

    /// 引擎错误
    #[error("Engine error: {0}")]
    Engine(String),

    /// 缓存错误
    #[error("Cache error: {0}")]
    Cache(String),

    /// 任务错误
    #[error("Task error: {0}")]
    Task(String),

    /// 速率限制错误
    #[error("Rate limit error: {0}")]
    RateLimit(String),

    /// 其他错误
    #[error("Error: {0}")]
    Other(String),
}

impl AppError {
    /// 获取错误的 HTTP 状态码
    pub fn status_code(&self) -> StatusCode {
        match self {
            AppError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Network(_) => StatusCode::BAD_GATEWAY,
            AppError::Config(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Validation(_) => StatusCode::BAD_REQUEST,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::PermissionDenied(_) => StatusCode::FORBIDDEN,
            AppError::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
            AppError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Json(_) => StatusCode::BAD_REQUEST,
            AppError::Engine(_) => StatusCode::BAD_GATEWAY,
            AppError::Cache(_) => StatusCode::SERVICE_UNAVAILABLE,
            AppError::Task(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::RateLimit(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 获取错误的代码
    pub fn error_code(&self) -> &'static str {
        match self {
            AppError::Database(_) => "DATABASE_ERROR",
            AppError::Network(_) => "EXTERNAL_SERVICE_ERROR",
            AppError::Config(_) => "CONFIGURATION_ERROR",
            AppError::Validation(_) => "VALIDATION_ERROR",
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::PermissionDenied(_) => "FORBIDDEN",
            AppError::Timeout(_) => "TIMEOUT",
            AppError::Io(_) => "IO_ERROR",
            AppError::Json(_) => "JSON_ERROR",
            AppError::Engine(_) => "ENGINE_ERROR",
            AppError::Cache(_) => "CACHE_ERROR",
            AppError::Task(_) => "TASK_ERROR",
            AppError::RateLimit(_) => "RATE_LIMITED",
            AppError::Other(_) => "INTERNAL_ERROR",
        }
    }

    /// 转换为 API 错误响应格式
    pub fn to_api_error_response(&self) -> ApiErrorResponse {
        ApiErrorResponse {
            code: self.error_code().to_string(),
            message: self.to_string(),
        }
    }
}

/// API 错误响应结构
#[derive(Debug, Serialize)]
pub struct ApiErrorResponse {
    /// 错误代码
    pub code: String,
    /// 错误消息
    pub message: String,
}

/// 应用程序结果类型
///
/// 使用统一的错误类型作为 Result 的错误变体
pub type AppResult<T> = Result<T, AppError>;

impl From<AppError> for ApiErrorResponse {
    fn from(error: AppError) -> Self {
        error.to_api_error_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

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

    #[test]
    fn test_status_code_mapping() {
        assert_eq!(AppError::NotFound("test".to_string()).status_code(), StatusCode::NOT_FOUND);
        assert_eq!(AppError::Validation("test".to_string()).status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(AppError::PermissionDenied("test".to_string()).status_code(), StatusCode::FORBIDDEN);
        assert_eq!(AppError::RateLimit("test".to_string()).status_code(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn test_error_code_mapping() {
        // Use a Custom error instead of the specific Conn variant
        let db_err = sea_orm::DbErr::Custom("test connection error".to_string());
        assert_eq!(AppError::Database(db_err).error_code(), "DATABASE_ERROR");
        assert_eq!(AppError::NotFound("test".to_string()).error_code(), "NOT_FOUND");
        assert_eq!(AppError::Validation("test".to_string()).error_code(), "VALIDATION_ERROR");
    }

    #[test]
    fn test_to_api_error_response() {
        let err = AppError::NotFound("User not found".to_string());
        let response = err.to_api_error_response();
        assert_eq!(response.code, "NOT_FOUND");
        assert_eq!(response.message, "Not found: User not found");
    }
}
