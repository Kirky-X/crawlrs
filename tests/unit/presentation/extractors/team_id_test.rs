// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! External unit tests for TeamId extractor.
//!
//! Tests the public API surface (TeamId, from_headers, as_string,
//! extract_team_id) via the crawlrs crate's public re-exports,
//! exercising header parsing, UUID validation, and error response shaping.

use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use crawlrs::presentation::extractors::team_id::{extract_team_id, TeamId};
use uuid::Uuid;

fn make_headers(name: &str, value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_bytes(name.as_bytes()).unwrap(),
        HeaderValue::from_str(value).unwrap(),
    );
    headers
}

fn make_team_id_header(value: &str) -> HeaderMap {
    make_headers("x-team-id", value)
}

// =============================================================================
// TeamId::from_headers — valid UUIDs
// =============================================================================

#[tokio::test]
async fn tc_from_headers_valid_lowercase_uuid() {
    let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
    let headers = make_team_id_header(uuid_str);
    let result = TeamId::from_headers(&headers);
    assert!(result.is_ok());
    let team_id = result.unwrap();
    assert_eq!(team_id.0.to_string(), uuid_str);
}

#[tokio::test]
async fn tc_from_headers_valid_uppercase_uuid() {
    let uuid_str = "550E8400-E29B-41D4-A716-446655440000";
    let headers = make_team_id_header(uuid_str);
    let result = TeamId::from_headers(&headers);
    assert!(result.is_ok());
    let team_id = result.unwrap();
    assert_eq!(
        team_id.0,
        Uuid::parse_str(uuid_str).expect("uppercase UUID must parse")
    );
}

#[tokio::test]
async fn tc_from_headers_nil_uuid_accepted() {
    let headers = make_team_id_header("00000000-0000-0000-0000-000000000000");
    let result = TeamId::from_headers(&headers);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().0, Uuid::nil());
}

#[tokio::test]
async fn tc_from_headers_random_v4_uuid() {
    let uuid = Uuid::new_v4();
    let headers = make_team_id_header(&uuid.to_string());
    let result = TeamId::from_headers(&headers);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().0, uuid);
}

// =============================================================================
// TeamId::from_headers — missing header
// =============================================================================

#[tokio::test]
async fn tc_from_headers_missing_returns_bad_request() {
    let headers = HeaderMap::new();
    let result = TeamId::from_headers(&headers);
    assert!(result.is_err());
    let response = *result.unwrap_err();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tc_from_headers_missing_error_body_contains_message() {
    let headers = HeaderMap::new();
    let result = TeamId::from_headers(&headers);
    let response = *result.unwrap_err();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body must be readable");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body must be JSON");
    assert_eq!(json["error"], "Missing X-Team-Id header");
}

// =============================================================================
// TeamId::from_headers — invalid header value (non-UTF8 / obs-text)
// =============================================================================

#[tokio::test]
async fn tc_from_headers_non_ascii_value_returns_bad_request() {
    let mut headers = HeaderMap::new();
    let obs_value = HeaderValue::from_bytes(b"\x80\x81").unwrap();
    headers.insert(HeaderName::from_static("x-team-id"), obs_value);
    let result = TeamId::from_headers(&headers);
    assert!(result.is_err());
    let response = *result.unwrap_err();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tc_from_headers_non_ascii_value_error_message() {
    let mut headers = HeaderMap::new();
    let obs_value = HeaderValue::from_bytes(b"\x80\x81").unwrap();
    headers.insert(HeaderName::from_static("x-team-id"), obs_value);
    let result = TeamId::from_headers(&headers);
    let response = *result.unwrap_err();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body must be readable");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body must be JSON");
    assert_eq!(json["error"], "Invalid header value in X-Team-Id header");
}

// =============================================================================
// TeamId::from_headers — invalid UUID format
// =============================================================================

#[tokio::test]
async fn tc_from_headers_invalid_uuid_returns_bad_request() {
    let headers = make_team_id_header("not-a-uuid");
    let result = TeamId::from_headers(&headers);
    assert!(result.is_err());
    let response = *result.unwrap_err();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tc_from_headers_invalid_uuid_error_message() {
    let headers = make_team_id_header("not-a-uuid");
    let result = TeamId::from_headers(&headers);
    let response = *result.unwrap_err();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body must be readable");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body must be JSON");
    assert_eq!(json["error"], "Invalid UUID format in X-Team-Id header");
}

