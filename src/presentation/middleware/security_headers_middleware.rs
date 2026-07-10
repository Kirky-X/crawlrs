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
    body::Body,
    http::{uri::Scheme, HeaderValue, Request, Response},
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
pub async fn security_headers_middleware(req: Request<Body>, next: Next) -> Response<Body> {
    // Extract URI from request before passing to next
    let uri = req.uri().clone();
    let is_https = uri.scheme() == Some(&Scheme::HTTPS);

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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::get,
        Router,
    };
    use tower::ServiceExt;

    /// Build a test router with the security headers middleware applied
    fn test_router() -> Router {
        Router::new()
            .route("/test", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(security_headers_middleware))
    }

    /// Helper to extract a header value as a string
    fn header_str<'a>(response: &'a axum::http::Response<Body>, name: &str) -> &'a str {
        response
            .headers()
            .get(name)
            .expect("header should be present")
            .to_str()
            .expect("header value should be valid utf-8")
    }

    #[tokio::test]
    async fn test_x_content_type_options_header_present() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header_str(&response, "x-content-type-options"), "nosniff");
    }

    #[tokio::test]
    async fn test_x_frame_options_header_present() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(header_str(&response, "x-frame-options"), "DENY");
    }

    #[tokio::test]
    async fn test_x_xss_protection_header_present() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(header_str(&response, "x-xss-protection"), "1; mode=block");
    }

    #[tokio::test]
    async fn test_content_security_policy_header_present() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let csp = header_str(&response, "content-security-policy");
        assert!(csp.contains("default-src 'self'"));
        assert!(csp.contains("script-src 'self'"));
        assert!(csp.contains("object-src 'none'"));
        assert!(csp.contains("frame-ancestors 'none'"));
    }

    #[tokio::test]
    async fn test_referrer_policy_header_present() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            header_str(&response, "referrer-policy"),
            "strict-origin-when-cross-origin"
        );
    }

    #[tokio::test]
    async fn test_permissions_policy_header_present() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let policy = header_str(&response, "permissions-policy");
        assert!(policy.contains("camera=()"));
        assert!(policy.contains("microphone=()"));
        assert!(policy.contains("geolocation=()"));
    }

    #[tokio::test]
    async fn test_cross_origin_opener_policy_header_present() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            header_str(&response, "cross-origin-opener-policy"),
            "same-origin"
        );
    }

    #[tokio::test]
    async fn test_cross_origin_resource_policy_header_present() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            header_str(&response, "cross-origin-resource-policy"),
            "same-origin"
        );
    }

    #[tokio::test]
    async fn test_hsts_header_absent_for_http() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        // HTTP request should NOT have HSTS header
        assert!(response
            .headers()
            .get("strict-transport-security")
            .is_none());
    }

    #[tokio::test]
    async fn test_hsts_header_present_for_https() {
        let app = test_router();
        // Build an HTTPS URI
        let response = app
            .oneshot(
                Request::builder()
                    .uri("https://example.com/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let hsts = header_str(&response, "strict-transport-security");
        assert!(hsts.contains("max-age=31536000"));
        assert!(hsts.contains("includeSubDomains"));
    }

    #[tokio::test]
    async fn test_all_security_headers_present_for_http() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let headers = response.headers();
        // Verify all expected headers are present (except HSTS which is HTTPS-only)
        assert!(headers.contains_key("x-content-type-options"));
        assert!(headers.contains_key("x-frame-options"));
        assert!(headers.contains_key("x-xss-protection"));
        assert!(headers.contains_key("content-security-policy"));
        assert!(headers.contains_key("referrer-policy"));
        assert!(headers.contains_key("permissions-policy"));
        assert!(headers.contains_key("cross-origin-opener-policy"));
        assert!(headers.contains_key("cross-origin-resource-policy"));
    }

    #[tokio::test]
    async fn test_all_security_headers_present_for_https() {
        let app = test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("https://example.com/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let headers = response.headers();
        // For HTTPS, HSTS should also be present
        assert!(headers.contains_key("x-content-type-options"));
        assert!(headers.contains_key("x-frame-options"));
        assert!(headers.contains_key("x-xss-protection"));
        assert!(headers.contains_key("content-security-policy"));
        assert!(headers.contains_key("referrer-policy"));
        assert!(headers.contains_key("permissions-policy"));
        assert!(headers.contains_key("cross-origin-opener-policy"));
        assert!(headers.contains_key("cross-origin-resource-policy"));
        assert!(headers.contains_key("strict-transport-security"));
    }

    #[tokio::test]
    async fn test_middleware_preserves_response_body() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&bytes[..], b"ok");
    }

    #[tokio::test]
    async fn test_csp_policy_constant_value() {
        assert!(CSP_POLICY.contains("default-src 'self'"));
        assert!(CSP_POLICY.contains("base-uri 'self'"));
        assert!(CSP_POLICY.contains("form-action 'self'"));
    }

    #[tokio::test]
    async fn test_permissions_policy_constant_value() {
        assert!(PERMISSIONS_POLICY.contains("accelerometer=()"));
        assert!(PERMISSIONS_POLICY.contains("payment=()"));
        assert!(PERMISSIONS_POLICY.contains("usb=()"));
    }

    #[tokio::test]
    async fn test_hsts_value_constant() {
        assert!(HSTS_VALUE.contains("max-age=31536000"));
        assert!(HSTS_VALUE.contains("includeSubDomains"));
    }
}
