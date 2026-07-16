// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! External unit tests for presentation::sdk public API.
//!
//! Tests the SDK-specific DTOs (request/response serialization) and the
//! `build_sdk_router()` construction function. The sdforge `#[forge]` macro
//! generates the HTTP endpoints; these tests verify the data contracts and
//! router assembly without spinning up a full server.

#![cfg(feature = "api-sdk")]

use uuid::Uuid;

use crawlrs::presentation::sdk::{
    build_sdk_router, SdkCreateCrawlRequest, SdkCreateTaskRequest, SdkCreateTaskResponse,
    SdkCrawlResponse, SdkScrapeRequest, SdkScrapeResponse, SdkSearchRequest, SdkSearchResponse,
    SdkSearchResult,
};

// =============================================================================
// SdkSearchRequest deserialization
// =============================================================================

#[test]
fn tc_sdk_search_request_minimal() {
    let json = r#"{"query":"rust web framework"}"#;
    let req: SdkSearchRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.query, "rust web framework");
    assert!(req.limit.is_none());
}

#[test]
fn tc_sdk_search_request_with_limit() {
    let json = r#"{"query":"tokio tutorial","limit":10}"#;
    let req: SdkSearchRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.query, "tokio tutorial");
    assert_eq!(req.limit, Some(10));
}

#[test]
fn tc_sdk_search_request_missing_query_fails() {
    let json = r#"{"limit":5}"#;
    let result: Result<SdkSearchRequest, _> = serde_json::from_str(json);
    assert!(result.is_err(), "missing query must fail");
}

#[test]
fn tc_sdk_search_request_empty_query() {
    let json = r#"{"query":""}"#;
    let req: SdkSearchRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.query, "");
}

#[test]
fn tc_sdk_search_request_deserialize_with_limit() {
    let json = r#"{"query":"test query","limit":20}"#;
    let req: SdkSearchRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.query, "test query");
    assert_eq!(req.limit, Some(20));
}

// =============================================================================
// SdkSearchResponse serialization
// =============================================================================

#[test]
fn tc_sdk_search_response_empty_results() {
    let resp = SdkSearchResponse {
        query: "empty".to_string(),
        results: vec![],
        credits_used: 0,
    };
    let json = serde_json::to_string(&resp).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["query"], "empty");
    assert_eq!(parsed["credits_used"], 0);
    assert_eq!(parsed["results"], serde_json::Value::Array(vec![]));
}

#[test]
fn tc_sdk_search_response_with_results() {
    let resp = SdkSearchResponse {
        query: "rust".to_string(),
        results: vec![SdkSearchResult {
            title: "Rust Programming Language".to_string(),
            url: "https://www.rust-lang.org/".to_string(),
            description: Some("A language empowering everyone to build reliable software.".to_string()),
            engine: "google".to_string(),
        }],
        credits_used: 1,
    };
    let json = serde_json::to_string(&resp).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["results"][0]["title"], "Rust Programming Language");
    assert_eq!(parsed["results"][0]["url"], "https://www.rust-lang.org/");
    assert_eq!(parsed["results"][0]["engine"], "google");
    assert_eq!(parsed["credits_used"], 1);
}

#[test]
fn tc_sdk_search_result_with_none_description() {
    let result = SdkSearchResult {
        title: "Test".to_string(),
        url: "https://example.com".to_string(),
        description: None,
        engine: "bing".to_string(),
    };
    let json = serde_json::to_string(&result).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert!(parsed["description"].is_null());
}

// =============================================================================
// SdkCreateTaskRequest deserialization
// =============================================================================

#[test]
fn tc_sdk_create_task_request_scrape() {
    let json = r#"{"url":"https://example.com","task_type":"scrape"}"#;
    let req: SdkCreateTaskRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.url, "https://example.com");
    assert_eq!(req.task_type, "scrape");
}

#[test]
fn tc_sdk_create_task_request_crawl() {
    let json = r#"{"url":"https://example.com","task_type":"crawl"}"#;
    let req: SdkCreateTaskRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.task_type, "crawl");
}

