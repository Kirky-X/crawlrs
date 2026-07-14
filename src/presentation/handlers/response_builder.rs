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
    /// Response timestamp in RFC3339 format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

impl<T> ApiResponse<T> {
    /// Create a success response with data
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            meta: None,
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
        }
    }

    /// Create a success response with data and pagination
    pub fn success_with_meta(data: T, meta: PaginationMeta) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            meta: Some(meta),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
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

/// Rate limit error response with retry information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitErrorResponse {
    /// Whether the request was successful
    pub success: bool,
    /// Error details
    pub error: ApiError,
    /// Seconds to wait before retrying
    pub retry_after_seconds: u64,
    /// Response timestamp in RFC3339 format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

impl RateLimitErrorResponse {
    /// Create a new rate limit error response
    pub fn new(message: impl Into<String>, retry_after_seconds: u64) -> Self {
        Self {
            success: false,
            error: ApiError {
                code: error_codes::RATE_LIMITED.to_string(),
                message: message.into(),
            },
            retry_after_seconds,
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
        }
    }
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

/// Standard API error response using ApiResponse
#[inline]
pub fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    // Infer error code from status code
    let code = match status {
        StatusCode::BAD_REQUEST => error_codes::VALIDATION_ERROR,
        StatusCode::NOT_FOUND => error_codes::NOT_FOUND,
        StatusCode::UNAUTHORIZED => error_codes::UNAUTHORIZED,
        StatusCode::FORBIDDEN => error_codes::FORBIDDEN,
        StatusCode::TOO_MANY_REQUESTS => error_codes::RATE_LIMITED,
        StatusCode::CONFLICT => error_codes::CONFLICT,
        StatusCode::PRECONDITION_FAILED => error_codes::PRECONDITION_FAILED,
        StatusCode::UNPROCESSABLE_ENTITY => error_codes::UNPROCESSABLE_ENTITY,
        StatusCode::SERVICE_UNAVAILABLE => error_codes::SERVICE_UNAVAILABLE,
        StatusCode::PAYMENT_REQUIRED => error_codes::QUOTA_EXCEEDED,
        _ => error_codes::INTERNAL_ERROR,
    };
    let response: ApiResponse<()> = ApiResponse::error(code, message);
    (status, Json(response)).into_response()
}

/// API error response with error code using ApiResponse
#[inline]
pub fn error_response_with_code(
    status: StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
) -> Response {
    let response: ApiResponse<()> = ApiResponse::error(code, message);
    (status, Json(response)).into_response()
}

/// Standard success response with data using ApiResponse
#[inline]
pub fn success_response<T: serde::Serialize>(status: StatusCode, data: T) -> Response {
    (status, Json(ApiResponse::success(data))).into_response()
}

