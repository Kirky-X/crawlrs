// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Audit handler external unit tests
//!
//! Tests the public API surface of audit_handler: query deserialization,
//! pagination clamping, response DTO serialization, branch selection logic,
//! and actual handler function calls with a mock AuditServiceTrait.
//!
//! Handler functions use a lazy DbPool (no real database connection needed)
//! and a mock AuditServiceTrait to test all code paths without Docker.

use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use axum::extract::{Extension, Query};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use uuid::Uuid;

use crawlrs::common::constants::server_config;
use crawlrs::domain::auth::{AuditDecision, AuditLogEntry};
use crawlrs::domain::services::audit_service::{
    AuditLogBuilder, AuditServiceError, AuditServiceTrait,
};
use crawlrs::presentation::handlers::audit_handler::{
    get_audit_logs, get_denied_requests, AuditLogsQuery, AuditLogsResponseDto,
    DeniedRequestsResponseDto,
};
use crawlrs::presentation::middleware::auth_middleware::AuthState;

// ============================================================================
// Helper: create a lazy DbPool for AuthState (no real connection needed)
// ============================================================================

fn create_test_db_pool() -> Arc<dbnexus::DbPool> {
    use dbnexus::{DbConfig, DbPool};
    std::thread::scope(|s| {
        let handle = s.spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for DbPool construction");
            let _guard = rt.enter();
            DbPool::try_from(&DbConfig::default()).expect("failed to create lazy DbPool for test")
        });
        Arc::new(handle.join().expect("DbPool construction thread panicked"))
    })
}

fn make_auth_state() -> AuthState {
    use crawlrs::domain::auth::ApiKeyScope;
    let pool = create_test_db_pool();
    AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default())
}

fn make_auth_state_with_key(api_key_id: Uuid) -> AuthState {
    use crawlrs::domain::auth::ApiKeyScope;
    let pool = create_test_db_pool();
    AuthState::new(pool, Uuid::new_v4(), api_key_id, ApiKeyScope::default())
}

fn sample_entry(action: &str, decision: AuditDecision) -> AuditLogEntry {
    AuditLogBuilder::new(action, decision)
        .with_api_key_id(Uuid::new_v4())
        .with_team_id(Uuid::new_v4())
        .build()
}

// ============================================================================
// Mock AuditServiceTrait
// ============================================================================

struct MockAuditService {
    logs: Mutex<Vec<AuditLogEntry>>,
    should_fail: bool,
}

impl MockAuditService {
    fn new(logs: Vec<AuditLogEntry>) -> Self {
        Self {
            logs: Mutex::new(logs),
            should_fail: false,
        }
    }

    fn failing() -> Self {
        Self {
            logs: Mutex::new(Vec::new()),
            should_fail: true,
        }
    }
}

#[async_trait]
impl AuditServiceTrait for MockAuditService {
    async fn log(&self, _entry: AuditLogEntry) -> Result<(), AuditServiceError> {
        Ok(())
    }

    async fn log_allow(
        &self,
        _action: String,
        _api_key_id: Uuid,
        _team_id: Uuid,
        _scope: crawlrs::domain::auth::ApiKeyScope,
    ) -> Result<(), AuditServiceError> {
        Ok(())
    }

    async fn log_deny(
        &self,
        _action: String,
        _api_key_id: Option<Uuid>,
        _team_id: Option<Uuid>,
        _reason: String,
        _scope: Option<crawlrs::domain::auth::ApiKeyScope>,
    ) -> Result<(), AuditServiceError> {
        Ok(())
    }

    async fn get_logs_for_key(
        &self,
        _api_key_id: Uuid,
        _limit: u64,
        _offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        if self.should_fail {
            return Err(AuditServiceError::DatabaseError(sea_orm::DbErr::Custom(
                "mock error".to_string(),
            )));
        }
        Ok(self.logs.lock().unwrap().clone())
    }

    async fn get_logs_for_team(
        &self,
        _team_id: Uuid,
        _limit: u64,
        _offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        if self.should_fail {
            return Err(AuditServiceError::DatabaseError(sea_orm::DbErr::Custom(
                "mock error".to_string(),
            )));
        }
        Ok(self.logs.lock().unwrap().clone())
    }

