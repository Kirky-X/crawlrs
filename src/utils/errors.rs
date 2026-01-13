// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use thiserror::Error;

/// Macro to generate standard error conversions for basic types
///
/// # Usage
///
/// ```rust
/// impl_basic_error_conversions!(MyError, InternalVariant);
/// ```
///
/// This generates:
/// - `impl From<String> for MyError`
/// - `impl From<&str> for MyError`
/// - `impl From<anyhow::Error> for MyError`
#[macro_export]
macro_rules! impl_basic_error_conversions {
    ($error_type:ty, $variant:ident) => {
        impl From<String> for $error_type {
            fn from(msg: String) -> Self {
                Self::$variant(msg)
            }
        }

        impl From<&str> for $error_type {
            fn from(msg: &str) -> Self {
                Self::$variant(msg.to_string())
            }
        }

        impl From<anyhow::Error> for $error_type {
            fn from(err: anyhow::Error) -> Self {
                Self::$variant(err.to_string())
            }
        }
    };
}

/// 仓库层错误类型
#[derive(Error, Debug)]
pub enum RepositoryError {
    #[error("数据库错误: {0}")]
    DatabaseError(String),

    #[error("未找到数据")]
    NotFound,

    #[error("数据已存在")]
    AlreadyExists,

    #[error("无效参数: {0}")]
    InvalidParameter(String),

    #[error("内部错误: {0}")]
    InternalError(String),
}

/// Worker错误类型
#[derive(Error, Debug)]
pub enum WorkerError {
    #[error("仓库错误: {0}")]
    RepositoryError(String),

    #[error("限流错误: {0}")]
    RateLimitingError(String),

    #[error("内部错误: {0}")]
    InternalError(String),

    #[error("领域错误: {0}")]
    DomainError(String),

    #[error("服务错误: {0}")]
    ServiceError(String),

    #[error("未找到: {0}")]
    NotFound(String),
}

// From implementations for WorkerError using macro
impl_basic_error_conversions!(WorkerError, InternalError);

impl From<RepositoryError> for WorkerError {
    fn from(err: RepositoryError) -> Self {
        match err {
            RepositoryError::DatabaseError(msg) => WorkerError::RepositoryError(msg),
            RepositoryError::NotFound => WorkerError::NotFound("Resource not found".to_string()),
            RepositoryError::AlreadyExists => {
                WorkerError::RepositoryError("Resource already exists".to_string())
            }
            RepositoryError::InvalidParameter(msg) => WorkerError::DomainError(msg),
            RepositoryError::InternalError(msg) => WorkerError::InternalError(msg),
        }
    }
}

/// 统一的应用层错误
#[derive(Error, Debug)]
pub enum AppError {
    #[error("认证失败: {0}")]
    Authentication(String),

    #[error("授权失败: {0}")]
    Authorization(String),

    #[error("验证错误: {0}")]
    Validation(String),

    #[error("未找到资源: {0}")]
    NotFound(String),

    #[error("请求过多: {0}")]
    RateLimited(String),

    #[error("内部服务器错误: {0}")]
    Internal(String),

    #[error("服务不可用: {0}")]
    ServiceUnavailable(String),
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            AppError::Authentication(msg) => (axum::http::StatusCode::UNAUTHORIZED, msg),
            AppError::Authorization(msg) => (axum::http::StatusCode::FORBIDDEN, msg),
            AppError::Validation(msg) => (axum::http::StatusCode::BAD_REQUEST, msg),
            AppError::NotFound(msg) => (axum::http::StatusCode::NOT_FOUND, msg),
            AppError::RateLimited(msg) => (axum::http::StatusCode::TOO_MANY_REQUESTS, msg),
            AppError::Internal(msg) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, msg),
            AppError::ServiceUnavailable(msg) => (axum::http::StatusCode::SERVICE_UNAVAILABLE, msg),
        };

        let body = serde_json::json!({
            "error": {
                "code": status.as_u16(),
                "message": message,
            }
        });

        (status, axum::Json(body)).into_response()
    }
}

// From implementations for AppError using macro
impl_basic_error_conversions!(AppError, Internal);

impl From<RepositoryError> for AppError {
    fn from(err: RepositoryError) -> Self {
        match err {
            RepositoryError::DatabaseError(msg) => AppError::Internal(msg),
            RepositoryError::NotFound => AppError::NotFound("Resource not found".to_string()),
            RepositoryError::AlreadyExists => {
                AppError::Validation("Resource already exists".to_string())
            }
            RepositoryError::InvalidParameter(msg) => AppError::Validation(msg),
            RepositoryError::InternalError(msg) => AppError::Internal(msg),
        }
    }
}

impl From<crate::domain::repositories::task_repository::RepositoryError> for AppError {
    fn from(err: crate::domain::repositories::task_repository::RepositoryError) -> Self {
        match err {
            crate::domain::repositories::task_repository::RepositoryError::Database(db_err) => {
                AppError::Internal(db_err.to_string())
            }
            crate::domain::repositories::task_repository::RepositoryError::NotFound => {
                AppError::NotFound("Resource not found".to_string())
            }
        }
    }
}

/// From implementations for security-related errors
impl From<crate::config::settings::ConfigSecurityError> for AppError {
    fn from(err: crate::config::settings::ConfigSecurityError) -> Self {
        AppError::Validation(err.to_string())
    }
}

impl From<crate::utils::robots::RobotsCheckerError> for AppError {
    fn from(err: crate::utils::robots::RobotsCheckerError) -> Self {
        AppError::Internal(err.to_string())
    }
}