/// Success response with metadata using ApiResponse
#[inline]
pub fn success_response_with_meta<T: serde::Serialize>(
    status: StatusCode,
    data: T,
    meta: PaginationMeta,
) -> Response {
    (status, Json(ApiResponse::success_with_meta(data, meta))).into_response()
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

    /// Too many requests error response with retry_after using RateLimitErrorResponse
    #[inline]
    pub fn too_many_requests_with_retry(
        message: impl Into<String>,
        retry_after_seconds: u64,
    ) -> Response {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(RateLimitErrorResponse::new(message, retry_after_seconds)),
        )
            .into_response()
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime as ChronoDateTime;

    #[test]
    fn test_api_response_success_has_timestamp() {
        let response = ApiResponse::success("test data");
        assert!(response.success);
        assert_eq!(response.data, Some("test data"));
        assert!(response.timestamp.is_some());
        // Verify timestamp is RFC3339 format
        let ts = response.timestamp.unwrap();
        assert!(ts.contains('-'));
        assert!(ts.contains(':'));
    }

    #[test]
    fn test_api_response_error_has_timestamp() {
        let response: ApiResponse<()> = ApiResponse::error("TEST_ERROR", "Test error message");
        assert!(!response.success);
        assert!(response.error.is_some());
        assert_eq!(response.error.as_ref().unwrap().code, "TEST_ERROR");
        assert!(response.timestamp.is_some());
    }

    #[test]
    fn test_api_response_success_with_meta_has_timestamp() {
        let meta = PaginationMeta::new(1, 10, 100);
        let response = ApiResponse::success_with_meta("data", meta);
        assert!(response.success);
        assert!(response.meta.is_some());
        assert!(response.timestamp.is_some());
    }

    #[test]
    fn test_timestamp_format_is_valid() {
        let response = ApiResponse::success("test");
        let ts_str = response.timestamp.unwrap();
        // Parse as DateTime to verify format
        let parsed = ChronoDateTime::parse_from_rfc3339(&ts_str);
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_timestamp_is_recent() {
        use chrono::Utc;

        let response = ApiResponse::success("test");
        let ts_str = response.timestamp.unwrap();
        let parsed = ChronoDateTime::parse_from_rfc3339(&ts_str).unwrap();
        let now = Utc::now();

        // Timestamp should be within 1 second of now
        let diff = (now.timestamp() - parsed.timestamp()).abs();
        assert!(diff < 2);
    }

    // ========== PaginationMeta tests ==========

    #[test]
    fn test_pagination_meta_new_with_zero_items() {
        let meta = PaginationMeta::new(1, 10, 0);
        assert_eq!(meta.page, 1);
        assert_eq!(meta.per_page, 10);
        assert_eq!(meta.total_items, 0);
        assert_eq!(meta.total_pages, 0);
        assert!(!meta.has_next);
        assert!(!meta.has_previous);
    }

    #[test]
    fn test_pagination_meta_new_exact_pages() {
        // 100 items, 10 per page -> 10 pages
        let meta = PaginationMeta::new(1, 10, 100);
        assert_eq!(meta.total_pages, 10);
        assert!(meta.has_next);
        assert!(!meta.has_previous);
    }

    #[test]
    fn test_pagination_meta_new_partial_last_page() {
        // 95 items, 10 per page -> 10 pages (95/10 = 9.5, ceil = 10)
        let meta = PaginationMeta::new(1, 10, 95);
        assert_eq!(meta.total_pages, 10);
    }

    #[test]
    fn test_pagination_meta_has_next_false_on_last_page() {
        let meta = PaginationMeta::new(10, 10, 100);
        assert_eq!(meta.total_pages, 10);
        assert!(!meta.has_next);
        assert!(meta.has_previous);
    }

    #[test]
    fn test_pagination_meta_has_previous_false_on_first_page() {
        let meta = PaginationMeta::new(1, 10, 100);
        assert!(!meta.has_previous);
    }

    #[test]
    fn test_pagination_meta_has_previous_true_on_page_two() {
        let meta = PaginationMeta::new(2, 10, 100);
        assert!(meta.has_previous);
    }

    #[test]
    fn test_pagination_meta_offset_first_page() {
        let meta = PaginationMeta::new(1, 10, 100);
        assert_eq!(meta.offset(), 0);
    }

    #[test]
    fn test_pagination_meta_offset_second_page() {
        let meta = PaginationMeta::new(2, 10, 100);
        assert_eq!(meta.offset(), 10);
    }

    #[test]
    fn test_pagination_meta_offset_third_page() {
        let meta = PaginationMeta::new(3, 20, 100);
        assert_eq!(meta.offset(), 40);
    }

    #[test]
    fn test_pagination_meta_limit_equals_per_page() {
        let meta = PaginationMeta::new(1, 25, 100);
        assert_eq!(meta.limit(), 25);
    }

    #[test]
    fn test_pagination_meta_div_ceil_one_item_one_per_page() {
        let meta = PaginationMeta::new(1, 1, 1);
        assert_eq!(meta.total_pages, 1);
        assert!(!meta.has_next);
    }

    // ========== IntoResponse status code mapping ==========

    #[tokio::test]
    async fn test_into_response_success_returns_200() {
        let response = ApiResponse::success("data");
        let http_response = response.into_response();
        assert_eq!(http_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_into_response_validation_error_returns_400() {
        let response: ApiResponse<()> = ApiResponse::error("VALIDATION_ERROR", "bad input");
        let http_response = response.into_response();
        assert_eq!(http_response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_into_response_not_found_returns_404() {
        let response: ApiResponse<()> = ApiResponse::error("NOT_FOUND", "missing");
        let http_response = response.into_response();
        assert_eq!(http_response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_into_response_unauthorized_returns_401() {
        let response: ApiResponse<()> = ApiResponse::error("UNAUTHORIZED", "no auth");
        let http_response = response.into_response();
        assert_eq!(http_response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_into_response_forbidden_returns_403() {
        let response: ApiResponse<()> = ApiResponse::error("FORBIDDEN", "denied");
        let http_response = response.into_response();
        assert_eq!(http_response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_into_response_rate_limited_returns_429() {
        let response: ApiResponse<()> = ApiResponse::error("RATE_LIMITED", "slow down");
        let http_response = response.into_response();
        assert_eq!(http_response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_into_response_conflict_returns_409() {
        let response: ApiResponse<()> = ApiResponse::error("CONFLICT", "dup");
        let http_response = response.into_response();
        assert_eq!(http_response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_into_response_precondition_failed_returns_412() {
        let response: ApiResponse<()> = ApiResponse::error("PRECONDITION_FAILED", "precond");
        let http_response = response.into_response();
        assert_eq!(http_response.status(), StatusCode::PRECONDITION_FAILED);
    }

    #[tokio::test]
    async fn test_into_response_unprocessable_entity_returns_422() {
        let response: ApiResponse<()> = ApiResponse::error("UNPROCESSABLE_ENTITY", "unprocessable");
        let http_response = response.into_response();
        assert_eq!(http_response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_into_response_unknown_code_returns_500() {
        let response: ApiResponse<()> = ApiResponse::error("UNKNOWN_CODE", "oops");
        let http_response = response.into_response();
        assert_eq!(http_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_into_response_error_no_error_field_returns_500() {
        // Manually construct a response with success=false but no error
        let response: ApiResponse<()> = ApiResponse {
            success: false,
            data: None,
            error: None,
            meta: None,
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
        };
        let http_response = response.into_response();
        assert_eq!(http_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ========== JSON serialization structure ==========

    #[tokio::test]
    async fn test_success_response_json_structure() {
        let response = ApiResponse::success("hello");
        let http_response = response.into_response();
        let bytes = axum::body::to_bytes(http_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], serde_json::Value::Bool(true));
        assert_eq!(json["data"], serde_json::Value::String("hello".to_string()));
        assert!(json.get("error").is_none() || json["error"].is_null());
        assert!(json.get("meta").is_none() || json["meta"].is_null());
        assert!(json["timestamp"].is_string());
    }

    #[tokio::test]
    async fn test_error_response_json_structure() {
        let response: ApiResponse<()> = ApiResponse::error("NOT_FOUND", "not here");
        let http_response = response.into_response();
        let bytes = axum::body::to_bytes(http_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], serde_json::Value::Bool(false));
        assert!(json.get("data").is_none() || json["data"].is_null());
        assert_eq!(json["error"]["code"], "NOT_FOUND");
        assert_eq!(json["error"]["message"], "not here");
    }

    #[tokio::test]
    async fn test_success_with_meta_json_includes_meta() {
        let meta = PaginationMeta::new(2, 10, 55);
        let response = ApiResponse::success_with_meta(vec!["a", "b"], meta);
        let http_response = response.into_response();
        let bytes = axum::body::to_bytes(http_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["meta"]["page"], 2);
        assert_eq!(json["meta"]["per_page"], 10);
        assert_eq!(json["meta"]["total_items"], 55);
        assert_eq!(json["meta"]["total_pages"], 6);
        assert_eq!(json["meta"]["has_next"], true);
        assert_eq!(json["meta"]["has_previous"], true);
    }

    // ========== error_response function ==========

    #[tokio::test]
    async fn test_error_response_bad_request() {
        let response = error_response(StatusCode::BAD_REQUEST, "bad data");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
        assert_eq!(json["error"]["message"], "bad data");
    }

    #[tokio::test]
    async fn test_error_response_not_found() {
        let response = error_response(StatusCode::NOT_FOUND, "missing");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn test_error_response_unauthorized() {
        let response = error_response(StatusCode::UNAUTHORIZED, "no token");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "UNAUTHORIZED");
    }

    #[tokio::test]
    async fn test_error_response_forbidden() {
        let response = error_response(StatusCode::FORBIDDEN, "denied");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "FORBIDDEN");
    }

    #[tokio::test]
    async fn test_error_response_too_many_requests() {
        let response = error_response(StatusCode::TOO_MANY_REQUESTS, "slow");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "RATE_LIMITED");
    }

    #[tokio::test]
    async fn test_error_response_conflict() {
        let response = error_response(StatusCode::CONFLICT, "dup");
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "CONFLICT");
    }

    #[tokio::test]
    async fn test_error_response_precondition_failed() {
        let response = error_response(StatusCode::PRECONDITION_FAILED, "precond");
        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "PRECONDITION_FAILED");
    }

    #[tokio::test]
    async fn test_error_response_unprocessable_entity() {
        let response = error_response(StatusCode::UNPROCESSABLE_ENTITY, "unprocessable");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "UNPROCESSABLE_ENTITY");
    }

    #[tokio::test]
    async fn test_error_response_service_unavailable() {
        let response = error_response(StatusCode::SERVICE_UNAVAILABLE, "down");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "SERVICE_UNAVAILABLE");
    }

    #[tokio::test]
    async fn test_error_response_payment_required() {
        let response = error_response(StatusCode::PAYMENT_REQUIRED, "pay up");
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "QUOTA_EXCEEDED");
    }

    #[tokio::test]
    async fn test_error_response_internal_error_default() {
        let response = error_response(StatusCode::INTERNAL_SERVER_ERROR, "boom");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "INTERNAL_ERROR");
    }

    // ========== error_response_with_code function ==========

    #[tokio::test]
    async fn test_error_response_with_code_custom() {
        let response =
            error_response_with_code(StatusCode::BAD_REQUEST, "CUSTOM_CODE", "custom msg");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "CUSTOM_CODE");
        assert_eq!(json["error"]["message"], "custom msg");
    }

    #[tokio::test]
    async fn test_error_response_with_code_preserves_status() {
        let response =
            error_response_with_code(StatusCode::NOT_FOUND, "CUSTOM_404", "not found custom");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // ========== success_response function ==========

    #[tokio::test]
    async fn test_success_response_with_data() {
        let response = success_response(StatusCode::OK, vec!["a", "b"]);
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["data"], serde_json::json!(["a", "b"]));
    }

    #[tokio::test]
    async fn test_success_response_with_created_status() {
        let response = success_response(StatusCode::CREATED, "created");
        assert_eq!(response.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["data"], "created");
    }

    // ========== success_response_with_meta function ==========

    #[tokio::test]
    async fn test_success_response_with_meta_includes_pagination() {
        let meta = PaginationMeta::new(1, 5, 12);
        let response = success_response_with_meta(StatusCode::OK, vec![1, 2, 3, 4, 5], meta);
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["meta"]["total_pages"], 3);
        assert_eq!(json["meta"]["has_next"], true);
        assert_eq!(json["data"], serde_json::json!([1, 2, 3, 4, 5]));
    }

    // ========== errors module ==========

    #[tokio::test]
    async fn test_errors_internal_server_error() {
        let response = errors::internal_server_error("internal");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "INTERNAL_ERROR");
        assert_eq!(json["error"]["message"], "internal");
    }

    #[tokio::test]
    async fn test_errors_bad_request() {
        let response = errors::bad_request("bad");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
    }

    #[tokio::test]
    async fn test_errors_not_found() {
        let response = errors::not_found("absent");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "NOT_FOUND");
        assert_eq!(json["error"]["message"], "absent");
    }

    #[tokio::test]
    async fn test_errors_forbidden() {
        let response = errors::forbidden("no access");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "FORBIDDEN");
    }

    #[tokio::test]
    async fn test_errors_unauthorized() {
        let response = errors::unauthorized("no auth");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "UNAUTHORIZED");
    }

    #[tokio::test]
    async fn test_errors_too_many_requests() {
        let response = errors::too_many_requests("slow down");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "RATE_LIMITED");
    }

    #[tokio::test]
    async fn test_errors_too_many_requests_with_retry() {
        let response = errors::too_many_requests_with_retry("rate limited", 60);
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert_eq!(json["error"]["code"], "RATE_LIMITED");
        assert_eq!(json["error"]["message"], "rate limited");
        assert_eq!(json["retry_after_seconds"], 60);
        assert!(json["timestamp"].is_string());
    }

    #[tokio::test]
    async fn test_errors_unprocessable_entity() {
        let response = errors::unprocessable_entity("cannot process");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "UNPROCESSABLE_ENTITY");
    }

    #[tokio::test]
    async fn test_errors_payment_required() {
        let response = errors::payment_required("pay up");
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "QUOTA_EXCEEDED");
    }

    // ========== validation_error & resource_not_found & access_denied ==========

    #[tokio::test]
    async fn test_validation_error_prepends_prefix() {
        let response = validation_error("field required");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
        assert_eq!(json["error"]["message"], "Validation error: field required");
    }

    #[tokio::test]
    async fn test_resource_not_found_appends_suffix() {
        let response = resource_not_found("Task");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "NOT_FOUND");
        assert_eq!(json["error"]["message"], "Task not found");
    }

    #[tokio::test]
    async fn test_access_denied_returns_forbidden() {
        let response = access_denied();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "FORBIDDEN");
        assert_eq!(json["error"]["message"], "Access denied");
    }

    // ========== RateLimitErrorResponse ==========

    #[test]
    fn test_rate_limit_error_response_fields() {
        let resp = RateLimitErrorResponse::new("too many", 30);
        assert!(!resp.success);
        assert_eq!(resp.error.code, "RATE_LIMITED");
        assert_eq!(resp.error.message, "too many");
        assert_eq!(resp.retry_after_seconds, 30);
        assert!(resp.timestamp.is_some());
    }

    #[test]
    fn test_rate_limit_error_response_timestamp_is_rfc3339() {
        let resp = RateLimitErrorResponse::new("slow", 10);
        let ts = resp.timestamp.unwrap();
        assert!(ChronoDateTime::parse_from_rfc3339(&ts).is_ok());
    }

    #[tokio::test]
    async fn test_rate_limit_error_response_serialization() {
        let resp = RateLimitErrorResponse::new("limited", 120);
        let json_str = serde_json::to_string(&resp).unwrap();
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(json["success"], false);
        assert_eq!(json["error"]["code"], "RATE_LIMITED");
        assert_eq!(json["retry_after_seconds"], 120);
    }

    // ========== ApiError struct ==========

    #[test]
    fn test_api_error_serialization() {
        let err = ApiError {
            code: "TEST_CODE".to_string(),
            message: "test message".to_string(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("TEST_CODE"));
        assert!(json.contains("test message"));
    }

    #[test]
    fn test_api_error_deserialization() {
        let json = r#"{"code":"ERR","message":"msg"}"#;
        let err: ApiError = serde_json::from_str(json).unwrap();
        assert_eq!(err.code, "ERR");
        assert_eq!(err.message, "msg");
    }

    // ========== error_codes module constants ==========

    #[test]
    fn test_error_codes_validation_error() {
        assert_eq!(error_codes::VALIDATION_ERROR, "VALIDATION_ERROR");
    }

    #[test]
    fn test_error_codes_not_found() {
        assert_eq!(error_codes::NOT_FOUND, "NOT_FOUND");
    }

    #[test]
    fn test_error_codes_unauthorized() {
        assert_eq!(error_codes::UNAUTHORIZED, "UNAUTHORIZED");
    }

    #[test]
    fn test_error_codes_forbidden() {
        assert_eq!(error_codes::FORBIDDEN, "FORBIDDEN");
    }

    #[test]
    fn test_error_codes_rate_limited() {
        assert_eq!(error_codes::RATE_LIMITED, "RATE_LIMITED");
    }

    #[test]
    fn test_error_codes_conflict() {
        assert_eq!(error_codes::CONFLICT, "CONFLICT");
    }

    #[test]
    fn test_error_codes_internal_error() {
        assert_eq!(error_codes::INTERNAL_ERROR, "INTERNAL_ERROR");
    }

    #[test]
    fn test_error_codes_service_unavailable() {
        assert_eq!(error_codes::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE");
    }

    #[test]
    fn test_error_codes_quota_exceeded() {
        assert_eq!(error_codes::QUOTA_EXCEEDED, "QUOTA_EXCEEDED");
    }

    #[test]
    fn test_error_codes_database_error() {
        assert_eq!(error_codes::DATABASE_ERROR, "DATABASE_ERROR");
    }

    #[test]
    fn test_error_codes_feature_disabled() {
        assert_eq!(error_codes::FEATURE_DISABLED, "FEATURE_DISABLED");
    }

    // ========== ApiResponse skip_serializing_if ==========

    #[test]
    fn test_api_response_error_field_skipped_when_none() {
        let response = ApiResponse::success("data");
        let json = serde_json::to_string(&response).unwrap();
        // error field should be skipped because it's None
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_api_response_meta_field_skipped_when_none() {
        let response: ApiResponse<()> = ApiResponse::error("ERR", "msg");
        let json = serde_json::to_string(&response).unwrap();
        // meta field should be skipped because it's None
        assert!(!json.contains("\"meta\""));
    }

    #[test]
    fn test_api_response_meta_present_when_some() {
        let meta = PaginationMeta::new(1, 10, 50);
        let response = ApiResponse::success_with_meta("data", meta);
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"meta\""));
    }

    // ========== ApiResponse round-trip deserialization ==========

    #[test]
    fn test_api_response_round_trip_success() {
        let original = ApiResponse::success(vec![1, 2, 3]);
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: ApiResponse<Vec<i32>> = serde_json::from_str(&json).unwrap();
        assert!(deserialized.success);
        assert_eq!(deserialized.data, Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_pagination_meta_round_trip() {
        let original = PaginationMeta::new(3, 15, 42);
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: PaginationMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.page, original.page);
        assert_eq!(deserialized.per_page, original.per_page);
        assert_eq!(deserialized.total_items, original.total_items);
        assert_eq!(deserialized.total_pages, original.total_pages);
        assert_eq!(deserialized.has_next, original.has_next);
        assert_eq!(deserialized.has_previous, original.has_previous);
    }

    // ========== 缺失的 error_codes 常量测试 ==========

    #[test]
    fn test_error_codes_precondition_failed() {
        assert_eq!(error_codes::PRECONDITION_FAILED, "PRECONDITION_FAILED");
    }

    #[test]
    fn test_error_codes_unprocessable_entity() {
        assert_eq!(error_codes::UNPROCESSABLE_ENTITY, "UNPROCESSABLE_ENTITY");
    }

    #[test]
    fn test_error_codes_cache_error() {
        assert_eq!(error_codes::CACHE_ERROR, "CACHE_ERROR");
    }

    #[test]
    fn test_error_codes_external_service_error() {
        assert_eq!(
            error_codes::EXTERNAL_SERVICE_ERROR,
            "EXTERNAL_SERVICE_ERROR"
        );
    }

    #[test]
    fn test_error_codes_timeout() {
        assert_eq!(error_codes::TIMEOUT, "TIMEOUT");
    }

    // ========== RateLimitErrorResponse 反序列化 ==========

    #[test]
    fn test_rate_limit_error_response_deserialization() {
        let json = r#"{"success":false,"error":{"code":"RATE_LIMITED","message":"slow down"},"retry_after_seconds":45,"timestamp":"2025-01-01T00:00:00Z"}"#;
        let resp: RateLimitErrorResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.success);
        assert_eq!(resp.error.code, "RATE_LIMITED");
        assert_eq!(resp.error.message, "slow down");
        assert_eq!(resp.retry_after_seconds, 45);
        assert!(resp.timestamp.is_some());
    }

    #[test]
    fn test_rate_limit_error_response_skip_timestamp_when_none() {
        // timestamp 为 None 时序列化结果不应包含 timestamp 字段
        let resp = RateLimitErrorResponse {
            success: false,
            error: ApiError {
                code: "RATE_LIMITED".to_string(),
                message: "test".to_string(),
            },
            retry_after_seconds: 10,
            timestamp: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(!json.contains("timestamp"));
    }
}