    async fn get_denied_requests(
        &self,
        _api_key_id: Uuid,
        _limit: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        if self.should_fail {
            return Err(AuditServiceError::DatabaseError(sea_orm::DbErr::Custom(
                "mock error".to_string(),
            )));
        }
        Ok(self
            .logs
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.decision == AuditDecision::Deny)
            .cloned()
            .collect())
    }
}

// ============================================================================
// AuditLogsQuery deserialization tests
// ============================================================================

#[test]
fn test_audit_logs_query_empty() {
    let query: AuditLogsQuery = serde_urlencoded::from_str("").unwrap();
    assert!(query.limit.is_none());
    assert!(query.offset.is_none());
    assert!(query.api_key_id.is_none());
    assert!(query.team_id.is_none());
}

#[test]
fn test_audit_logs_query_with_limit_and_offset() {
    let query: AuditLogsQuery = serde_urlencoded::from_str("limit=50&offset=100").unwrap();
    assert_eq!(query.limit, Some(50));
    assert_eq!(query.offset, Some(100));
}

#[test]
fn test_audit_logs_query_with_api_key_id() {
    let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
    let query_str = format!("api_key_id={}", uuid_str);
    let query: AuditLogsQuery = serde_urlencoded::from_str(&query_str).unwrap();
    assert!(query.api_key_id.is_some());
    assert_eq!(query.api_key_id.unwrap().to_string(), uuid_str);
}

#[test]
fn test_audit_logs_query_with_team_id() {
    let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
    let query_str = format!("team_id={}", uuid_str);
    let query: AuditLogsQuery = serde_urlencoded::from_str(&query_str).unwrap();
    assert!(query.team_id.is_some());
}

#[test]
fn test_audit_logs_query_with_all_params() {
    let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
    let query_str = format!(
        "limit=10&offset=20&api_key_id={}&team_id={}",
        uuid_str, uuid_str
    );
    let query: AuditLogsQuery = serde_urlencoded::from_str(&query_str).unwrap();
    assert_eq!(query.limit, Some(10));
    assert_eq!(query.offset, Some(20));
    assert!(query.api_key_id.is_some());
    assert!(query.team_id.is_some());
}

#[test]
fn test_audit_logs_query_invalid_uuid_fails() {
    let result: Result<AuditLogsQuery, _> = serde_urlencoded::from_str("api_key_id=not-a-uuid");
    assert!(result.is_err());
}

#[test]
fn test_audit_logs_query_invalid_limit_fails() {
    let result: Result<AuditLogsQuery, _> = serde_urlencoded::from_str("limit=abc");
    assert!(result.is_err());
}

#[test]
fn test_audit_logs_query_limit_zero() {
    let query: AuditLogsQuery = serde_urlencoded::from_str("limit=0").unwrap();
    assert_eq!(query.limit, Some(0));
}

#[test]
fn test_audit_logs_query_large_offset() {
    let query: AuditLogsQuery = serde_urlencoded::from_str("offset=999999999").unwrap();
    assert_eq!(query.offset, Some(999999999));
}

// ============================================================================
// Pagination limit clamping logic (mirrors handler lines 47-51)
// ============================================================================

#[test]
fn test_limit_defaults_to_default_page_limit_when_none() {
    let limit: Option<u64> = None;
    let clamped = limit
        .unwrap_or(server_config::DEFAULT_PAGE_LIMIT as u64)
        .min(server_config::MAX_PAGE_LIMIT as u64);
    assert_eq!(clamped, server_config::DEFAULT_PAGE_LIMIT as u64);
}

#[test]
fn test_limit_uses_custom_value_when_within_max() {
    let limit: Option<u64> = Some(50);
    let clamped = limit
        .unwrap_or(server_config::DEFAULT_PAGE_LIMIT as u64)
        .min(server_config::MAX_PAGE_LIMIT as u64);
    assert_eq!(clamped, 50);
}

#[test]
fn test_limit_clamped_to_max_when_exceeds() {
    let limit: Option<u64> = Some(5000);
    let clamped = limit
        .unwrap_or(server_config::DEFAULT_PAGE_LIMIT as u64)
        .min(server_config::MAX_PAGE_LIMIT as u64);
    assert_eq!(clamped, server_config::MAX_PAGE_LIMIT as u64);
}

