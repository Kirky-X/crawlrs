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
//! - Referrer-Policy
//! - Permissions-Policy
//! - Cross-Origin-Opener-Policy
//! - Cross-Origin-Resource-Policy

use axum::{
    http::{HeaderValue, Request, Response},
    middleware::Next,
};

/// Enhanced Content-Security-Policy value
/// Includes comprehensive restrictions for better XSS protection
const CSP_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'";

/// Permissions-Policy value - disables sensitive browser features
const PERMISSIONS_POLICY: &str = "accelerometer=(), camera=(), geolocation=(), gyroscope=(), magnetometer=(), microphone=(), payment=(), usb=()";

/// HSTS header value - 1 year max-age with includeSubDomains
const HSTS_VALUE: &str = "max-age=31536000; includeSubDomains";

/// Add security headers to response
pub async fn security_headers_middleware(req: Request, next: Next) -> Response {
    // Extract URI from request before passing to next
    let uri = req.uri().clone();
    let is_https = uri.scheme() == Some(&http::uri::Scheme::HTTPS);

    let mut response = next.run(req).await;

    // X-Content-Type-Options: nosniff
    // Prevents MIME type sniffing attacks
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );

    // X-Frame-Options: DENY
    // Prevents clickjacking attacks by disallowing framing
    response
        .headers_mut()
        .insert("x-frame-options", HeaderValue::from_static("DENY"));

    // X-XSS-Protection: 1; mode=block
    // Enables XSS filtering in older browsers (legacy but still useful)
    response.headers_mut().insert(
        "x-xss-protection",
        HeaderValue::from_static("1; mode=block"),
    );

    // Content-Security-Policy
    // Comprehensive CSP to prevent XSS and data injection attacks
    response.headers_mut().insert(
        "content-security-policy",
        HeaderValue::from_static(CSP_POLICY),
    );

    // Referrer-Policy: strict-origin-when-cross-origin
    // Controls how much referrer information is sent with requests
    // - Same origin: sends full URL
    // - Cross-origin HTTPS->HTTPS: sends origin only
    // - Cross-origin HTTPS->HTTP: sends nothing (prevents leakage)
    response.headers_mut().insert(
        "referrer-policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );

    // Permissions-Policy
    // Disables access to sensitive browser features (camera, microphone, etc.)
    response.headers_mut().insert(
        "permissions-policy",
        HeaderValue::from_static(PERMISSIONS_POLICY),
    );

    // Cross-Origin-Opener-Policy: same-origin
    // Protects against cross-origin attacks by isolating browsing context
    response.headers_mut().insert(
        "cross-origin-opener-policy",
        HeaderValue::from_static("same-origin"),
    );

    // Cross-Origin-Resource-Policy: same-origin
    // Prevents cross-origin resources from being loaded by other origins
    response.headers_mut().insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );

    // Strict-Transport-Security (HSTS)
    // Only add for HTTPS connections to force secure connections
    // max-age = 1 year (31536000 seconds)
    // includeSubDomains directive applies to all subdomains
    if is_https {
        response.headers_mut().insert(
            "strict-transport-security",
            HeaderValue::from_static(HSTS_VALUE),
        );
    }

    response
}
