// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Audit service external unit tests
//!
//! Supplements the embedded tests in `src/domain/services/audit_service.rs` by
//! exercising the `AuditServiceTrait` implementation through the public trait
//! interface (covering error paths of trait-delegated methods) and verifying
//! builder + service integration end-to-end via the public crate API.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use uuid::Uuid;

use crawlrs::domain::auth::{ApiKeyScope, AuditDecision, AuditLogEntry};
use crawlrs::domain::repositories::audit_log_repository::{
    AuditLogRepository, AuditRepositoryError,
};
use crawlrs::domain::services::audit_service::{
    AuditLogBuilder, AuditService, AuditServiceError, AuditServiceTrait,
};

// =============================================================================
// Mock Audit Log Repository
// =============================================================================

struct MockAuditLogRepository {
    created: Mutex<Vec<AuditLogEntry>>,
    find_results: Mutex<Vec<AuditLogEntry>>,
    cleanup_count: u64,
    fail_all: bool,
}

impl MockAuditLogRepository {
    fn new() -> Self {
        Self {
            created: Mutex::new(Vec::new()),
            find_results: Mutex::new(Vec::new()),
            cleanup_count: 0,
            fail_all: false,
        }
    }

    fn failing() -> Self {
        Self {
            created: Mutex::new(Vec::new()),
            find_results: Mutex::new(Vec::new()),
            cleanup_count: 0,
            fail_all: true,
        }
    }

    fn with_find_results(results: Vec<AuditLogEntry>) -> Self {
        Self {
            created: Mutex::new(Vec::new()),
            find_results: Mutex::new(results),
            cleanup_count: 0,
            fail_all: false,
        }
    }

    fn with_cleanup_count(count: u64) -> Self {
        Self {
            created: Mutex::new(Vec::new()),
            find_results: Mutex::new(Vec::new()),
            cleanup_count: count,
            fail_all: false,
        }
    }

    fn created_count(&self) -> usize {
        self.created.lock().expect("created lock").len()
    }

    fn created_entries(&self) -> Vec<AuditLogEntry> {
        self.created.lock().expect("created lock").clone()
    }
}

impl Default for MockAuditLogRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AuditLogRepository for MockAuditLogRepository {
    async fn create(&self, entry: &AuditLogEntry) -> Result<AuditLogEntry, AuditRepositoryError> {
        if self.fail_all {
            return Err(AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(
                "mock create failure".to_string(),
            )));
        }
        self.created
            .lock()
            .expect("created lock")
            .push(entry.clone());
        Ok(entry.clone())
    }

    async fn find_by_api_key_id(
        &self,
        _api_key_id: Uuid,
        _limit: u64,
        _offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError> {
        if self.fail_all {
            return Err(AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(
                "mock find failure".to_string(),
            )));
        }
        Ok(self.find_results.lock().expect("find lock").clone())
    }

    async fn find_by_team_id(
        &self,
        _team_id: Uuid,
        _limit: u64,
        _offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError> {
        if self.fail_all {
            return Err(AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(
                "mock find failure".to_string(),
            )));
        }
        Ok(self.find_results.lock().expect("find lock").clone())
    }

    async fn find_denied_for_key(
        &self,
        _api_key_id: Uuid,
        _limit: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError> {
        if self.fail_all {
            return Err(AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(
                "mock find failure".to_string(),
            )));
        }
        Ok(self.find_results.lock().expect("find lock").clone())
    }

    async fn cleanup_old_logs(&self, _retention_days: i64) -> Result<u64, AuditRepositoryError> {
        if self.fail_all {
            return Err(AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(
                "mock cleanup failure".to_string(),
            )));
        }
        Ok(self.cleanup_count)
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn sample_entry(action: &str, decision: AuditDecision) -> AuditLogEntry {
    AuditLogBuilder::new(action, decision)
        .with_api_key_id(Uuid::new_v4())
        .with_team_id(Uuid::new_v4())
        .build()
}

fn make_service() -> (
    AuditService<MockAuditLogRepository>,
    Arc<MockAuditLogRepository>,
) {
    let repo = Arc::new(MockAuditLogRepository::new());
    let service = AuditService::new(repo.clone());
    (service, repo)
}

fn make_failing_service() -> AuditService<MockAuditLogRepository> {
    AuditService::new(Arc::new(MockAuditLogRepository::failing()))
}

// =============================================================================
// AuditService inherent methods - success paths
// =============================================================================