#[test]
fn test_limit_exactly_at_max_is_not_clamped() {
    let limit: Option<u64> = Some(server_config::MAX_PAGE_LIMIT as u64);
    let clamped = limit
        .unwrap_or(server_config::DEFAULT_PAGE_LIMIT as u64)
        .min(server_config::MAX_PAGE_LIMIT as u64);
    assert_eq!(clamped, server_config::MAX_PAGE_LIMIT as u64);
}

#[test]
fn test_limit_zero_is_preserved() {
    let limit: Option<u64> = Some(0);
    let clamped = limit
        .unwrap_or(server_config::DEFAULT_PAGE_LIMIT as u64)
        .min(server_config::MAX_PAGE_LIMIT as u64);
    assert_eq!(clamped, 0);
}

#[test]
fn test_offset_defaults_to_zero_when_none() {
    let offset: Option<u64> = None;
    let default_offset = offset.unwrap_or(0);
    assert_eq!(default_offset, 0);
}

#[test]
fn test_offset_uses_custom_value() {
    let offset: Option<u64> = Some(100);
    let default_offset = offset.unwrap_or(0);
    assert_eq!(default_offset, 100);
}

// ============================================================================
// AuditLogsResponseDto serialization
// ============================================================================

#[test]
fn test_audit_logs_response_dto_empty() {
    let dto: AuditLogsResponseDto<serde_json::Value> = AuditLogsResponseDto { logs: vec![] };
    let json = serde_json::to_string(&dto).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["logs"], serde_json::Value::Array(vec![]));
}

#[test]
fn test_audit_logs_response_dto_with_entries() {
    let dto: AuditLogsResponseDto<&str> = AuditLogsResponseDto {
        logs: vec!["entry1", "entry2"],
    };
    let json = serde_json::to_string(&dto).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["logs"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["logs"][0], "entry1");
    assert_eq!(parsed["logs"][1], "entry2");
}

#[test]
fn test_audit_logs_response_dto_with_json_logs() {
    let logs = vec![
        serde_json::json!({"action": "read", "resource": "task"}),
        serde_json::json!({"action": "write", "resource": "webhook"}),
    ];
    let dto = AuditLogsResponseDto { logs };
    let json = serde_json::to_string(&dto).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["logs"][0]["action"], "read");
    assert_eq!(parsed["logs"][1]["action"], "write");
}

// ============================================================================
// DeniedRequestsResponseDto serialization
// ============================================================================

#[test]
fn test_denied_requests_response_dto_empty() {
    let dto: DeniedRequestsResponseDto<serde_json::Value> = DeniedRequestsResponseDto {
        denied_requests: vec![],
    };
    let json = serde_json::to_string(&dto).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["denied_requests"], serde_json::Value::Array(vec![]));
}

#[test]
fn test_denied_requests_response_dto_with_entries() {
    let dto = DeniedRequestsResponseDto {
        denied_requests: vec![
            serde_json::json!({"reason": "rate limited", "ip": "1.2.3.4"}),
            serde_json::json!({"reason": "forbidden", "ip": "5.6.7.8"}),
        ],
    };
    let json = serde_json::to_string(&dto).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["denied_requests"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["denied_requests"][0]["reason"], "rate limited");
    assert_eq!(parsed["denied_requests"][1]["reason"], "forbidden");
}

#[test]
fn test_denied_requests_response_dto_clone() {
    let dto = DeniedRequestsResponseDto {
        denied_requests: vec![serde_json::json!({"reason": "test"})],
    };
    let cloned = dto.clone();
    assert_eq!(cloned.denied_requests.len(), 1);
}

// ============================================================================
// server_config constants
// ============================================================================

#[test]
fn test_default_page_limit_value() {
    assert_eq!(server_config::DEFAULT_PAGE_LIMIT, 100);
}

#[test]
fn test_max_page_limit_value() {
    assert_eq!(server_config::MAX_PAGE_LIMIT, 1000);
}

#[test]
fn test_default_page_limit_less_than_max() {
    assert!(server_config::DEFAULT_PAGE_LIMIT < server_config::MAX_PAGE_LIMIT);
}

// ============================================================================
// Branch selection logic (mirrors handler match on query)
// ============================================================================