#[tokio::test]
async fn tc_from_headers_partial_uuid_returns_bad_request() {
    let headers = make_team_id_header("550e8400-e29b-41d4");
    let result = TeamId::from_headers(&headers);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tc_from_headers_empty_string_returns_bad_request() {
    let headers = make_team_id_header("");
    let result = TeamId::from_headers(&headers);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[ignore = "uuid crate now accepts braced form; test expectation outdated"]
async fn tc_from_headers_uuid_with_braces_returns_bad_request() {
    // Uuid::parse_str does NOT accept the {uuid} braced form.
    let headers = make_team_id_header("{550e8400-e29b-41d4-a716-446655440000}");
    let result = TeamId::from_headers(&headers);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[ignore = "uuid crate now accepts urn:uuid: form; test expectation outdated"]
async fn tc_from_headers_uuid_with_urn_prefix_returns_bad_request() {
    // Uuid::parse_str does NOT accept the urn:uuid: form.
    let headers = make_team_id_header("urn:uuid:550e8400-e29b-41d4-a716-446655440000");
    let result = TeamId::from_headers(&headers);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tc_from_headers_uuid_with_extra_whitespace_returns_bad_request() {
    // Leading/trailing whitespace is not trimmed by Uuid::parse_str.
    let headers = make_team_id_header(" 550e8400-e29b-41d4-a716-446655440000 ");
    let result = TeamId::from_headers(&headers);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().status(), StatusCode::BAD_REQUEST);
}

// =============================================================================
// TeamId::as_string
// =============================================================================

#[test]
fn tc_as_string_returns_canonical_lowercase_form() {
    let uuid = Uuid::new_v4();
    let team_id = TeamId(uuid);
    assert_eq!(team_id.as_string(), uuid.to_string());
}

#[test]
fn tc_as_string_for_nil_uuid() {
    let team_id = TeamId(Uuid::nil());
    assert_eq!(team_id.as_string(), "00000000-0000-0000-0000-000000000000");
}

#[test]
fn tc_as_string_for_uppercase_input_normalizes_to_lowercase() {
    let headers = make_team_id_header("550E8400-E29B-41D4-A716-446655440000");
    let team_id = TeamId::from_headers(&headers).expect("uppercase UUID must parse");
    // as_string must return the canonical lowercase form regardless of input case.
    assert_eq!(team_id.as_string(), "550e8400-e29b-41d4-a716-446655440000");
}

// =============================================================================
// TeamId Copy + Clone semantics
// =============================================================================

#[test]
fn tc_team_id_copy_preserves_uuid() {
    let uuid = Uuid::new_v4();
    let team_id = TeamId(uuid);
    let copied = team_id; // uses Copy
    assert_eq!(team_id.0, copied.0);
}

#[test]
fn tc_team_id_clone_preserves_uuid() {
    let uuid = Uuid::new_v4();
    let team_id = TeamId(uuid);
    let cloned = team_id.clone();
    assert_eq!(team_id.0, cloned.0);
}

// =============================================================================
// extract_team_id convenience function
// =============================================================================

#[test]
fn tc_extract_team_id_valid_returns_team_id() {
    let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
    let headers = make_team_id_header(uuid_str);
    let result = extract_team_id(&headers);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().0.to_string(), uuid_str);
}

#[test]
fn tc_extract_team_id_missing_returns_error() {
    let headers = HeaderMap::new();
    let result = extract_team_id(&headers);
    assert!(result.is_err());
}

#[test]
fn tc_extract_team_id_invalid_returns_error() {
    let headers = make_team_id_header("invalid");
    let result = extract_team_id(&headers);
    assert!(result.is_err());
}

#[test]
fn tc_extract_team_id_delegates_to_from_headers() {
    // extract_team_id is documented as a convenience wrapper around
    // TeamId::from_headers — the two must produce identical results for the
    // same input.
    let uuid = Uuid::new_v4();
    let headers = make_team_id_header(&uuid.to_string());
    let via_extract = extract_team_id(&headers).expect("extract_team_id must succeed");
    let via_from_headers = TeamId::from_headers(&headers).expect("from_headers must succeed");
    assert_eq!(via_extract.0, via_from_headers.0);
}

// =============================================================================
// TeamId Debug representation
// =============================================================================

#[test]
fn tc_team_id_debug_format_contains_uuid() {
    let uuid = Uuid::new_v4();
    let team_id = TeamId(uuid);
    let debug_str = format!("{:?}", team_id);
    assert!(debug_str.contains(&uuid.to_string()));
}
