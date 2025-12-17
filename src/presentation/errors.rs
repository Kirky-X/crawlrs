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

pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self.0.downcast_ref::<RepositoryError>() {
            Some(RepositoryError::Database(db_err)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", db_err),
            ),
            Some(RepositoryError::NotFound) => {
                (StatusCode::NOT_FOUND, "Resource not found".to_string())
            }
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Internal server error: {}", self.0),
            ),
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