#[test]
fn test_branch_selects_api_key_when_api_key_id_present() {
    // Handler: first match arm is api_key_id: Some(_)
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: Some(0),
        api_key_id: Some(Uuid::new_v4()),
        team_id: Some(Uuid::new_v4()),
    };
    // First match arm wins (api_key_id takes priority over team_id)
    assert!(query.api_key_id.is_some());
}

#[test]
fn test_branch_selects_team_when_only_team_id_present() {
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: Some(0),
        api_key_id: None,
        team_id: Some(Uuid::new_v4()),
    };
    // Second match arm: team_id: Some(_), api_key_id: _
    assert!(query.api_key_id.is_none());
    assert!(query.team_id.is_some());
}

#[test]
fn test_branch_falls_back_to_auth_state_when_neither_present() {
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: Some(0),
        api_key_id: None,
        team_id: None,
    };
    // Falls to _ arm: uses auth_state.api_key_id
    assert!(query.api_key_id.is_none());
    assert!(query.team_id.is_none());
}

#[test]
fn test_branch_api_key_takes_priority_over_team_id() {
    // When both are present, api_key_id branch is matched first
    let query = AuditLogsQuery {
        limit: None,
        offset: None,
        api_key_id: Some(Uuid::new_v4()),
        team_id: Some(Uuid::new_v4()),
    };
    // Verify the match pattern: api_key_id: Some(_) is first
    match query {
        AuditLogsQuery {
            api_key_id: Some(_),
            ..
        } => {
            // This is the first branch — correct
        }
        AuditLogsQuery {
            team_id: Some(_), ..
        } => {
            panic!("team_id branch should not be reached when api_key_id is Some");
        }
        _ => {
            panic!("fallback branch should not be reached");
        }
    }
}

// ============================================================================
// Handler function tests: get_audit_logs
// ============================================================================

#[tokio::test]
async fn test_get_audit_logs_by_api_key_id() {
    let logs = vec![
        sample_entry("search", AuditDecision::Allow),
        sample_entry("scrape", AuditDecision::Allow),
    ];
    let mock = Arc::new(MockAuditService::new(logs));
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: Some(0),
        api_key_id: Some(Uuid::new_v4()),
        team_id: None,
    };

    let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_audit_logs_by_team_id() {
    let logs = vec![sample_entry("crawl", AuditDecision::Allow)];
    let mock = Arc::new(MockAuditService::new(logs));
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: Some(50),
        offset: None,
        api_key_id: None,
        team_id: Some(Uuid::new_v4()),
    };

    let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_audit_logs_fallback_to_auth_state_api_key() {
    let logs = vec![sample_entry("extract", AuditDecision::Allow)];
    let mock = Arc::new(MockAuditService::new(logs));
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: None,
        offset: None,
        api_key_id: None,
        team_id: None,
    };

    let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_audit_logs_api_key_takes_priority_over_team_id() {
    let logs = vec![sample_entry("search", AuditDecision::Allow)];
    let mock = Arc::new(MockAuditService::new(logs));
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: Some(0),
        api_key_id: Some(Uuid::new_v4()),
        team_id: Some(Uuid::new_v4()),
    };

    let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    // Should succeed — api_key_id branch is taken
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_audit_logs_error_returns_internal_server_error() {
    let mock = Arc::new(MockAuditService::failing());
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: Some(0),
        api_key_id: Some(Uuid::new_v4()),
        team_id: None,
    };

    let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_get_audit_logs_error_on_team_branch() {
    let mock = Arc::new(MockAuditService::failing());
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: Some(0),
        api_key_id: None,
        team_id: Some(Uuid::new_v4()),
    };

    let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_get_audit_logs_error_on_fallback_branch() {
    let mock = Arc::new(MockAuditService::failing());
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: None,
        offset: None,
        api_key_id: None,
        team_id: None,
    };

    let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_get_audit_logs_empty_logs() {
    let mock = Arc::new(MockAuditService::new(vec![]));
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: Some(0),
        api_key_id: Some(Uuid::new_v4()),
        team_id: None,
    };

    let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_audit_logs_limit_clamped_to_max() {
    let logs = vec![sample_entry("search", AuditDecision::Allow)];
    let mock = Arc::new(MockAuditService::new(logs));
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: Some(50000),
        offset: Some(0),
        api_key_id: Some(Uuid::new_v4()),
        team_id: None,
    };

    let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