#[tokio::test]
async fn test_audit_service_log_success_records_entry() {
    let (service, repo) = make_service();
    let entry = sample_entry("scrape.create", AuditDecision::Allow);

    let result = service.log(entry).await;
    assert!(result.is_ok());
    assert_eq!(repo.created_count(), 1);
}

#[tokio::test]
async fn test_audit_service_log_allow_builds_entry_with_scope() {
    let (service, repo) = make_service();
    let api_key_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    let scope = ApiKeyScope::read_only();

    service
        .log_allow("scrape.create", api_key_id, team_id, scope.clone())
        .await
        .expect("log_allow should succeed");

    let entries = repo.created_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].requested_action, "scrape.create");
    assert_eq!(entries[0].decision, AuditDecision::Allow);
    assert_eq!(entries[0].api_key_id, Some(api_key_id));
    assert_eq!(entries[0].team_id, Some(team_id));
    assert_eq!(entries[0].scope_used, Some(scope));
    assert!(entries[0].denial_reason.is_none());
}

#[tokio::test]
async fn test_audit_service_log_deny_with_all_fields() {
    let (service, repo) = make_service();
    let api_key_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    let scope = ApiKeyScope::full_access();

    service
        .log_deny(
            "admin.delete",
            Some(api_key_id),
            Some(team_id),
            "insufficient scope",
            Some(scope.clone()),
        )
        .await
        .expect("log_deny should succeed");

    let entries = repo.created_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].requested_action, "admin.delete");
    assert_eq!(entries[0].decision, AuditDecision::Deny);
    assert_eq!(entries[0].api_key_id, Some(api_key_id));
    assert_eq!(entries[0].team_id, Some(team_id));
    assert_eq!(
        entries[0].denial_reason.as_deref(),
        Some("insufficient scope")
    );
    assert_eq!(entries[0].scope_used, Some(scope));
}

#[tokio::test]
async fn test_audit_service_log_deny_with_none_fields_preserves_none() {
    let (service, repo) = make_service();

    service
        .log_deny("anonymous.action", None, None, "auth required", None)
        .await
        .expect("log_deny should succeed");

    // `log_deny` 传入 None 时保留 None 语义（写入 NULL 而非 nil UUID），
    // 避免 find_by_api_key_id(nil_uuid) 误匹配。与 AuditLogBuilder::maybe_with_*
    // 的 M-2 regression guard 行为一致。
    let entries = repo.created_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].decision, AuditDecision::Deny);
    assert_eq!(entries[0].api_key_id, None);
    assert_eq!(entries[0].team_id, None);
    assert_eq!(entries[0].scope_used, None);
    assert_eq!(entries[0].denial_reason.as_deref(), Some("auth required"));
}

#[tokio::test]
async fn test_audit_service_get_logs_for_key_returns_results() {
    let entry1 = sample_entry("a1", AuditDecision::Allow);
    let entry2 = sample_entry("a2", AuditDecision::Deny);
    let repo = Arc::new(MockAuditLogRepository::with_find_results(vec![
        entry1.clone(),
        entry2.clone(),
    ]));
    let service = AuditService::new(repo);

    let results = service
        .get_logs_for_key(Uuid::new_v4(), 10, 0)
        .await
        .expect("get_logs_for_key should succeed");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].requested_action, "a1");
    assert_eq!(results[1].requested_action, "a2");
}

#[tokio::test]
async fn test_audit_service_get_logs_for_team_returns_results() {
    let entry = sample_entry("team_action", AuditDecision::Allow);
    let repo = Arc::new(MockAuditLogRepository::with_find_results(vec![entry]));
    let service = AuditService::new(repo);

    let results = service
        .get_logs_for_team(Uuid::new_v4(), 50, 10)
        .await
        .expect("get_logs_for_team should succeed");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].requested_action, "team_action");
}

#[tokio::test]
async fn test_audit_service_get_denied_requests_returns_results() {
    let denied = sample_entry("blocked", AuditDecision::Deny);
    let repo = Arc::new(MockAuditLogRepository::with_find_results(vec![denied]));
    let service = AuditService::new(repo);

    let results = service
        .get_denied_requests(Uuid::new_v4(), 100)
        .await
        .expect("get_denied_requests should succeed");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].decision, AuditDecision::Deny);
}

#[tokio::test]
async fn test_audit_service_cleanup_old_logs_returns_count() {
    let repo = Arc::new(MockAuditLogRepository::with_cleanup_count(42));
    let service = AuditService::new(repo);

    let count = service
        .cleanup_old_logs(90)
        .await
        .expect("cleanup_old_logs should succeed");
    assert_eq!(count, 42);
}

// =============================================================================
// AuditService inherent methods - error paths
// =============================================================================

