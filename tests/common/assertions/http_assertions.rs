// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(dead_code)]

/// HTTP 断言模块
///
/// 提供常用的 HTTP 响应断言
use axum::http::StatusCode;
use axum_test::TestResponse;

/// HTTP 断言辅助函数
pub struct HttpAssertions;

impl HttpAssertions {
    /// 断言 201 Created
    pub fn assert_created(response: &TestResponse) {
        assert_eq!(
            response.status_code(),
            StatusCode::CREATED,
            "Expected 201 Created, got {}",
            response.status_code()
        );
    }

    /// 断言 202 Accepted
    pub fn assert_accepted(response: &TestResponse) {
        assert_eq!(
            response.status_code(),
            StatusCode::ACCEPTED,
            "Expected 202 Accepted, got {}",
            response.status_code()
        );
    }

    /// 断言 201 Created 或 202 Accepted
    pub fn assert_created_or_accepted(response: &TestResponse) {
        let status = response.status_code();
        assert!(
            status == StatusCode::CREATED || status == StatusCode::ACCEPTED,
            "Expected 201 or 202, got {}",
            status
        );
    }

    /// 断言 400 Bad Request
    pub fn assert_bad_request(response: &TestResponse) {
        assert_eq!(
            response.status_code(),
            StatusCode::BAD_REQUEST,
            "Expected 400 Bad Request, got {}",
            response.status_code()
        );
    }

    /// 断言 401 Unauthorized
    pub fn assert_unauthorized(response: &TestResponse) {
        assert_eq!(
            response.status_code(),
            StatusCode::UNAUTHORIZED,
            "Expected 401 Unauthorized, got {}",
            response.status_code()
        );
    }

    /// 断言 403 Forbidden
    pub fn assert_forbidden(response: &TestResponse) {
        assert_eq!(
            response.status_code(),
            StatusCode::FORBIDDEN,
            "Expected 403 Forbidden, got {}",
            response.status_code()
        );
    }

    /// 断言 404 Not Found
    pub fn assert_not_found(response: &TestResponse) {
        assert_eq!(
            response.status_code(),
            StatusCode::NOT_FOUND,
            "Expected 404 Not Found, got {}",
            response.status_code()
        );
    }

    /// 断言 429 Too Many Requests
    pub fn assert_too_many_requests(response: &TestResponse) {
        assert_eq!(
            response.status_code(),
            StatusCode::TOO_MANY_REQUESTS,
            "Expected 429 Too Many Requests, got {}",
            response.status_code()
        );
    }

    /// 断言成功状态 (2xx)
    pub fn assert_success(response: &TestResponse) {
        let status = response.status_code();
        assert!(
            status.is_success(),
            "Expected success status (2xx), got {}",
            status
        );
    }

    /// 断言服务器错误 (5xx)
    pub fn assert_server_error(response: &TestResponse) {
        let status = response.status_code();
        assert!(
            status.is_server_error(),
            "Expected server error (5xx), got {}",
            status
        );
    }
}
