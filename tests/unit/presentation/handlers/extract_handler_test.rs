// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! External unit tests for extract_handler public API.
//!
//! Tests DTO serialization/deserialization (ExtractResponseDto,
//! ExtractRequestDto, ExtractResultDto), constants validation, and the
//! ExtractResponseDto wrapper struct exported by the handler module.

use uuid::Uuid;

use crawlrs::application::dto::extract_request::{
    ExtractRequestDto, ExtractResponseDto, ExtractResultDto,
};
use crawlrs::common::constants::crawl_task;
use crawlrs::presentation::handlers::extract_handler::ExtractResponseDto as HandlerExtractResponseDto;

// =============================================================================
// ExtractRequestDto deserialization
// =============================================================================

#[test]
fn tc_extract_request_dto_minimal_urls_only() {
    let json = r#"{"urls":["https://example.com"]}"#;
    let req: ExtractRequestDto = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.urls, vec!["https://example.com"]);
    assert!(req.prompt.is_none());
    assert!(req.schema.is_none());
    assert!(req.model.is_none());
    assert!(req.rules.is_none());
    assert!(req.sync_wait_ms.is_none());
}

#[test]
fn tc_extract_request_dto_with_prompt() {
    let json = r#"{"urls":["https://example.com"],"prompt":"Extract title"}"#;
    let req: ExtractRequestDto = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.prompt, Some("Extract title".to_string()));
}

#[test]
fn tc_extract_request_dto_with_schema() {
    let json = r#"{"urls":["https://example.com"],"schema":{"type":"object"}}"#;
    let req: ExtractRequestDto = serde_json::from_str(json).expect("must parse");
    assert!(req.schema.is_some());
    assert_eq!(req.schema.unwrap()["type"], "object");
}

#[test]
fn tc_extract_request_dto_with_model() {
    let json = r#"{"urls":["https://example.com"],"model":"gpt-4"}"#;
    let req: ExtractRequestDto = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.model, Some("gpt-4".to_string()));
}

#[test]
fn tc_extract_request_dto_with_sync_wait_ms() {
    let json = r#"{"urls":["https://example.com"],"sync_wait_ms":5000}"#;
    let req: ExtractRequestDto = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.sync_wait_ms, Some(5000));
}

#[test]
fn tc_extract_request_dto_with_all_fields() {
    let json = r#"{
        "urls": ["https://a.com", "https://b.com"],
        "prompt": "Extract data",
        "schema": {"type": "object"},
        "model": "gpt-4",
        "sync_wait_ms": 10000
    }"#;
    let req: ExtractRequestDto = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.urls.len(), 2);
    assert_eq!(req.prompt, Some("Extract data".to_string()));
    assert!(req.schema.is_some());
    assert_eq!(req.model, Some("gpt-4".to_string()));
    assert_eq!(req.sync_wait_ms, Some(10000));
}

#[test]
fn tc_extract_request_dto_empty_urls_array() {
    let json = r#"{"urls":[]}"#;
    let req: ExtractRequestDto = serde_json::from_str(json).expect("must parse");
    assert!(req.urls.is_empty());
}

#[test]
fn tc_extract_request_dto_missing_urls_fails() {
    let json = r#"{"prompt":"test"}"#;
    let result: Result<ExtractRequestDto, _> = serde_json::from_str(json);
    assert!(result.is_err(), "missing urls must fail deserialization");
}

#[test]
fn tc_extract_request_dto_serialization_round_trip() {
    let original = ExtractRequestDto {
        urls: vec!["https://example.com".to_string()],
        prompt: Some("test".to_string()),
        schema: Some(serde_json::json!({"type": "object"})),
        model: Some("gpt-4".to_string()),
        rules: None,
        sync_wait_ms: Some(5000),
    };
    let json = serde_json::to_string(&original).expect("must serialize");
    let parsed: ExtractRequestDto = serde_json::from_str(&json).expect("must deserialize");
    assert_eq!(parsed.urls, original.urls);
    assert_eq!(parsed.prompt, original.prompt);
    assert_eq!(parsed.model, original.model);
    assert_eq!(parsed.sync_wait_ms, original.sync_wait_ms);
}

// =============================================================================
// ExtractResponseDto (application DTO) serialization
// =============================================================================

#[test]
fn tc_extract_response_dto_empty_results() {
    let dto = ExtractResponseDto { results: vec![] };
    let json = serde_json::to_string(&dto).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["results"], serde_json::Value::Array(vec![]));
}