#[tokio::test]
async fn test_audit_service_log_repository_error_propagates() {
    let service = make_failing_service();
    let entry = sample_entry("search", AuditDecision::Allow);

    let result = service.log(entry).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        AuditServiceError::RepositoryError(_) => {}
    }
}

#[tokio::test]
async fn test_audit_service_log_allow_repository_error() {
    let service = make_failing_service();
    let result = service
        .log_allow("x", Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default())
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_audit_service_log_deny_repository_error() {
    let service = make_failing_service();
    let result = service
        .log_deny("x", None, None, "r".to_string(), None)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_audit_service_get_logs_for_key_repository_error() {
    let service = make_failing_service();
    let result = service.get_logs_for_key(Uuid::new_v4(), 10, 0).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_audit_service_get_logs_for_team_repository_error() {
    let service = make_failing_service();
    let result = service.get_logs_for_team(Uuid::new_v4(), 50, 0).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_audit_service_get_denied_requests_repository_error() {
    let service = make_failing_service();
    let result = service.get_denied_requests(Uuid::new_v4(), 100).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_audit_service_cleanup_old_logs_repository_error() {
    let service = make_failing_service();
    let result = service.cleanup_old_logs(30).await;
    assert!(result.is_err());
}

// =============================================================================
// AuditServiceTrait impl - success paths (delegates to inherent methods)
// =============================================================================

#[tokio::test]
async fn test_trait_log_success_delegates_to_inherent() {
    let (service, repo) = make_service();
    let entry = sample_entry("trait_action", AuditDecision::Allow);

    let result = AuditServiceTrait::log(&service, entry).await;
    assert!(result.is_ok());
    assert_eq!(repo.created_count(), 1);
}

#[tokio::test]
async fn test_trait_log_allow_success() {
    let (service, repo) = make_service();

    let result = AuditServiceTrait::log_allow(
        &service,
        "trait.allow".to_string(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        ApiKeyScope::default(),
    )
    .await;
    assert!(result.is_ok());

    let entries = repo.created_entries();
    assert_eq!(entries[0].requested_action, "trait.allow");
    assert_eq!(entries[0].decision, AuditDecision::Allow);
}

#[tokio::test]
async fn test_trait_log_deny_success() {
    let (service, repo) = make_service();

    let result = AuditServiceTrait::log_deny(
        &service,
        "trait.deny".to_string(),
        None,
        None,
        "nope".to_string(),
        None,
    )
    .await;
    assert!(result.is_ok());

    let entries = repo.created_entries();
    assert_eq!(entries[0].requested_action, "trait.deny");
    assert_eq!(entries[0].decision, AuditDecision::Deny);
    assert_eq!(entries[0].denial_reason.as_deref(), Some("nope"));
}

#[tokio::test]
async fn test_trait_get_logs_for_key_success() {
    let entry = sample_entry("k", AuditDecision::Allow);
    let repo = Arc::new(MockAuditLogRepository::with_find_results(vec![entry]));
    let service: AuditService<MockAuditLogRepository> = AuditService::new(repo);

    let results = AuditServiceTrait::get_logs_for_key(&service, Uuid::new_v4(), 5, 0)
        .await
        .expect("trait get_logs_for_key should succeed");
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_trait_get_logs_for_team_success() {
    let repo = Arc::new(MockAuditLogRepository::new());
    let service: AuditService<MockAuditLogRepository> = AuditService::new(repo);

    let results = AuditServiceTrait::get_logs_for_team(&service, Uuid::new_v4(), 5, 0)
        .await
        .expect("trait get_logs_for_team should succeed");
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_trait_get_denied_requests_success() {
    let repo = Arc::new(MockAuditLogRepository::new());
    let service: AuditService<MockAuditLogRepository> = AuditService::new(repo);

    let results = AuditServiceTrait::get_denied_requests(&service, Uuid::new_v4(), 5)
        .await
        .expect("trait get_denied_requests should succeed");
    assert!(results.is_empty());
}

// =============================================================================
// AuditServiceTrait impl - ERROR paths (covers previously uncovered lines)
// =============================================================================

#[tokio::test]
async fn test_trait_log_error_propagates_repository_failure() {
    let service: AuditService<MockAuditLogRepository> = make_failing_service();
    let entry = sample_entry("trait_err", AuditDecision::Allow);

    let result = AuditServiceTrait::log(&service, entry).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        AuditServiceError::RepositoryError(_) => {}
    }
}

#[tokio::test]
async fn test_trait_log_allow_error_propagates_repository_failure() {
    let service: AuditService<MockAuditLogRepository> = make_failing_service();

    let result = AuditServiceTrait::log_allow(
        &service,
        "fail.allow".to_string(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        ApiKeyScope::default(),
    )
    .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        AuditServiceError::RepositoryError(_) => {}
    }
}

#[tokio::test]
async fn test_trait_log_deny_error_propagates_repository_failure() {
    let service: AuditService<MockAuditLogRepository> = make_failing_service();

    let result = AuditServiceTrait::log_deny(
        &service,
        "fail.deny".to_string(),
        None,
        None,
        "r".to_string(),
        None,
    )
    .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        AuditServiceError::RepositoryError(_) => {}
    }
}

#[tokio::test]
async fn test_trait_get_logs_for_key_error_propagates_repository_failure() {
    let service: AuditService<MockAuditLogRepository> = make_failing_service();

    let result = AuditServiceTrait::get_logs_for_key(&service, Uuid::new_v4(), 10, 0).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        AuditServiceError::RepositoryError(_) => {}
    }
}

#[tokio::test]
async fn test_trait_get_logs_for_team_error_propagates_repository_failure() {
    let service: AuditService<MockAuditLogRepository> = make_failing_service();

    let result = AuditServiceTrait::get_logs_for_team(&service, Uuid::new_v4(), 50, 0).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        AuditServiceError::RepositoryError(_) => {}
    }
}

#[tokio::test]
async fn test_trait_get_denied_requests_error_propagates_repository_failure() {
    let service: AuditService<MockAuditLogRepository> = make_failing_service();

    let result = AuditServiceTrait::get_denied_requests(&service, Uuid::new_v4(), 100).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        AuditServiceError::RepositoryError(_) => {}
    }
}

// =============================================================================
// AuditLogBuilder tests via public API
// =============================================================================

#[test]
fn test_builder_new_sets_action_and_decision() {
    let builder = AuditLogBuilder::new("test_action", AuditDecision::Allow);
    let entry = builder.build();
    assert_eq!(entry.requested_action, "test_action");
    assert_eq!(entry.decision, AuditDecision::Allow);
}

#[test]
fn test_builder_with_api_key_id_sets_field() {
    let api_key_id = Uuid::new_v4();
    let entry = AuditLogBuilder::new("a", AuditDecision::Allow)
        .with_api_key_id(api_key_id)
        .build();
    assert_eq!(entry.api_key_id, Some(api_key_id));
}

#[test]
fn test_builder_with_team_id_sets_field() {
    let team_id = Uuid::new_v4();
    let entry = AuditLogBuilder::new("a", AuditDecision::Allow)
        .with_team_id(team_id)
        .build();
    assert_eq!(entry.team_id, Some(team_id));
}

#[test]
fn test_builder_with_scope_sets_field() {
    let scope = ApiKeyScope::full_access();
    let entry = AuditLogBuilder::new("a", AuditDecision::Allow)
        .with_scope(scope.clone())
        .build();
    assert_eq!(entry.scope_used, Some(scope));
}

#[test]
fn test_builder_with_denial_reason_sets_field() {
    let entry = AuditLogBuilder::new("a", AuditDecision::Deny)
        .with_denial_reason("reason")
        .build();
    assert_eq!(entry.decision, AuditDecision::Deny);
    assert_eq!(entry.denial_reason.as_deref(), Some("reason"));
}

#[test]
fn test_builder_with_metadata_inserts_into_object() {
    let entry = AuditLogBuilder::new("a", AuditDecision::Allow)
        .with_metadata("key", serde_json::json!("value"))
        .build();
    match &entry.metadata {
        serde_json::Value::Object(map) => {
            assert_eq!(map.get("key"), Some(&serde_json::json!("value")));
        }
        _ => panic!("Expected object metadata"),
    }
}

#[test]
fn test_builder_build_generates_unique_id_and_timestamp() {
    let entry1 = AuditLogBuilder::new("a", AuditDecision::Allow).build();
    let entry2 = AuditLogBuilder::new("a", AuditDecision::Allow).build();
    assert_ne!(entry1.id, entry2.id);
    assert!(entry1.created_at <= chrono::Utc::now());
}

#[test]
fn test_builder_chaining_all_methods() {
    let api_key_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    let trace_id = Uuid::new_v4();
    let ip: std::net::IpAddr = std::net::Ipv4Addr::new(10, 0, 0, 1).into();

    let entry = AuditLogBuilder::new("chain", AuditDecision::Deny)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .with_denial_reason("no perm")
        .with_scope(ApiKeyScope::full_access())
        .with_ip_address(ip)
        .with_trace_id(trace_id)
        .with_user_agent("Agent/1.0")
        .with_request_path("/api/test")
        .with_request_method("POST")
        .with_metadata("k", serde_json::json!(1))
        .build();

    assert_eq!(entry.requested_action, "chain");
    assert_eq!(entry.decision, AuditDecision::Deny);
    assert_eq!(entry.api_key_id, Some(api_key_id));
    assert_eq!(entry.team_id, Some(team_id));
    assert_eq!(entry.denial_reason.as_deref(), Some("no perm"));
    assert_eq!(entry.scope_used, Some(ApiKeyScope::full_access()));
    assert_eq!(entry.ip_address(), Some(ip));
    assert_eq!(entry.trace_id(), Some(trace_id));
    assert_eq!(entry.user_agent(), Some("Agent/1.0"));
    assert_eq!(entry.request_path(), Some("/api/test"));
    assert_eq!(entry.request_method(), Some("POST"));
}

// =============================================================================
// AuditServiceError conversion tests
// =============================================================================

#[test]
fn test_audit_service_error_from_repository_db_error() {
    let repo_err = AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom("x".to_string()));
    let service_err: AuditServiceError = repo_err.into();
    match service_err {
        AuditServiceError::RepositoryError(_) => {}
    }
}

#[test]
fn test_audit_service_error_from_repository_not_found() {
    let repo_err = AuditRepositoryError::NotFound;
    let service_err: AuditServiceError = repo_err.into();
    match service_err {
        AuditServiceError::RepositoryError(AuditRepositoryError::NotFound) => {}
        other => panic!("expected RepositoryError(NotFound), got {:?}", other),
    }
}

#[test]
fn test_audit_service_error_display_repository_not_found() {
    let err = AuditServiceError::RepositoryError(AuditRepositoryError::NotFound);
    let msg = format!("{}", err);
    assert!(msg.contains("Repository error"));
    assert!(msg.contains("Audit log not found"));
}

// =============================================================================
// Edge cases
// =============================================================================

#[tokio::test]
async fn test_log_with_empty_action_succeeds() {
    let (service, repo) = make_service();
    let entry = AuditLogBuilder::new("", AuditDecision::Allow).build();

    let result = service.log(entry).await;
    assert!(result.is_ok());
    assert_eq!(repo.created_count(), 1);
}

#[tokio::test]
async fn test_log_with_long_action_succeeds() {
    let (service, repo) = make_service();
    let long_action = "x".repeat(1000);
    let entry = AuditLogBuilder::new(long_action.clone(), AuditDecision::Allow).build();

    let result = service.log(entry).await;
    assert!(result.is_ok());

    let entries = repo.created_entries();
    assert_eq!(entries[0].requested_action.len(), 1000);
}

#[tokio::test]
async fn test_log_multiple_entries_in_sequence() {
    let (service, repo) = make_service();

    for i in 0..5 {
        let entry = AuditLogBuilder::new(format!("action_{}", i), AuditDecision::Allow).build();
        service.log(entry).await.expect("log should succeed");
    }

    assert_eq!(repo.created_count(), 5);
}

#[tokio::test]
async fn test_get_logs_for_key_empty_returns_empty() {
    let repo = Arc::new(MockAuditLogRepository::new());
    let service = AuditService::new(repo);

    let results = service
        .get_logs_for_key(Uuid::new_v4(), 10, 0)
        .await
        .expect("should succeed");
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_get_denied_requests_empty_returns_empty() {
    let repo = Arc::new(MockAuditLogRepository::new());
    let service = AuditService::new(repo);

    let results = service
        .get_denied_requests(Uuid::new_v4(), 100)
        .await
        .expect("should succeed");
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_cleanup_old_logs_zero_returns_zero() {
    let repo = Arc::new(MockAuditLogRepository::new());
    let service = AuditService::new(repo);

    let count = service.cleanup_old_logs(30).await.expect("should succeed");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_audit_workflow_log_allow_then_query() {
    let repo = Arc::new(MockAuditLogRepository::new());
    let service = AuditService::new(repo.clone());

    let api_key_id = Uuid::new_v4();
    service
        .log_allow(
            "scrape_request",
            api_key_id,
            Uuid::new_v4(),
            ApiKeyScope::read_only(),
        )
        .await
        .expect("log_allow should succeed");

    let entries = repo.created_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].requested_action, "scrape_request");
    assert_eq!(entries[0].api_key_id, Some(api_key_id));
    assert_eq!(entries[0].decision, AuditDecision::Allow);
}
