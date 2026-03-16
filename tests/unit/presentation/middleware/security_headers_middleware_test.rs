#![cfg(test)]
// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware,
    routing::get,
    Router,
};
use tower::ServiceExt;

/// Helper function to create a test app with security headers middleware
fn create_test_app() -> Router {
    Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/api/test", get(|| async { "{\"status\": \"ok\"}" }))
        .layer(middleware::from_fn(
            crawlrs::presentation::middleware::security_headers_middleware::security_headers_middleware,
        ))
}

#[tokio::test]
async fn test_x_content_type_options_header() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);

    let header = response
        .headers()
        .get("x-content-type-options")
        .expect("X-Content-Type-Options header should be present");
    assert_eq!(header, "nosniff");
}

#[tokio::test]
async fn test_x_frame_options_header() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);

    let header = response
        .headers()
        .get("x-frame-options")
        .expect("X-Frame-Options header should be present");
    assert_eq!(header, "DENY");
}

#[tokio::test]
async fn test_x_xss_protection_header() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);

    let header = response
        .headers()
        .get("x-xss-protection")
        .expect("X-XSS-Protection header should be present");
    assert_eq!(header, "1; mode=block");
}

#[tokio::test]
async fn test_content_security_policy_header() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);

    let header = response
        .headers()
        .get("content-security-policy")
        .expect("Content-Security-Policy header should be present");

    let csp_value = header.to_str().expect("CSP header should be valid UTF-8");

    // Verify all required CSP directives are present
    assert!(
        csp_value.contains("default-src 'self'"),
        "CSP should contain default-src 'self'"
    );
    assert!(
        csp_value.contains("script-src 'self'"),
        "CSP should contain script-src 'self'"
    );
    assert!(
        csp_value.contains("style-src 'self'"),
        "CSP should contain style-src 'self'"
    );
    assert!(
        csp_value.contains("img-src 'self' data:"),
        "CSP should contain img-src 'self' data:"
    );
    assert!(
        csp_value.contains("font-src 'self'"),
        "CSP should contain font-src 'self'"
    );
    assert!(
        csp_value.contains("object-src 'none'"),
        "CSP should contain object-src 'none'"
    );
    assert!(
        csp_value.contains("base-uri 'self'"),
        "CSP should contain base-uri 'self'"
    );
    assert!(
        csp_value.contains("form-action 'self'"),
        "CSP should contain form-action 'self'"
    );
    assert!(
        csp_value.contains("frame-ancestors 'none'"),
        "CSP should contain frame-ancestors 'none'"
    );
}

#[tokio::test]
async fn test_referrer_policy_header() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);

    let header = response
        .headers()
        .get("referrer-policy")
        .expect("Referrer-Policy header should be present");
    assert_eq!(header, "strict-origin-when-cross-origin");
}

#[tokio::test]
async fn test_permissions_policy_header() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);

    let header = response
        .headers()
        .get("permissions-policy")
        .expect("Permissions-Policy header should be present");

    let policy_value = header.to_str().expect("Permissions-Policy header should be valid UTF-8");

    // Verify all sensitive features are disabled
    assert!(
        policy_value.contains("accelerometer=()"),
        "Permissions-Policy should disable accelerometer"
    );
    assert!(
        policy_value.contains("camera=()"),
        "Permissions-Policy should disable camera"
    );
    assert!(
        policy_value.contains("geolocation=()"),
        "Permissions-Policy should disable geolocation"
    );
    assert!(
        policy_value.contains("gyroscope=()"),
        "Permissions-Policy should disable gyroscope"
    );
    assert!(
        policy_value.contains("magnetometer=()"),
        "Permissions-Policy should disable magnetometer"
    );
    assert!(
        policy_value.contains("microphone=()"),
        "Permissions-Policy should disable microphone"
    );
    assert!(
        policy_value.contains("payment=()"),
        "Permissions-Policy should disable payment"
    );
    assert!(
        policy_value.contains("usb=()"),
        "Permissions-Policy should disable usb"
    );
}