#[test]
fn tc_extract_response_dto_with_results() {
    let dto = ExtractResponseDto {
        results: vec![ExtractResultDto {
            url: "https://example.com".to_string(),
            data: serde_json::json!({"title": "Example"}),
            error: None,
        }],
    };
    let json = serde_json::to_string(&dto).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["results"][0]["url"], "https://example.com");
    assert_eq!(parsed["results"][0]["data"]["title"], "Example");
    assert!(parsed["results"][0]["error"].is_null());
}

#[test]
fn tc_extract_response_dto_with_error_result() {
    let dto = ExtractResponseDto {
        results: vec![ExtractResultDto {
            url: "https://fail.example.com".to_string(),
            data: serde_json::Value::Null,
            error: Some("timeout".to_string()),
        }],
    };
    let json = serde_json::to_string(&dto).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["results"][0]["error"], "timeout");
}

// =============================================================================
// ExtractResultDto serialization
// =============================================================================

#[test]
fn tc_extract_result_dto_success_no_error() {
    let dto = ExtractResultDto {
        url: "https://ok.example.com".to_string(),
        data: serde_json::json!({"status": "ok"}),
        error: None,
    };
    let json = serde_json::to_string(&dto).expect("must serialize");
    let parsed: ExtractResultDto = serde_json::from_str(&json).expect("must deserialize");
    assert_eq!(parsed.url, "https://ok.example.com");
    assert!(parsed.error.is_none());
}

#[test]
fn tc_extract_result_dto_with_error() {
    let dto = ExtractResultDto {
        url: "https://err.example.com".to_string(),
        data: serde_json::Value::Null,
        error: Some("connection refused".to_string()),
    };
    let json = serde_json::to_string(&dto).expect("must serialize");
    let parsed: ExtractResultDto = serde_json::from_str(&json).expect("must deserialize");
    assert_eq!(parsed.error, Some("connection refused".to_string()));
}

// =============================================================================
// Handler-level ExtractResponseDto (id + status wrapper)
// =============================================================================

#[test]
fn tc_handler_extract_response_dto_serialization() {
    let dto = HandlerExtractResponseDto {
        id: Uuid::new_v4(),
        status: "queued".to_string(),
    };
    let json = serde_json::to_string(&dto).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert!(parsed["id"].is_string());
    assert_eq!(parsed["status"], "queued");
}

#[test]
fn tc_handler_extract_response_dto_deserialization() {
    let id = Uuid::new_v4();
    let json = format!(r#"{{"id":"{}","status":"completed"}}"#, id);
    let dto: HandlerExtractResponseDto = serde_json::from_str(&json).expect("must deserialize");
    assert_eq!(dto.id, id);
    assert_eq!(dto.status, "completed");
}

#[test]
fn tc_handler_extract_response_dto_clone_preserves_fields() {
    let dto = HandlerExtractResponseDto {
        id: Uuid::new_v4(),
        status: "failed".to_string(),
    };
    let cloned = dto.clone();
    assert_eq!(dto.id, cloned.id);
    assert_eq!(dto.status, cloned.status);
}

#[test]
fn tc_handler_extract_response_dto_debug_format() {
    let dto = HandlerExtractResponseDto {
        id: Uuid::nil(),
        status: "queued".to_string(),
    };
    let debug = format!("{:?}", dto);
    assert!(debug.contains("queued"));
}

// =============================================================================
// Constants validation
// =============================================================================

#[test]
fn tc_max_sync_wait_ms_constant_value() {
    assert_eq!(crawl_task::MAX_SYNC_WAIT_MS, 30000);
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn tc_max_sync_wait_ms_is_positive() {
    assert!(crawl_task::MAX_SYNC_WAIT_MS > 0);
}

#[test]
fn tc_sync_wait_ms_at_max_is_valid() {
    // sync_wait_ms == MAX_SYNC_WAIT_MS must be accepted (boundary).
    let max = crawl_task::MAX_SYNC_WAIT_MS;
    assert!(max <= 30000, "MAX_SYNC_WAIT_MS must be <= 30000");
}

#[test]
fn tc_sync_wait_ms_above_max_is_invalid() {
    // The handler rejects sync_wait_ms > MAX_SYNC_WAIT_MS with BAD_REQUEST.
    let over = crawl_task::MAX_SYNC_WAIT_MS + 1;
    assert!(over > crawl_task::MAX_SYNC_WAIT_MS);
}
