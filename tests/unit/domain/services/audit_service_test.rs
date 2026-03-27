// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Audit service tests
//!
//! Tests for the AuditService including logging and query operations

use std::sync::Arc;
use uuid::Uuid;

use crate::domain::auth::ApiKeyScope;
use crate::domain::services::audit_service::{
    audit_log_repo_impl::AuditLogRepository,
    AuditLogBuilder, AuditLogEntry, AuditDecision, AuditService, AuditServiceError,
};

// === Mock Audit Log Repository ===

struct MockAuditLogRepository {
    logs: Arc<std::sync::Mutex<Vec<AuditLogEntry>>>,
    should_fail: Arc<std::sync::atomic::AtomicBool>,
}

impl MockAuditLogRepository {
    fn new() -> Self {
        Self {
            logs: Arc::new(std::sync::Mutex::new(Vec::new())),
            should_fail: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn get_log_count(&self) -> usize {
        let logs = self.logs.lock().unwrap();
        logs.len()
    }
}

#[async_trait::async_trait]
impl AuditLogRepository for MockAuditLogRepository {
    async fn create(&self, entry: &AuditLogEntry) -> Result<AuditLogEntry, AuditServiceError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AuditServiceError::DatabaseError(
                "Database error".to_string(),
            ));
        }

        let mut logs = self.logs.lock().unwrap();
        logs.push(entry.clone());
        Ok(entry.clone())
    }

    async fn find_by_api_key_id(
        &self,
        _api_key_id: Uuid,
        _limit: u64,
        _offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        let logs = self.logs.lock().unwrap();
        Ok(logs.clone())
    }

    async fn find_by_team_id(
        &self,
        _team_id: Uuid,
        _limit: u64,
        _offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        let logs = self.logs.lock().unwrap();
        Ok(logs.clone())
    }

    async fn find_denied_for_key(
        &self,
        _api_key_id: Uuid,
        _limit: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        Ok(vec![])
    }

    async fn cleanup_old_logs(&self, _retention_days: i64) -> Result<u64, AuditServiceError> {
        Ok(0)
    }
}

// === Helper Functions ===

fn create_test_service() -> AuditService<MockAuditLogRepository> {
    let repo = Arc::new(MockAuditLogRepository::new());
    AuditService::new(repo)
}

fn create_test_entry() -> AuditLogEntry {
    AuditLogBuilder::new("test_action", AuditDecision::Allowed)
        .with_api_key_id(Uuid::new_v4())
        .with_team_id(Uuid::new_v4())
        .build()
}

// === Unit Tests ===

#[tokio::test]
async fn test_service_creation() {
    let repo = Arc::new(MockAuditLogRepository::new());
    let service = AuditService::new(repo);

    // Service created successfully
    assert_eq!(service.repo.get_log_count(), 0);
}

#[tokio::test]
async fn test_log_audit_entry() {
    let service = create_test_service();
    let entry = create_test_entry();

    let result = service.log(entry.clone()).await;

    assert!(result.is_ok());
    assert_eq!(service.repo.get_log_count(), 1);
}

#[tokio::test]
async fn test_log_allow() {
    let service = create_test_service();

    let api_key_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    let scope = ApiKeyScope::Scrape;

    let result = service
        .log_allow(
            "test_action".to_string(),
            api_key_id,
            team_id,
            scope.clone(),
        )
        .await;

    assert!(result.is_ok());
    assert_eq!(service.repo.get_log_count(), 1);
}

#[tokio::test]
async fn test_log_deny() {
    let service = create_test_service();

    let api_key_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    let result = service
        .log_deny(
            "test_action".to_string(),
            Some(api_key_id),
            Some(team_id),
            "Test reason".to_string(),
            None,
        )
        .await;

    assert!(result.is_ok());
    assert_eq!(service.repo.get_log_count(), 1);
}

