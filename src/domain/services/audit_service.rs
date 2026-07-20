// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Service for audit logging

use crate::common::error::CrawlRsError;
use crate::domain::auth::{ApiKeyScope, AuditDecision, AuditLogEntry};
use crate::domain::repositories::audit_log_repository::{AuditLogRepository, AuditRepositoryError};
use log::debug;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

// AuditLogBuilder 已拆分到独立文件（架构 MEDIUM 1：单一职责）
// 这里 re-export 保持外部 import 路径向后兼容：
// `crawlrs::domain::services::audit_service::AuditLogBuilder` 仍可用
pub use crate::domain::services::audit_log_builder::AuditLogBuilder;

#[derive(Debug, Error)]
pub enum AuditServiceError {
    #[error("Repository error: {0}")]
    RepositoryError(#[from] AuditRepositoryError),
}

impl From<AuditServiceError> for CrawlRsError {
    fn from(error: AuditServiceError) -> Self {
        match error {
            AuditServiceError::RepositoryError(repo_err) => {
                // 尝试将 RepositoryError 转换为更具体的 CrawlRsError
                match repo_err {
                    AuditRepositoryError::DatabaseError(db_err) => CrawlRsError::Database(db_err),
                    _ => CrawlRsError::Other(repo_err.to_string()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    // ============================================================
    // AuditService tests
    // ============================================================

    use crate::domain::repositories::audit_log_repository::AuditLogRepository;
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// Mock audit log repository that records all interactions.
    struct MockAuditLogRepository {
        /// All entries passed to create()
        created: Mutex<Vec<AuditLogEntry>>,
        /// Entries to return from find_* methods
        find_results: Mutex<Vec<AuditLogEntry>>,
        /// Number of entries cleanup reports as deleted
        cleanup_count: u64,
        /// When true, all operations return an error
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
    }

    impl Default for MockAuditLogRepository {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl AuditLogRepository for MockAuditLogRepository {
        async fn create(
            &self,
            entry: &AuditLogEntry,
        ) -> Result<AuditLogEntry, AuditRepositoryError> {
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

        async fn cleanup_old_logs(
            &self,
            _retention_days: i64,
        ) -> Result<u64, AuditRepositoryError> {
            if self.fail_all {
                return Err(AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(
                    "mock cleanup failure".to_string(),
                )));
            }
            Ok(self.cleanup_count)
        }
    }

    fn sample_entry(action: &str, decision: AuditDecision) -> AuditLogEntry {
        AuditLogBuilder::new(action, decision)
            .with_api_key_id(Uuid::new_v4())
            .with_team_id(Uuid::new_v4())
            .build()
    }

    // ---- AuditService::new / construction ----

    #[test]
    fn test_audit_service_new_constructs_service() {
        let repo: Arc<MockAuditLogRepository> = Arc::new(MockAuditLogRepository::new());
        let service = AuditService::new(repo.clone());
        // Verify the service can be created; we exercise it via log below
        let _ = service.audit_repo.clone();
    }

    // ---- AuditService::log ----

    #[tokio::test]
    async fn test_audit_service_log_success() {
        let repo = Arc::new(MockAuditLogRepository::new());
        let service = AuditService::new(repo.clone());
        let entry = sample_entry("search", AuditDecision::Allow);
        let result = service.log(entry.clone()).await;
        assert!(result.is_ok());
        let created = repo.created.lock().expect("lock");
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].requested_action, "search");
        assert_eq!(created[0].decision, AuditDecision::Allow);
    }

    #[tokio::test]
    async fn test_audit_service_log_repository_error_propagates() {
        let repo = Arc::new(MockAuditLogRepository::failing());
        let service = AuditService::new(repo);
        let entry = sample_entry("search", AuditDecision::Allow);
        let result = service.log(entry).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AuditServiceError::RepositoryError(_) => {}
        }
    }

    // ---- AuditService::log_allow ----

    #[tokio::test]
    async fn test_audit_service_log_allow_builds_allow_entry() {
        let repo = Arc::new(MockAuditLogRepository::new());
        let service = AuditService::new(repo.clone());
        let api_key_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let scope = ApiKeyScope::read_only();

        service
            .log_allow("scrape.create", api_key_id, team_id, scope.clone())
            .await
            .expect("log_allow should succeed");

        let created = repo.created.lock().expect("lock");
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].requested_action, "scrape.create");
        assert_eq!(created[0].decision, AuditDecision::Allow);
        assert_eq!(created[0].api_key_id, Some(api_key_id));
        assert_eq!(created[0].team_id, Some(team_id));
        assert_eq!(created[0].scope_used, Some(scope));
        // Allow entries have no denial reason
        assert!(created[0].denial_reason.is_none());
    }

    #[tokio::test]
    async fn test_audit_service_log_allow_repository_error() {
        let repo = Arc::new(MockAuditLogRepository::failing());
        let service = AuditService::new(repo);
        let result = service
            .log_allow("x", Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default())
            .await;
        assert!(result.is_err());
    }

    // ---- AuditService::log_deny ----

    #[tokio::test]
    async fn test_audit_service_log_deny_with_all_fields() {
        let repo = Arc::new(MockAuditLogRepository::new());
        let service = AuditService::new(repo.clone());
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

        let created = repo.created.lock().expect("lock");
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].requested_action, "admin.delete");
        assert_eq!(created[0].decision, AuditDecision::Deny);
        assert_eq!(created[0].api_key_id, Some(api_key_id));
        assert_eq!(created[0].team_id, Some(team_id));
        assert_eq!(
            created[0].denial_reason.as_deref(),
            Some("insufficient scope")
        );
        assert_eq!(created[0].scope_used, Some(scope));
    }