#[test]
fn tc_sdk_create_task_request_missing_url_fails() {
    let json = r#"{"task_type":"scrape"}"#;
    let result: Result<SdkCreateTaskRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn tc_sdk_create_task_request_missing_task_type_fails() {
    let json = r#"{"url":"https://example.com"}"#;
    let result: Result<SdkCreateTaskRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn tc_sdk_create_task_request_invalid_task_type_still_parses() {
    // task_type is a String — any value deserializes. Validation happens in
    // the handler, not the DTO.
    let json = r#"{"url":"https://example.com","task_type":"unknown"}"#;
    let req: SdkCreateTaskRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.task_type, "unknown");
}

// =============================================================================
// SdkCreateTaskResponse serialization
// =============================================================================

#[test]
fn tc_sdk_create_task_response_serialization() {
    let id = Uuid::new_v4();
    let resp = SdkCreateTaskResponse {
        id,
        url: "https://example.com".to_string(),
        status: "Queued".to_string(),
    };
    let json = serde_json::to_string(&resp).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["id"], id.to_string());
    assert_eq!(parsed["url"], "https://example.com");
    assert_eq!(parsed["status"], "Queued");
}

// =============================================================================
// SdkScrapeRequest / SdkScrapeResponse
// =============================================================================

#[test]
fn tc_sdk_scrape_request_deserialization() {
    let json = r#"{"url":"https://example.com/page"}"#;
    let req: SdkScrapeRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.url, "https://example.com/page");
}

#[test]
fn tc_sdk_scrape_request_missing_url_fails() {
    let json = r#"{}"#;
    let result: Result<SdkScrapeRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn tc_sdk_scrape_response_serialization() {
    let id = Uuid::new_v4();
    let resp = SdkScrapeResponse {
        id,
        url: "https://scrape.example.com".to_string(),
    };
    let json = serde_json::to_string(&resp).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["id"], id.to_string());
    assert_eq!(parsed["url"], "https://scrape.example.com");
}

// =============================================================================
// SdkCreateCrawlRequest / SdkCrawlResponse
// =============================================================================

#[test]
fn tc_sdk_create_crawl_request_deserialization() {
    let json = r#"{"name":"Test Crawl","url":"https://example.com","seed_url":"https://example.com/start"}"#;
    let req: SdkCreateCrawlRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.name, "Test Crawl");
    assert_eq!(req.url, "https://example.com");
    assert_eq!(req.seed_url, "https://example.com/start");
}

#[test]
fn tc_sdk_create_crawl_request_missing_name_fails() {
    let json = r#"{"url":"https://example.com","seed_url":"https://example.com/start"}"#;
    let result: Result<SdkCreateCrawlRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn tc_sdk_create_crawl_request_missing_url_fails() {
    let json = r#"{"name":"Test","seed_url":"https://example.com/start"}"#;
    let result: Result<SdkCreateCrawlRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn tc_sdk_create_crawl_request_missing_seed_url_fails() {
    let json = r#"{"name":"Test","url":"https://example.com"}"#;
    let result: Result<SdkCreateCrawlRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn tc_sdk_crawl_response_serialization() {
    let id = Uuid::new_v4();
    let resp = SdkCrawlResponse {
        id,
        status: "Active".to_string(),
        url: "https://crawl.example.com".to_string(),
    };
    let json = serde_json::to_string(&resp).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["id"], id.to_string());
    assert_eq!(parsed["status"], "Active");
    assert_eq!(parsed["url"], "https://crawl.example.com");
}

// =============================================================================
// build_sdk_router construction
// =============================================================================

#[test]
fn tc_build_sdk_router_returns_router() {
    // build_sdk_router() must return a valid axum::Router without panicking.
    // sdforge collects routes via its inventory system at compile time.
    let _router = build_sdk_router();
}

#[test]
fn tc_build_sdk_router_can_be_cloned() {
    let router = build_sdk_router();
    let _cloned = router.clone();
}