#[tokio::test]
async fn test_cross_origin_opener_policy_header() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);

    let header = response
        .headers()
        .get("cross-origin-opener-policy")
        .expect("Cross-Origin-Opener-Policy header should be present");
    assert_eq!(header, "same-origin");
}

#[tokio::test]
async fn test_cross_origin_resource_policy_header() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);

    let header = response
        .headers()
        .get("cross-origin-resource-policy")
        .expect("Cross-Origin-Resource-Policy header should be present");
    assert_eq!(header, "same-origin");
}

#[tokio::test]
async fn test_hsts_header_not_added_for_http() {
    let app = create_test_app();

    // HTTP request (default scheme)
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);

    // HSTS header should NOT be present for HTTP
    let hsts_header = response.headers().get("strict-transport-security");
    assert!(
        hsts_header.is_none(),
        "HSTS header should not be present for HTTP requests"
    );
}

#[tokio::test]
async fn test_hsts_header_added_for_https() {
    let app = create_test_app();

    // HTTPS request
    let response = app
        .oneshot(
            Request::builder()
                .uri("https://localhost/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);

    // HSTS header should be present for HTTPS
    let header = response
        .headers()
        .get("strict-transport-security")
        .expect("HSTS header should be present for HTTPS requests");

    let hsts_value = header.to_str().expect("HSTS header should be valid UTF-8");

    // Verify HSTS directives
    assert!(
        hsts_value.contains("max-age=31536000"),
        "HSTS should have max-age of 1 year"
    );
    assert!(
        hsts_value.contains("includeSubDomains"),
        "HSTS should include includeSubDomains directive"
    );
}

#[tokio::test]
async fn test_all_security_headers_present() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);

    let headers = response.headers();

    // Verify all required security headers are present
    let required_headers = [
        "x-content-type-options",
        "x-frame-options",
        "x-xss-protection",
        "content-security-policy",
        "referrer-policy",
        "permissions-policy",
        "cross-origin-opener-policy",
        "cross-origin-resource-policy",
    ];

    for header_name in &required_headers {
        assert!(
            headers.contains_key(*header_name),
            "Required security header '{}' should be present",
            header_name
        );
    }
}

#[tokio::test]
async fn test_security_headers_on_different_routes() {
    let app = create_test_app();

    // Test on root route
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert!(response.headers().contains_key("x-content-type-options"));
    assert!(response.headers().contains_key("x-frame-options"));

    // Test on API route
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/test")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert!(response.headers().contains_key("x-content-type-options"));
    assert!(response.headers().contains_key("x-frame-options"));
    assert!(response.headers().contains_key("content-security-policy"));
}

#[tokio::test]
async fn test_csp_prevents_inline_scripts() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    let csp = response
        .headers()
        .get("content-security-policy")
        .expect("CSP header should be present")
        .to_str()
        .expect("CSP should be valid UTF-8");

    // Verify unsafe-inline is NOT present (security best practice)
    assert!(
        !csp.contains("'unsafe-inline'"),
        "CSP should NOT contain 'unsafe-inline' for security"
    );

    // Verify unsafe-eval is NOT present
    assert!(
        !csp.contains("'unsafe-eval'"),
        "CSP should NOT contain 'unsafe-eval' for security"
    );
}

#[tokio::test]
async fn test_object_src_none() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    let csp = response
        .headers()
        .get("content-security-policy")
        .expect("CSP header should be present")
        .to_str()
        .expect("CSP should be valid UTF-8");

    // Verify object-src is set to 'none' to prevent plugins
    assert!(
        csp.contains("object-src 'none'"),
        "CSP should set object-src to 'none' to prevent plugin-based attacks"
    );
}

#[tokio::test]
async fn test_frame_ancestors_none() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    let csp = response
        .headers()
        .get("content-security-policy")
        .expect("CSP header should be present")
        .to_str()
        .expect("CSP should be valid UTF-8");

    // Verify frame-ancestors is set to 'none' for clickjacking protection
    assert!(
        csp.contains("frame-ancestors 'none'"),
        "CSP should set frame-ancestors to 'none' for clickjacking protection"
    );
}
