// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::http::StatusCode;
use axum_test::TestResponse;

pub struct ResponseHelpers;

impl ResponseHelpers {
    pub fn is_success_or_accepted(status: StatusCode) -> bool {
        status == StatusCode::CREATED || status == StatusCode::ACCEPTED || status.is_success()
    }

    pub fn assert_any_of(response: &TestResponse, expected: &[StatusCode], context: &str) {
        let status = response.status_code();
        assert!(
            expected.contains(&status),
            "Expected {} for {}, got {}",
            expected
                .iter()
                .map(|s| s.as_u16().to_string())
                .collect::<Vec<_>>()
                .join(" or "),
            context,
            status
        );
    }

    pub fn assert_created_or_accepted(response: &TestResponse) {
        Self::assert_any_of(
            response,
            &[StatusCode::CREATED, StatusCode::ACCEPTED],
            "scrape request",
        );
    }

    pub fn assert_success_or_rate_limited(response: &TestResponse) {
        Self::assert_any_of(
            response,
            &[
                StatusCode::CREATED,
                StatusCode::ACCEPTED,
                StatusCode::TOO_MANY_REQUESTS,
            ],
            "request",
        );
    }
}
