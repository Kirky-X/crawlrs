// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified response builders for presentation layer
//!
//! Provides standardized response formatting to eliminate code duplication
//! across handlers and ensure consistent API responses.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Standard API error response
#[inline]
pub fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({
            "success": false,
            "error": message.into()
        })),
    )
        .into_response()
}

/// API error response with error code
#[inline]
pub fn error_response_with_code(
    status: StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
) -> Response {
    (
        status,
        Json(json!({
            "success": false,
            "error": {
                "code": code.into(),
                "message": message.into()
            }
        })),
    )
        .into_response()
}

/// Standard success response with data
#[inline]
pub fn success_response<T: serde::Serialize>(status: StatusCode, data: T) -> Response {
    (status, Json(json!({ "success": true, "data": data }))).into_response()
}

/// Success response with metadata
#[inline]
pub fn success_response_with_meta<T: serde::Serialize, M: serde::Serialize>(
    status: StatusCode,
    data: T,
    meta: M,
) -> Response {
    (
        status,
        Json(json!({
            "success": true,
            "data": data,
            "meta": meta
        })),
    )
        .into_response()
}

/// Common error responses for quick access
pub mod errors {
    use super::*;

    /// Internal server error response
    #[inline]
    pub fn internal_server_error(message: impl Into<String>) -> Response {
        error_response(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    /// Bad request error response
    #[inline]
    pub fn bad_request(message: impl Into<String>) -> Response {
        error_response(StatusCode::BAD_REQUEST, message)
    }

    /// Not found error response
    #[inline]
    pub fn not_found(message: impl Into<String>) -> Response {
        error_response(StatusCode::NOT_FOUND, message)
    }

    /// Forbidden error response
    #[inline]
    pub fn forbidden(message: impl Into<String>) -> Response {
        error_response(StatusCode::FORBIDDEN, message)
    }

    /// Unauthorized error response
    #[inline]
    pub fn unauthorized(message: impl Into<String>) -> Response {
        error_response(StatusCode::UNAUTHORIZED, message)
    }

    /// Too many requests error response
    #[inline]
    pub fn too_many_requests(message: impl Into<String>) -> Response {
        error_response(StatusCode::TOO_MANY_REQUESTS, message)
    }

    /// Unprocessable entity error response
    #[inline]
    pub fn unprocessable_entity(message: impl Into<String>) -> Response {
        error_response(StatusCode::UNPROCESSABLE_ENTITY, message)
    }

    /// Payment required error response
    #[inline]
    pub fn payment_required(message: impl Into<String>) -> Response {
        error_response(StatusCode::PAYMENT_REQUIRED, message)
    }
}

/// Validation error response
#[inline]
pub fn validation_error(message: impl Into<String>) -> Response {
    error_response(
        StatusCode::BAD_REQUEST,
        format!("Validation error: {}", message.into()),
    )
}

/// Not found response for resources
#[inline]
pub fn resource_not_found(resource: impl Into<String>) -> Response {
    errors::not_found(format!("{} not found", resource.into()))
}

/// Access denied response
#[inline]
pub fn access_denied() -> Response {
    errors::forbidden("Access denied")
}