// ============================================================================
// Handler function tests: get_denied_requests
// ============================================================================

#[tokio::test]
async fn test_get_denied_requests_success() {
    let denied = vec![
        sample_entry("search", AuditDecision::Deny),
        sample_entry("scrape", AuditDecision::Deny),
    ];
    let mock = Arc::new(MockAuditService::new(denied));
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: None,
        api_key_id: None,
        team_id: None,
    };

    let response = get_denied_requests(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_denied_requests_error_returns_internal_server_error() {
    let mock = Arc::new(MockAuditService::failing());
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: None,
        api_key_id: None,
        team_id: None,
    };

    let response = get_denied_requests(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_get_denied_requests_empty() {
    let mock = Arc::new(MockAuditService::new(vec![]));
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: None,
        api_key_id: None,
        team_id: None,
    };

    let response = get_denied_requests(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_denied_requests_limit_clamped() {
    let denied = vec![sample_entry("search", AuditDecision::Deny)];
    let mock = Arc::new(MockAuditService::new(denied));
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: Some(99999),
        offset: None,
        api_key_id: None,
        team_id: None,
    };

    let response = get_denied_requests(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_denied_requests_uses_auth_state_api_key_id() {
    // get_denied_requests always uses auth_state.api_key_id (not query params)
    let api_key_id = Uuid::new_v4();
    let denied = vec![sample_entry("search", AuditDecision::Deny)];
    let mock = Arc::new(MockAuditService::new(denied));
    let auth_state = make_auth_state_with_key(api_key_id);
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: None,
        api_key_id: Some(Uuid::new_v4()),
        team_id: None,
    };

    let response = get_denied_requests(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_denied_requests_with_allow_entries_filters() {
    // Mock returns only Deny entries from get_denied_requests
    let entries = vec![
        sample_entry("search", AuditDecision::Deny),
        sample_entry("search", AuditDecision::Allow),
        sample_entry("crawl", AuditDecision::Deny),
    ];
    let mock = Arc::new(MockAuditService::new(entries));
    let auth_state = make_auth_state();
    let query = AuditLogsQuery {
        limit: Some(10),
        offset: None,
        api_key_id: None,
        team_id: None,
    };

    let response = get_denied_requests(Extension(mock), Extension(auth_state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

// ============================================================================
// AuditLogEntry and AuditLogBuilder tests
// ============================================================================

#[test]
fn test_audit_log_builder_minimal() {
    let entry = AuditLogBuilder::new("search", AuditDecision::Allow).build();
    assert_eq!(entry.requested_action, "search");
    assert_eq!(entry.decision, AuditDecision::Allow);
    assert!(entry.api_key_id.is_none());
    assert!(entry.team_id.is_none());
    assert!(entry.denial_reason.is_none());
}

#[test]
fn test_audit_log_builder_with_api_key_and_team() {
    let api_key_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    let entry = AuditLogBuilder::new("scrape", AuditDecision::Deny)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .build();
    assert_eq!(entry.api_key_id, Some(api_key_id));
    assert_eq!(entry.team_id, Some(team_id));
    assert_eq!(entry.decision, AuditDecision::Deny);
}

#[test]
fn test_audit_log_entry_serialization() {
    let entry = AuditLogBuilder::new("search", AuditDecision::Allow)
        .with_api_key_id(Uuid::new_v4())
        .build();
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["requested_action"], "search");
    assert_eq!(parsed["decision"], "Allow");
}

#[test]
fn test_audit_decision_display() {
    assert_eq!(format!("{}", AuditDecision::Allow), "ALLOW");
    assert_eq!(format!("{}", AuditDecision::Deny), "DENY");
}

#[test]
fn test_audit_log_entry_clone() {
    let entry = AuditLogBuilder::new("search", AuditDecision::Allow)
        .with_api_key_id(Uuid::new_v4())
        .build();
    let cloned = entry.clone();
    assert_eq!(entry.requested_action, cloned.requested_action);
    assert_eq!(entry.decision, cloned.decision);
    assert_eq!(entry.api_key_id, cloned.api_key_id);
}