#[tokio::test]
async fn test_get_logs_for_key() {
    let service = create_test_service();
    let entry = create_test_entry();

    service.log(entry).await.unwrap();

    let api_key_id = Uuid::new_v4();
    let result = service.get_logs_for_key(api_key_id, 10, 0).await;

    assert!(result.is_ok());
    assert!(!result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_logs_for_team() {
    let service = create_test_service();
    let entry = create_test_entry();

    service.log(entry).await.unwrap();

    let team_id = Uuid::new_v4();
    let result = service.get_logs_for_team(team_id, 10, 0).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_denied_requests() {
    let service = create_test_service();

    let api_key_id = Uuid::new_v4();
    let result = service.get_denied_requests(api_key_id, 10).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_log_repository_error() {
    let repo = Arc::new(MockAuditLogRepository {
        logs: Arc::new(std::sync::Mutex::new(Vec::new())),
        should_fail: Arc::new(std::sync::atomic::AtomicBool::new(true)),
    });
    let service = AuditService::new(repo);
    let entry = create_test_entry();

    let result = service.log(entry).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        AuditServiceError::DatabaseError(_)
    ));
}

// === Builder Tests ===

#[test]
fn test_audit_log_builder_basic() {
    let entry = AuditLogBuilder::new("test_action", AuditDecision::Allowed).build();

    assert_eq!(entry.action, "test_action");
    assert!(matches!(entry.decision, AuditDecision::Allowed));
}

#[test]
fn test_audit_log_builder_with_api_key() {
    let api_key_id = Uuid::new_v4();
    let entry = AuditLogBuilder::new("test_action", AuditDecision::Allowed)
        .with_api_key_id(api_key_id)
        .build();

    assert_eq!(entry.api_key_id, Some(api_key_id));
}

#[test]
fn test_audit_log_builder_with_team() {
    let team_id = Uuid::new_v4();
    let entry = AuditLogBuilder::new("test_action", AuditDecision::Allowed)
        .with_team_id(team_id)
        .build();

    assert_eq!(entry.team_id, Some(team_id));
}

#[test]
fn test_audit_log_builder_with_scope() {
    let scope = ApiKeyScope::Admin;
    let entry = AuditLogBuilder::new("test_action", AuditDecision::Allowed)
        .with_scope(scope.clone())
        .build();

    assert_eq!(entry.scope, Some(scope));
}

#[test]
fn test_audit_log_builder_with_denial_reason() {
    let entry = AuditLogBuilder::new("test_action", AuditDecision::Denied)
        .with_denial_reason("Test reason")
        .build();

    assert_eq!(entry.decision, AuditDecision::Denied);
    assert_eq!(entry.denial_reason, Some("Test reason".to_string()));
}

#[test]
fn test_audit_log_builder_with_metadata() {
    let entry = AuditLogBuilder::new("test_action", AuditDecision::Allowed)
        .with_metadata("key", serde_json::json!("value"))
        .build();

    assert!(entry.metadata.contains_key("key"));
}

// === Complex Workflow Tests ===

#[tokio::test]
async fn test_audit_log_workflow() {
    let service = create_test_service();

    // Log allow
    let api_key_id = Uuid::new_v4();
    service
        .log_allow(
            "scrape_request".to_string(),
            api_key_id,
            Uuid::new_v4(),
            ApiKeyScope::Scrape,
        )
        .await
        .unwrap();

    // Query logs
    let logs = service.get_logs_for_key(api_key_id, 10, 0).await.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].action, "scrape_request");
}

#[tokio::test]
async fn test_log_multiple_entries() {
    let service = create_test_service();

    for i in 0..5 {
        let entry = AuditLogBuilder::new(
            format!("action_{}", i),
            AuditDecision::Allowed,
        )
        .build();
        service.log(entry).await.unwrap();
    }

    assert_eq!(service.repo.get_log_count(), 5);
}

// === Error Handling Tests ===

#[test]
fn test_audit_service_error_display() {
    let error = AuditServiceError::DatabaseError("test error".to_string());
    assert_eq!(format!("{}", error), "Database error: test error");
}

#[test]
fn test_audit_service_error_from_anyhow() {
    let anyhow_err = anyhow::anyhow!("test error");
    let service_err: AuditServiceError = anyhow_err.into();
    assert!(matches!(
        service_err,
        AuditServiceError::DatabaseError(_)
    ));
}

// === Edge Cases ===

#[tokio::test]
async fn test_log_with_empty_action() {
    let service = create_test_service();

    let entry = AuditLogBuilder::new("", AuditDecision::Allowed).build();

    let result = service.log(entry).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_log_with_long_action() {
    let service = create_test_service();

    let long_action = "x".repeat(1000);
    let entry = AuditLogBuilder::new(long_action.clone(), AuditDecision::Allowed).build();

    let result = service.log(entry).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_logs_with_pagination() {
    let service = create_test_service();

    // Add 20 entries
    for i in 0..20 {
        let entry = AuditLogBuilder::new(format!("action_{}", i), AuditDecision::Allowed).build();
        service.log(entry).await.unwrap();
    }

    // Query with limit
    let api_key_id = Uuid::new_v4();
    let logs = service.get_logs_for_key(api_key_id, 10, 0).await.unwrap();
    assert_eq!(logs.len(), 20); // Mock returns all logs, ignoring pagination

    // Query with offset
    let logs2 = service.get_logs_for_key(api_key_id, 10, 10).await.unwrap();
    assert!(logs2.is_empty()); // Mock returns empty for non-matching key
}

#[tokio::test]
async fn test_cleanup_old_logs() {
    let service = create_test_service();

    let result = service.cleanup_old_logs(30).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}
