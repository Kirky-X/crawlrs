// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! External unit tests for webhook_handler public API.
//!
//! Tests DTO serialization (CreateWebhookRequest, WebhookResponse,
//! WebhookListResponse) and the Webhook model's construction.

use chrono::Utc;
use uuid::Uuid;

use crawlrs::application::dto::webhook_request::{
    CreateWebhookRequest, WebhookListResponse, WebhookResponse,
};
use crawlrs::domain::models::Webhook;

// =============================================================================
// CreateWebhookRequest deserialization
// =============================================================================

#[test]
fn tc_create_webhook_request_valid_url() {
    let json = r#"{"url":"https://example.com/webhook"}"#;
    let req: CreateWebhookRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.url, "https://example.com/webhook");
}

#[test]
fn tc_create_webhook_request_rejects_extra_fields() {
    let json = r#"{"url":"https://example.com","extra":"field"}"#;
    let result: Result<CreateWebhookRequest, _> = serde_json::from_str(json);
    assert!(result.is_err(), "unknown fields must be rejected");
}

#[test]
fn tc_create_webhook_request_missing_url_fails() {
    let json = r#"{}"#;
    let result: Result<CreateWebhookRequest, _> = serde_json::from_str(json);
    assert!(result.is_err(), "missing url must fail");
}

#[test]
fn tc_create_webhook_request_empty_url() {
    let json = r#"{"url":""}"#;
    let req: CreateWebhookRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.url, "");
}

#[test]
fn tc_create_webhook_request_serialization_round_trip() {
    let original = CreateWebhookRequest {
        url: "https://my.webhook.site/abc123".to_string(),
    };
    let json = serde_json::to_string(&original).expect("must serialize");
    let parsed: CreateWebhookRequest = serde_json::from_str(&json).expect("must deserialize");
    assert_eq!(parsed.url, original.url);
}

#[test]
fn tc_create_webhook_request_http_url() {
    let json = r#"{"url":"http://example.com/hook"}"#;
    let req: CreateWebhookRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.url, "http://example.com/hook");
}

#[test]
fn tc_create_webhook_request_with_path_and_query() {
    let json = r#"{"url":"https://example.com/api/v1/webhook?token=abc"}"#;
    let req: CreateWebhookRequest = serde_json::from_str(json).expect("must parse");
    assert!(req.url.contains("token=abc"));
}

// =============================================================================
// WebhookResponse serialization
// =============================================================================

#[test]
fn tc_webhook_response_serialization_with_secret() {
    let response = WebhookResponse {
        id: Uuid::new_v4(),
        team_id: Uuid::new_v4(),
        url: "https://example.com/hook".to_string(),
        created_at: Utc::now(),
        is_active: true,
        secret: Some("secret123".to_string()),
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["url"], "https://example.com/hook");
    assert_eq!(parsed["is_active"], true);
    assert_eq!(parsed["secret"], "secret123");
}

#[test]
fn tc_webhook_response_serialization_secret_none() {
    let response = WebhookResponse {
        id: Uuid::new_v4(),
        team_id: Uuid::new_v4(),
        url: "https://example.com/hook".to_string(),
        created_at: Utc::now(),
        is_active: false,
        secret: None,
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["is_active"], false);
    assert!(parsed["secret"].is_null());
}

#[test]
fn tc_webhook_response_serialization_includes_all_fields() {
    let id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    let response = WebhookResponse {
        id,
        team_id,
        url: "https://x.com".to_string(),
        created_at: Utc::now(),
        is_active: true,
        secret: Some("s".to_string()),
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["id"], id.to_string());
    assert_eq!(parsed["team_id"], team_id.to_string());
    assert_eq!(parsed["url"], "https://x.com");
    assert_eq!(parsed["is_active"], true);
    assert_eq!(parsed["secret"], "s");
}

#[test]
fn tc_webhook_response_clone_preserves_fields() {
    let response = WebhookResponse {
        id: Uuid::new_v4(),
        team_id: Uuid::new_v4(),
        url: "https://clone.example.com".to_string(),
        created_at: Utc::now(),
        is_active: true,
        secret: Some("secret".to_string()),
    };
    let cloned = response.clone();
    assert_eq!(response.id, cloned.id);
    assert_eq!(response.url, cloned.url);
    assert_eq!(response.is_active, cloned.is_active);
    assert_eq!(response.secret, cloned.secret);
}

// =============================================================================
// WebhookListResponse serialization
// =============================================================================

#[test]
fn tc_webhook_list_response_empty() {
    let response = WebhookListResponse {
        webhooks: vec![],
        total: 0,
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["total"], 0);
    assert_eq!(parsed["webhooks"], serde_json::Value::Array(vec![]));
}

#[test]
fn tc_webhook_list_response_with_items() {
    let w1 = WebhookResponse {
        id: Uuid::new_v4(),
        team_id: Uuid::new_v4(),
        url: "https://hook1.example.com".to_string(),
        created_at: Utc::now(),
        is_active: true,
        secret: None,
    };
    let w2 = WebhookResponse {
        id: Uuid::new_v4(),
        team_id: Uuid::new_v4(),
        url: "https://hook2.example.com".to_string(),
        created_at: Utc::now(),
        is_active: false,
        secret: Some("s2".to_string()),
    };
    let response = WebhookListResponse {
        webhooks: vec![w1, w2],
        total: 2,
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["total"], 2);
    assert_eq!(parsed["webhooks"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["webhooks"][0]["url"], "https://hook1.example.com");
    assert_eq!(parsed["webhooks"][1]["url"], "https://hook2.example.com");
}

#[test]
fn tc_webhook_list_response_total_matches_count() {
    let webhooks: Vec<WebhookResponse> = (0..3)
        .map(|_| WebhookResponse {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            created_at: Utc::now(),
            is_active: true,
            secret: None,
        })
        .collect();
    let count = webhooks.len();
    let response = WebhookListResponse {
        webhooks,
        total: count,
    };
    assert_eq!(response.total, 3);
    assert_eq!(response.webhooks.len(), 3);
}

// =============================================================================
// Webhook model construction
// =============================================================================

#[test]
fn tc_webhook_new_constructor() {
    let id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    let webhook = Webhook::new(id, team_id, "https://example.com/hook".to_string());
    assert_eq!(webhook.id, id);
    assert_eq!(webhook.team_id, team_id);
    assert_eq!(webhook.url, "https://example.com/hook");
    assert!(webhook.created_at <= Utc::now());
}

#[test]
fn tc_webhook_clone_preserves_fields() {
    let webhook = Webhook::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "https://clone.example.com".to_string(),
    );
    let cloned = webhook.clone();
    assert_eq!(webhook.id, cloned.id);
    assert_eq!(webhook.team_id, cloned.team_id);
    assert_eq!(webhook.url, cloned.url);
    assert_eq!(webhook.created_at, cloned.created_at);
}

#[test]
fn tc_webhook_debug_format_contains_url() {
    let webhook = Webhook::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "https://debug.example.com".to_string(),
    );
    let debug = format!("{:?}", webhook);
    assert!(debug.contains("debug.example.com"));
}
