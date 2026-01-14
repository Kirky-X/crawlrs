// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(dead_code)]

use axum::http::StatusCode;
use axum_test::TestResponse;

pub struct HttpAssertions;

impl HttpAssertions {
    fn assert_status(response: &TestResponse, expected: StatusCode, name: &str) {
        assert_eq!(
            response.status_code(),
            expected,
            "Expected {}, got {}",
            name,
            response.status_code()
        );
    }

    fn assert_status_in(response: &TestResponse, expected: &[StatusCode], name: &str) {
        let status = response.status_code();
        assert!(
            expected.contains(&status),
            "Expected {}, got {}",
            name,
            status
        );
    }

    pub fn assert_created(response: &TestResponse) {
        Self::assert_status(response, StatusCode::CREATED, "201 Created");
    }

    pub fn assert_accepted(response: &TestResponse) {
        Self::assert_status(response, StatusCode::ACCEPTED, "202 Accepted");
    }

    pub fn assert_created_or_accepted(response: &TestResponse) {
        Self::assert_status_in(
            response,
            &[StatusCode::CREATED, StatusCode::ACCEPTED],
            "201 or 202",
        );
    }

    pub fn assert_bad_request(response: &TestResponse) {
        Self::assert_status(response, StatusCode::BAD_REQUEST, "400 Bad Request");
    }

    pub fn assert_unauthorized(response: &TestResponse) {
        Self::assert_status(response, StatusCode::UNAUTHORIZED, "401 Unauthorized");
    }

    pub fn assert_forbidden(response: &TestResponse) {
        Self::assert_status(response, StatusCode::FORBIDDEN, "403 Forbidden");
    }

    pub fn assert_not_found(response: &TestResponse) {
        Self::assert_status(response, StatusCode::NOT_FOUND, "404 Not Found");
    }

    pub fn assert_too_many_requests(response: &TestResponse) {
        Self::assert_status(response, StatusCode::TOO_MANY_REQUESTS, "429 Too Many Requests");
    }

    pub fn assert_success(response: &TestResponse) {
        let status = response.status_code();
        assert!(
            status.is_success(),
            "Expected success status (2xx), got {}",
            status
        );
    }

    pub fn assert_server_error(response: &TestResponse) {
        let status = response.status_code();
        assert!(
            status.is_server_error(),
            "Expected server error (5xx), got {}",
            status
        );
    }
}
