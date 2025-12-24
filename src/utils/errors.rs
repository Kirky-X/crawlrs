// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use thiserror::Error;

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
