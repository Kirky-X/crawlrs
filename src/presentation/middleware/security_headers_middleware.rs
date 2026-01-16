// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Security headers middleware
//!
//! Adds security-related HTTP headers to all responses:
//! - X-Content-Type-Options: nosniff
//! - X-Frame-Options: DENY
//! - X-XSS-Protection: 1; mode=block
//! - Strict-Transport-Security (HTTPS only)
//! - Content-Security-Policy

use axum::{
    http::{HeaderValue, Response},
    middleware::Next,
    Request,
};
use std::time::Duration;

/// Add security headers to response
pub async fn security_headers_middleware(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;

    // X-Content-Type-Options: nosniff
    // Prevents MIME type sniffing
    if let Some(value) = response.headers_mut().get_mut("x-content-type-options") {
        *value = HeaderValue::from_static("nosniff");
    } else {
        response.headers_mut().insert(
            "x-content-type-options",
            HeaderValue::from_static("nosniff"),
        );
    }

    // X-Frame-Options: DENY
    // Prevents clickjacking attacks
    if let Some(value) = response.headers_mut().get_mut("x-frame-options") {
        *value = HeaderValue::from_static("DENY");
    } else {
        response
            .headers_mut()
            .insert("x-frame-options", HeaderValue::from_static("DENY"));
    }

    // X-XSS-Protection: 1; mode=block
    // Enables XSS filtering in older browsers
    if let Some(value) = response.headers_mut().get_mut("x-xss-protection") {
        *value = HeaderValue::from_static("1; mode=block");
    } else {
        response.headers_mut().insert(
            "x-xss-protection",
            HeaderValue::from_static("1; mode=block"),
        );
    }

    // Content-Security-Policy
    // Restricts resource loading to same origin
    // Note: 'unsafe-inline' removed for better XSS protection
    // If inline styles are needed, consider using nonces or hashes
    if let Some(value) = response.headers_mut().get_mut("content-security-policy") {
        *value =
            HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'");
    } else {
        response.headers_mut().insert(
            "content-security-policy",
            HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'"),
        );
    }

    // Strict-Transport-Security (HSTS)
    // Only add for HTTPS connections
    // max-age = 1 year (31536000 seconds)
    // includeSubDomains directive
    let uri = response.extensions().get::<http::Uri>().cloned();
    let is_https = uri
        .as_ref()
        .map(|u| u.scheme() == Some(&http::uri::Scheme::HTTPS))
        .unwrap_or(false);

    if is_https {
        if let Some(value) = response.headers_mut().get_mut("strict-transport-security") {
            *value = HeaderValue::from_static("max-age=31536000; includeSubDomains");
        } else {
            response.headers_mut().insert(
                "strict-transport-security",
                HeaderValue::from_static("max-age=31536000; includeSubDomains"),
            );
        }
    }

    response
}
