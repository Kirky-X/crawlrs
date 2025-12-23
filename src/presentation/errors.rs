// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::domain::repositories::task_repository::RepositoryError;

/// 应用错误类型
///
/// 封装所有可能的应用层错误，提供统一的错误处理接口
#[derive(Debug)]
pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let error_message = self.0.to_string();

        let status = match self.0.downcast_ref::<RepositoryError>() {
            Some(RepositoryError::Database(_db_err)) => StatusCode::INTERNAL_SERVER_ERROR,
            Some(RepositoryError::NotFound) => StatusCode::NOT_FOUND,
            None => {
                // 检查是否为验证错误（包含特定关键词）
                if error_message.contains("cannot be empty")
                    || error_message.contains("invalid")
                    || error_message.contains("required")
                    || error_message.contains("validation")
                {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }
        };

        let body = Json(json!({ "error": error_message }));
        (status, body).into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
