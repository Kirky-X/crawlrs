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
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Unified API response wrapper
///
/// # Type Parameters
///
/// * `T` - The type of data being returned
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    /// Whether the request was successful
    pub success: bool,
    /// Response data
    pub data: Option<T>,
    /// Error details (only present when success is false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
    /// Pagination metadata (only present for list responses)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<PaginationMeta>,
}

impl<T> ApiResponse<T> {
    /// Create a success response with data
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            meta: None,
        }
    }

    /// Create a success response with data and pagination
    pub fn success_with_meta(data: T, meta: PaginationMeta) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            meta: Some(meta),
        }
    }

    /// Create an error response
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(ApiError {
                code: code.into(),
                message: message.into(),
            }),
            meta: None,
        }
    }
}

/// Pagination metadata for list responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationMeta {
    /// Current page number (1-indexed)
    pub page: u32,
    /// Number of items per page
    pub per_page: u32,
    /// Total number of items
    pub total_items: u64,
    /// Total number of pages
    pub total_pages: u32,
    /// Has next page
    pub has_next: bool,
    /// Has previous page
    pub has_previous: bool,
}

impl PaginationMeta {
    /// Create pagination metadata from parameters
    pub fn new(page: u32, per_page: u32, total_items: u64) -> Self {
        let total_pages = if total_items == 0 {
            0
        } else {
            total_items.div_ceil(per_page as u64) as u32
        };

        Self {
            page,
            per_page,
            total_items,
            total_pages,
            has_next: page < total_pages,
            has_previous: page > 1,
        }
    }

    /// Calculate offset for database queries
    pub fn offset(&self) -> u64 {
        ((self.page - 1) * self.per_page) as u64
    }

    /// Calculate limit for database queries
    pub fn limit(&self) -> u32 {
        self.per_page
    }
}

/// Standard API error details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    /// Error code for programmatic handling
    pub code: String,
    /// Human-readable error message
    pub message: String,
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        let status = if self.success {
            StatusCode::OK
        } else {
            // Determine status code from error code
            match self.error.as_ref().map(|e| e.code.as_str()) {
                Some("VALIDATION_ERROR") => StatusCode::BAD_REQUEST,
                Some("NOT_FOUND") => StatusCode::NOT_FOUND,
                Some("UNAUTHORIZED") => StatusCode::UNAUTHORIZED,
                Some("FORBIDDEN") => StatusCode::FORBIDDEN,
                Some("RATE_LIMITED") => StatusCode::TOO_MANY_REQUESTS,
                Some("CONFLICT") => StatusCode::CONFLICT,
                Some("PRECONDITION_FAILED") => StatusCode::PRECONDITION_FAILED,
                Some("UNPROCESSABLE_ENTITY") => StatusCode::UNPROCESSABLE_ENTITY,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            }
        };
        (status, Json(self)).into_response()
    }
}

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

/// Standard error codes for API responses
pub mod error_codes {
    /// Validation failed
    pub const VALIDATION_ERROR: &str = "VALIDATION_ERROR";
    /// Resource not found
    pub const NOT_FOUND: &str = "NOT_FOUND";
    /// Unauthorized access
    pub const UNAUTHORIZED: &str = "UNAUTHORIZED";
    /// Forbidden access
    pub const FORBIDDEN: &str = "FORBIDDEN";
    /// Rate limit exceeded
    pub const RATE_LIMITED: &str = "RATE_LIMITED";
    /// Resource conflict
    pub const CONFLICT: &str = "CONFLICT";
    /// Precondition failed
    pub const PRECONDITION_FAILED: &str = "PRECONDITION_FAILED";
    /// Unprocessable entity
    pub const UNPROCESSABLE_ENTITY: &str = "UNPROCESSABLE_ENTITY";
    /// Internal server error
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
    /// Service unavailable
    pub const SERVICE_UNAVAILABLE: &str = "SERVICE_UNAVAILABLE";
    /// Database error
    pub const DATABASE_ERROR: &str = "DATABASE_ERROR";
    /// Cache error
    pub const CACHE_ERROR: &str = "CACHE_ERROR";
    /// External service error
    pub const EXTERNAL_SERVICE_ERROR: &str = "EXTERNAL_SERVICE_ERROR";
    /// Timeout error
    pub const TIMEOUT: &str = "TIMEOUT";
    /// Quota exceeded
    pub const QUOTA_EXCEEDED: &str = "QUOTA_EXCEEDED";
    /// Feature not enabled
    pub const FEATURE_DISABLED: &str = "FEATURE_DISABLED";
}