    #[tokio::test]
    async fn test_audit_service_log_deny_with_none_fields_preserves_none() {
        // M-2 fix: None 字段必须保留为 None（写入数据库 NULL），而非 unwrap_or_default()
        // 转换为 nil UUID (`00000000-...`)。nil UUID 会被 find_by_api_key_id(nil_uuid) 误匹配，
        // 混淆真实 API key 的审计日志。
        let repo = Arc::new(MockAuditLogRepository::new());
        let service = AuditService::new(repo.clone());

        service
            .log_deny("anonymous.action", None, None, "auth required", None)
            .await
            .expect("log_deny should succeed");

        let created = repo.created.lock().expect("lock");
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].decision, AuditDecision::Deny);
        // None values must stay None (NULL in DB), not become nil UUID / default scope
        assert_eq!(created[0].api_key_id, None);
        assert_eq!(created[0].team_id, None);
        assert_eq!(created[0].scope_used, None);
        assert_eq!(created[0].denial_reason.as_deref(), Some("auth required"));
    }

    #[tokio::test]
    async fn test_audit_service_log_deny_repository_error() {
        let repo = Arc::new(MockAuditLogRepository::failing());
        let service = AuditService::new(repo);
        let result = service
            .log_deny("x", None, None, "r".to_string(), None)
            .await;
        assert!(result.is_err());
    }

    // ---- AuditService::get_logs_for_key ----

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
    async fn test_audit_service_get_logs_for_key_empty() {
        let repo = Arc::new(MockAuditLogRepository::new());
        let service = AuditService::new(repo);
        let results = service
            .get_logs_for_key(Uuid::new_v4(), 10, 0)
            .await
            .expect("should succeed");
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_audit_service_get_logs_for_key_repository_error() {
        let repo = Arc::new(MockAuditLogRepository::failing());
        let service = AuditService::new(repo);
        let result = service.get_logs_for_key(Uuid::new_v4(), 10, 0).await;
        assert!(result.is_err());
    }

    // ---- AuditService::get_logs_for_team ----

    #[tokio::test]
    async fn test_audit_service_get_logs_for_team_returns_results() {
        let entry = sample_entry("team_action", AuditDecision::Allow);
        let repo = Arc::new(MockAuditLogRepository::with_find_results(vec![
            entry.clone()
        ]));
        let service = AuditService::new(repo);

        let results = service
            .get_logs_for_team(Uuid::new_v4(), 50, 10)
            .await
            .expect("get_logs_for_team should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].requested_action, "team_action");
    }

    #[tokio::test]
    async fn test_audit_service_get_logs_for_team_repository_error() {
        let repo = Arc::new(MockAuditLogRepository::failing());
        let service = AuditService::new(repo);
        let result = service.get_logs_for_team(Uuid::new_v4(), 50, 0).await;
        assert!(result.is_err());
    }

    // ---- AuditService::get_denied_requests ----

    #[tokio::test]
    async fn test_audit_service_get_denied_requests_returns_only_denied() {
        let denied1 = sample_entry("blocked1", AuditDecision::Deny);
        let denied2 = sample_entry("blocked2", AuditDecision::Deny);
        let repo = Arc::new(MockAuditLogRepository::with_find_results(vec![
            denied1.clone(),
            denied2.clone(),
        ]));
        let service = AuditService::new(repo);

        let results = service
            .get_denied_requests(Uuid::new_v4(), 100)
            .await
            .expect("get_denied_requests should succeed");
        assert_eq!(results.len(), 2);
        // All returned entries should be Deny decisions (mock returns what we gave it)
        assert!(results.iter().all(|r| r.decision == AuditDecision::Deny));
    }

    #[tokio::test]
    async fn test_audit_service_get_denied_requests_empty() {
        let repo = Arc::new(MockAuditLogRepository::new());
        let service = AuditService::new(repo);
        let results = service
            .get_denied_requests(Uuid::new_v4(), 100)
            .await
            .expect("should succeed");
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_audit_service_get_denied_requests_repository_error() {
        let repo = Arc::new(MockAuditLogRepository::failing());
        let service = AuditService::new(repo);
        let result = service.get_denied_requests(Uuid::new_v4(), 100).await;
        assert!(result.is_err());
    }

    // ---- AuditService::cleanup_old_logs ----

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

    #[tokio::test]
    async fn test_audit_service_cleanup_old_logs_zero() {
        let repo = Arc::new(MockAuditLogRepository::new());
        let service = AuditService::new(repo);
        let count = service.cleanup_old_logs(30).await.expect("should succeed");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_audit_service_cleanup_old_logs_repository_error() {
        let repo = Arc::new(MockAuditLogRepository::failing());
        let service = AuditService::new(repo);
        let result = service.cleanup_old_logs(30).await;
        assert!(result.is_err());
    }

    // ---- AuditServiceTrait impl ----

    #[tokio::test]
    async fn test_audit_service_trait_log_delegates_to_log() {
        let repo = Arc::new(MockAuditLogRepository::new());
        let service: AuditService<MockAuditLogRepository> = AuditService::new(repo.clone());
        let entry = sample_entry("trait_action", AuditDecision::Allow);

        // Use the trait method
        let result = AuditServiceTrait::log(&service, entry.clone()).await;
        assert!(result.is_ok());

        let created = repo.created.lock().expect("lock");
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].requested_action, "trait_action");
    }

    #[tokio::test]
    async fn test_audit_service_trait_log_allow() {
        let repo = Arc::new(MockAuditLogRepository::new());
        let service: AuditService<MockAuditLogRepository> = AuditService::new(repo.clone());

        let result = AuditServiceTrait::log_allow(
            &service,
            "trait.allow".to_string(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        )
        .await;
        assert!(result.is_ok());

        let created = repo.created.lock().expect("lock");
        assert_eq!(created[0].requested_action, "trait.allow");
        assert_eq!(created[0].decision, AuditDecision::Allow);
    }

    #[tokio::test]
    async fn test_audit_service_trait_log_deny() {
        let repo = Arc::new(MockAuditLogRepository::new());
        let service: AuditService<MockAuditLogRepository> = AuditService::new(repo.clone());

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

        let created = repo.created.lock().expect("lock");
        assert_eq!(created[0].requested_action, "trait.deny");
        assert_eq!(created[0].decision, AuditDecision::Deny);
        assert_eq!(created[0].denial_reason.as_deref(), Some("nope"));
    }

    #[tokio::test]
    async fn test_audit_service_trait_get_logs_for_key() {
        let entry = sample_entry("k", AuditDecision::Allow);
        let repo = Arc::new(MockAuditLogRepository::with_find_results(vec![entry]));
        let service: AuditService<MockAuditLogRepository> = AuditService::new(repo);

        let results = AuditServiceTrait::get_logs_for_key(&service, Uuid::new_v4(), 5, 0)
            .await
            .expect("trait get_logs_for_key should succeed");
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_audit_service_trait_get_logs_for_team() {
        let repo = Arc::new(MockAuditLogRepository::new());
        let service: AuditService<MockAuditLogRepository> = AuditService::new(repo);

        let results = AuditServiceTrait::get_logs_for_team(&service, Uuid::new_v4(), 5, 0)
            .await
            .expect("trait get_logs_for_team should succeed");
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_audit_service_trait_get_denied_requests() {
        let repo = Arc::new(MockAuditLogRepository::new());
        let service: AuditService<MockAuditLogRepository> = AuditService::new(repo);

        let results = AuditServiceTrait::get_denied_requests(&service, Uuid::new_v4(), 5)
            .await
            .expect("trait get_denied_requests should succeed");
        assert!(results.is_empty());
    }

    // ---- AuditServiceError From impls ----
    // 注意：AuditServiceError 已不再实现 From<sea_orm::DbErr>（分层违规 HIGH 1 修复）。
    // DbErr 必须先经 AuditRepositoryError::DatabaseError 包装，再 .into() 为 AuditServiceError。

    #[test]
    fn test_audit_service_error_from_repository_db_error_to_app_error() {
        let repo_err = AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(
            "repo db failure".to_string(),
        ));
        let service_err: AuditServiceError = repo_err.into();
        let app_err: CrawlRsError = service_err.into();
        // DatabaseError variant of AuditRepositoryError should map to CrawlRsError::Database
        match app_err {
            CrawlRsError::Database(_) => {}
            other => panic!("expected CrawlRsError::Database, got {:?}", other),
        }
    }

    #[test]
    fn test_audit_service_error_from_repository_not_found_to_app_error_other() {
        let repo_err = AuditRepositoryError::NotFound;
        let service_err: AuditServiceError = repo_err.into();
        let app_err: CrawlRsError = service_err.into();
        // NotFound variant should map to CrawlRsError::Other (string)
        match app_err {
            CrawlRsError::Other(msg) => {
                assert!(msg.contains("Audit log not found"), "msg was: {}", msg);
            }
            other => panic!("expected CrawlRsError::Other, got {:?}", other),
        }
    }

    #[test]
    fn test_audit_service_error_display_repository() {
        let err = AuditServiceError::RepositoryError(AuditRepositoryError::NotFound);
        let msg = format!("{}", err);
        assert!(msg.contains("Repository error"));
        assert!(msg.contains("Audit log not found"));
    }

    #[test]
    fn test_audit_log_entry_getters_return_set_values() {
        let ip: std::net::IpAddr = Ipv4Addr::new(192, 168, 0, 1).into();
        let trace_id = Uuid::new_v4();
        let entry = AuditLogBuilder::new("action", AuditDecision::Allow)
            .with_ip_address(ip)
            .with_trace_id(trace_id)
            .with_user_agent("Agent/1.0")
            .with_request_path("/path")
            .with_request_method("POST")
            .build();

        assert_eq!(entry.ip_address(), Some(ip));
        assert_eq!(entry.trace_id(), Some(trace_id));
        assert_eq!(entry.user_agent(), Some("Agent/1.0"));
        assert_eq!(entry.request_path(), Some("/path"));
        assert_eq!(entry.request_method(), Some("POST"));
    }

    #[test]
    fn test_audit_log_entry_getters_return_none_when_unset() {
        let entry = AuditLogBuilder::new("action", AuditDecision::Allow).build();
        assert!(entry.ip_address().is_none());
        assert!(entry.trace_id().is_none());
        assert!(entry.user_agent().is_none());
        assert!(entry.request_path().is_none());
        assert!(entry.request_method().is_none());
    }

    // ---- logger initialization to cover debug! macro argument lines ----

    use std::sync::Once;

    static TEST_LOGGER_INIT: Once = Once::new();

    struct TestLogger;
    impl log::Log for TestLogger {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn log(&self, _: &log::Record) {}
        fn flush(&self) {}
    }

    static TEST_LOGGER: TestLogger = TestLogger;

    fn ensure_test_logger() {
        TEST_LOGGER_INIT.call_once(|| {
            let _ = log::set_logger(&TEST_LOGGER);
            log::set_max_level(log::LevelFilter::Debug);
        });
    }

    #[tokio::test]
    async fn test_audit_service_log_with_logger_covers_debug_macro_args() {
        ensure_test_logger();
        let repo = Arc::new(MockAuditLogRepository::new());
        let service = AuditService::new(repo.clone());
        let entry = sample_entry("search", AuditDecision::Allow);
        let result = service.log(entry).await;
        assert!(result.is_ok());
        let created = repo.created.lock().expect("lock");
        assert_eq!(created.len(), 1);
    }

    #[tokio::test]
    async fn test_audit_service_log_deny_with_logger_covers_debug_macro_args() {
        ensure_test_logger();
        let repo = Arc::new(MockAuditLogRepository::new());
        let service = AuditService::new(repo.clone());
        let api_key_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let scope = ApiKeyScope::read_only();
        service
            .log_deny(
                "scrape.create".to_string(),
                Some(api_key_id),
                Some(team_id),
                "denied for test".to_string(),
                Some(scope),
            )
            .await
            .expect("log_deny should succeed");
        let created = repo.created.lock().expect("lock");
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].decision, AuditDecision::Deny);
    }
}

/// Trait for AuditService - enables dependency injection
#[async_trait::async_trait]
pub trait AuditServiceTrait: Send + Sync {
    /// Create a new audit log entry
    async fn log(&self, entry: AuditLogEntry) -> Result<(), AuditServiceError>;

    /// Log an allow decision
    async fn log_allow(
        &self,
        action: String,
        api_key_id: Uuid,
        team_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<(), AuditServiceError>;

    /// Log a deny decision
    async fn log_deny(
        &self,
        action: String,
        api_key_id: Option<Uuid>,
        team_id: Option<Uuid>,
        reason: String,
        scope: Option<ApiKeyScope>,
    ) -> Result<(), AuditServiceError>;

    /// Get audit logs for an API Key
    async fn get_logs_for_key(
        &self,
        api_key_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError>;

    /// Get audit logs for a team
    async fn get_logs_for_team(
        &self,
        team_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError>;

    /// Get denied requests for an API Key
    async fn get_denied_requests(
        &self,
        api_key_id: Uuid,
        limit: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError>;
}

/// Service for managing audit logs
#[derive(Clone)]
pub struct AuditService<R: AuditLogRepository> {
    audit_repo: Arc<R>,
}

impl<R: AuditLogRepository> AuditService<R> {
    /// Create a new service
    pub fn new(audit_repo: Arc<R>) -> Self {
        Self { audit_repo }
    }

    /// Create a new audit log entry
    pub async fn log_internal(&self, entry: AuditLogEntry) -> Result<(), AuditServiceError> {
        debug!(
            "Creating audit log: action={}, decision={}",
            entry.requested_action, entry.decision
        );
        self.audit_repo
            .create(&entry)
            .await
            .map(|_| ())
            .map_err(Into::into)
    }

    /// Log an allow decision
    pub async fn log_allow_internal(
        &self,
        action: impl Into<String>,
        api_key_id: Uuid,
        team_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<(), AuditServiceError> {
        let entry = AuditLogBuilder::new(action, AuditDecision::Allow)
            .with_api_key_id(api_key_id)
            .with_team_id(team_id)
            .with_scope(scope)
            .build();

        self.log_internal(entry).await
    }

    /// Log a deny decision
    pub async fn log_deny_internal(
        &self,
        action: impl Into<String>,
        api_key_id: Option<Uuid>,
        team_id: Option<Uuid>,
        reason: impl Into<String>,
        scope: Option<ApiKeyScope>,
    ) -> Result<(), AuditServiceError> {
        // 用 maybe_with_* 保留 None 语义：未认证拒绝场景下 api_key_id/team_id 为 None，
        // 写入 NULL 而非 nil UUID，避免 find_by_api_key_id(nil_uuid) 误匹配。
        let entry = AuditLogBuilder::new(action, AuditDecision::Deny)
            .maybe_with_api_key_id(api_key_id)
            .maybe_with_team_id(team_id)
            .with_denial_reason(reason)
            .maybe_with_scope(scope)
            .build();

        self.log_internal(entry).await
    }

    /// Create a new audit log entry (public wrapper)
    pub async fn log(&self, entry: AuditLogEntry) -> Result<(), AuditServiceError> {
        self.log_internal(entry).await
    }

    /// Log an allow decision (public wrapper)
    pub async fn log_allow(
        &self,
        action: impl Into<String>,
        api_key_id: Uuid,
        team_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<(), AuditServiceError> {
        self.log_allow_internal(action, api_key_id, team_id, scope)
            .await
    }

    /// Log a deny decision (public wrapper)
    pub async fn log_deny(
        &self,
        action: impl Into<String>,
        api_key_id: Option<Uuid>,
        team_id: Option<Uuid>,
        reason: impl Into<String>,
        scope: Option<ApiKeyScope>,
    ) -> Result<(), AuditServiceError> {
        self.log_deny_internal(action, api_key_id, team_id, reason, scope)
            .await
    }

    /// Get audit logs for an API Key
    pub async fn get_logs_for_key(
        &self,
        api_key_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        self.audit_repo
            .find_by_api_key_id(api_key_id, limit, offset)
            .await
            .map_err(Into::into)
    }

    /// Get audit logs for a team
    pub async fn get_logs_for_team(
        &self,
        team_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        self.audit_repo
            .find_by_team_id(team_id, limit, offset)
            .await
            .map_err(Into::into)
    }

    /// Get denied requests for an API Key
    pub async fn get_denied_requests(
        &self,
        api_key_id: Uuid,
        limit: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        self.audit_repo
            .find_denied_for_key(api_key_id, limit)
            .await
            .map_err(Into::into)
    }

    /// Clean up old audit logs
    pub async fn cleanup_old_logs(&self, retention_days: i64) -> Result<u64, AuditServiceError> {
        self.audit_repo
            .cleanup_old_logs(retention_days)
            .await
            .map_err(Into::into)
    }
}

#[async_trait::async_trait]
impl<R: AuditLogRepository + 'static> AuditServiceTrait for AuditService<R> {
    async fn log(&self, entry: AuditLogEntry) -> Result<(), AuditServiceError> {
        self.log_internal(entry).await
    }

    async fn log_allow(
        &self,
        action: String,
        api_key_id: Uuid,
        team_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<(), AuditServiceError> {
        self.log_allow_internal(action, api_key_id, team_id, scope)
            .await
    }

    async fn log_deny(
        &self,
        action: String,
        api_key_id: Option<Uuid>,
        team_id: Option<Uuid>,
        reason: String,
        scope: Option<ApiKeyScope>,
    ) -> Result<(), AuditServiceError> {
        self.log_deny_internal(action, api_key_id, team_id, reason, scope)
            .await
    }

    async fn get_logs_for_key(
        &self,
        api_key_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        self.audit_repo
            .find_by_api_key_id(api_key_id, limit, offset)
            .await
            .map_err(Into::into)
    }

    async fn get_logs_for_team(
        &self,
        team_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        self.audit_repo
            .find_by_team_id(team_id, limit, offset)
            .await
            .map_err(Into::into)
    }

    async fn get_denied_requests(
        &self,
        api_key_id: Uuid,
        limit: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        self.audit_repo
            .find_denied_for_key(api_key_id, limit)
            .await
            .map_err(Into::into)
    }
}
