// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use chrono::Utc;
use dbnexus::DbPool;
use sea_orm::{ColumnTrait, EntityTrait, Order, QueryFilter, QueryOrder, QuerySelect, Set};
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::auth::{AuditDecision, AuditLogEntry};
use crate::domain::repositories::audit_log_repository::{AuditLogRepository, AuditRepositoryError};
use crate::infrastructure::database::entities::auth::audit_log::{
    Column as AuditColumn, Entity as AuditEntity,
};

#[derive(Clone)]
pub struct AuditLogRepositoryImpl {
    pool: Arc<DbPool>,
}

impl AuditLogRepositoryImpl {
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AuditLogRepository for AuditLogRepositoryImpl {
    async fn create(&self, entry: &AuditLogEntry) -> Result<AuditLogEntry, AuditRepositoryError> {
        let session = self.pool.get_session("admin").await?;

        let conn = session.connection()?;

        let entry_cloned = entry.clone();
        let metadata_value = serde_json::to_value(entry_cloned.metadata).unwrap_or_default();
        let scope_used_value = entry_cloned
            .scope_used
            .map(|s| serde_json::to_value(s).unwrap_or_default());
        let ip_address_value = entry_cloned.ip_address.map(|ip| ip.to_string());
        let active_model =
            crate::infrastructure::database::entities::auth::audit_log::ActiveModel {
                id: Set(entry_cloned.id),
                api_key_id: Set(entry_cloned.api_key_id),
                team_id: Set(entry_cloned.team_id),
                requested_action: Set(entry_cloned.requested_action),
                decision: Set(entry_cloned.decision.to_string()),
                denial_reason: Set(entry_cloned.denial_reason),
                scope_used: Set(scope_used_value),
                ip_address: Set(ip_address_value),
                trace_id: Set(entry_cloned.trace_id),
                user_agent: Set(entry_cloned.user_agent),
                request_path: Set(entry_cloned.request_path),
                request_method: Set(entry_cloned.request_method),
                metadata: Set(metadata_value),
                ..Default::default()
            };

        AuditEntity::insert(active_model).exec(conn).await?;
        Ok(entry.clone())
    }

    async fn find_by_api_key_id(
        &self,
        api_key_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError> {
        let session = self.pool.get_session("admin").await?;

        let conn = session.connection()?;

        let logs = AuditEntity::find()
            .filter(AuditColumn::ApiKeyId.eq(api_key_id))
            .order_by(AuditColumn::CreatedAt, Order::Desc)
            .limit(limit)
            .offset(offset)
            .all(conn)
            .await?;

        Ok(logs.into_iter().map(|l| l.into()).collect())
    }

    async fn find_by_team_id(
        &self,
        team_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError> {
        let session = self.pool.get_session("admin").await?;

        let conn = session.connection()?;

        let logs = AuditEntity::find()
            .filter(AuditColumn::TeamId.eq(team_id))
            .order_by(AuditColumn::CreatedAt, Order::Desc)
            .limit(limit)
            .offset(offset)
            .all(conn)
            .await?;

        Ok(logs.into_iter().map(|l| l.into()).collect())
    }

    async fn find_denied_for_key(
        &self,
        api_key_id: Uuid,
        limit: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError> {
        let session = self.pool.get_session("admin").await?;

        let conn = session.connection()?;

        let logs = AuditEntity::find()
            .filter(AuditColumn::ApiKeyId.eq(api_key_id))
            .filter(AuditColumn::Decision.eq(AuditDecision::Deny.to_string()))
            .order_by(AuditColumn::CreatedAt, Order::Desc)
            .limit(limit)
            .all(conn)
            .await?;

        Ok(logs.into_iter().map(|l| l.into()).collect())
    }

    async fn cleanup_old_logs(&self, retention_days: i64) -> Result<u64, AuditRepositoryError> {
        let cutoff = Utc::now() - chrono::Duration::days(retention_days);

        let session = self.pool.get_session("admin").await?;

        let conn = session.connection()?;

        let result = AuditEntity::delete_many()
            .filter(AuditColumn::CreatedAt.lt(cutoff))
            .exec(conn)
            .await?;

        Ok(result.rows_affected as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::auth::{ApiKeyScope, AuditDecision, AuditLogEntry};

    /// Build a lazy DbPool that does not actually connect; `get_session()` will
    /// fail at runtime, allowing us to exercise every error path in this
    /// repository without a real database.
    fn create_test_db_pool() -> Arc<DbPool> {
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool construction");
                let _guard = rt.enter();
                DbPool::try_from(&dbnexus::DbConfig::default())
                    .expect("failed to create lazy DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool construction thread panicked"))
        })
    }

    fn sample_scope() -> ApiKeyScope {
        ApiKeyScope {
            read: true,
            write: false,
            admin: false,
            search_limit: 100,
            scrape_limit: 50,
        }
    }

    fn sample_audit_log_entry() -> AuditLogEntry {
        AuditLogEntry {
            id: Uuid::new_v4(),
            api_key_id: Some(Uuid::new_v4()),
            team_id: Some(Uuid::new_v4()),
            requested_action: "crawl.create".to_string(),
            decision: AuditDecision::Allow,
            denial_reason: None,
            scope_used: Some(sample_scope()),
            ip_address: Some(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
            trace_id: Some(Uuid::new_v4()),
            user_agent: Some("test-agent/1.0".to_string()),
            request_path: Some("/api/v1/crawls".to_string()),
            request_method: Some("POST".to_string()),
            metadata: serde_json::json!({"request_id": "req-123"}),
            created_at: chrono::Utc::now(),
        }
    }

    // ========== construction ==========

    #[test]
    fn test_new_creates_repository_instance() {
        let pool = create_test_db_pool();
        let repo = AuditLogRepositoryImpl::new(pool);
        let _clone = repo.clone();
    }

    // ========== error paths (lazy pool: get_session fails) ==========

    #[tokio::test]
    async fn test_create_returns_error_without_real_db() {
        let repo = AuditLogRepositoryImpl::new(create_test_db_pool());
        let entry = sample_audit_log_entry();
        let result = repo.create(&entry).await;
        assert!(
            result.is_err(),
            "create should fail without a real database"
        );
        match result.unwrap_err() {
            AuditRepositoryError::DatabaseError(_) => {}
            other => panic!("expected DatabaseError variant, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_find_by_api_key_id_returns_error_without_real_db() {
        let repo = AuditLogRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_api_key_id(Uuid::new_v4(), 10, 0).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AuditRepositoryError::DatabaseError(_) => {}
            other => panic!("expected DatabaseError variant, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_find_by_team_id_returns_error_without_real_db() {
        let repo = AuditLogRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_team_id(Uuid::new_v4(), 10, 0).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AuditRepositoryError::DatabaseError(_) => {}
            other => panic!("expected DatabaseError variant, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_find_denied_for_key_returns_error_without_real_db() {
        let repo = AuditLogRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_denied_for_key(Uuid::new_v4(), 5).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AuditRepositoryError::DatabaseError(_) => {}
            other => panic!("expected DatabaseError variant, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_cleanup_old_logs_returns_error_without_real_db() {
        let repo = AuditLogRepositoryImpl::new(create_test_db_pool());
        let result = repo.cleanup_old_logs(30).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AuditRepositoryError::DatabaseError(_) => {}
            other => panic!("expected DatabaseError variant, got {:?}", other),
        }
    }

    // ========== AuditRepositoryError variant display exhaustive ==========

    #[test]
    fn test_audit_repository_error_database_error_display() {
        let err = AuditRepositoryError::DatabaseError(sea_orm::DbErr::RecordNotFound(
            "audit log missing".to_string(),
        ));
        let msg = format!("{}", err);
        assert!(msg.contains("Database error"));
        assert!(msg.contains("audit log missing"));
    }

    #[test]
    fn test_audit_repository_error_not_found_display() {
        let err = AuditRepositoryError::NotFound;
        assert_eq!(format!("{}", err), "Audit log not found");
    }

    // ========== From<sea_orm::DbErr> exhaustive via #[from] ==========

    #[test]
    fn test_audit_repository_error_from_dberr_record_not_found() {
        let db_err = sea_orm::DbErr::RecordNotFound("not found".to_string());
        let repo_err: AuditRepositoryError = db_err.into();
        match repo_err {
            AuditRepositoryError::DatabaseError(_) => {}
            other => panic!("expected DatabaseError variant, got {:?}", other),
        }
    }

    #[test]
    fn test_audit_repository_error_from_dberr_query_runtime() {
        let db_err =
            sea_orm::DbErr::Query(sea_orm::RuntimeErr::Internal("syntax error".to_string()));
        let repo_err: AuditRepositoryError = db_err.into();
        match repo_err {
            AuditRepositoryError::DatabaseError(_) => {}
            other => panic!("expected DatabaseError variant, got {:?}", other),
        }
    }

    #[test]
    fn test_audit_repository_error_from_dberr_connection_acquire() {
        let db_err = sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout);
        let repo_err: AuditRepositoryError = db_err.into();
        match repo_err {
            AuditRepositoryError::DatabaseError(_) => {}
            other => panic!("expected DatabaseError variant, got {:?}", other),
        }
    }

    #[test]
    fn test_audit_repository_error_from_dberr_record_not_inserted() {
        let db_err = sea_orm::DbErr::RecordNotInserted;
        let repo_err: AuditRepositoryError = db_err.into();
        match repo_err {
            AuditRepositoryError::DatabaseError(_) => {}
            other => panic!("expected DatabaseError variant, got {:?}", other),
        }
    }

    // ========== From<dbnexus::DbError> exhaustive coverage ==========

    #[test]
    fn test_audit_repository_error_from_dbnexus_db_error_connection() {
        let inner = sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout);
        let db_err = dbnexus::DbError::Connection(inner);
        let repo_err: AuditRepositoryError = db_err.into();
        match repo_err {
            AuditRepositoryError::DatabaseError(sea_orm::DbErr::ConnectionAcquire(_)) => {}
            other => panic!("expected DatabaseError(ConnectionAcquire), got {:?}", other),
        }
    }

    #[test]
    fn test_audit_repository_error_from_dbnexus_db_error_config() {
        let db_err = dbnexus::DbError::Config("invalid url".to_string());
        let repo_err: AuditRepositoryError = db_err.into();
        match repo_err {
            AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(msg)) => {
                assert!(msg.contains("Config"));
                assert!(msg.contains("invalid url"));
            }
            other => panic!("expected DatabaseError(Custom), got {:?}", other),
        }
    }

    #[test]
    fn test_audit_repository_error_from_dbnexus_db_error_permission() {
        let db_err = dbnexus::DbError::Permission("forbidden".to_string());
        let repo_err: AuditRepositoryError = db_err.into();
        match repo_err {
            AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(msg)) => {
                assert!(msg.contains("Permission"));
                assert!(msg.contains("forbidden"));
            }
            other => panic!("expected DatabaseError(Custom), got {:?}", other),
        }
    }

    #[test]
    fn test_audit_repository_error_from_dbnexus_db_error_transaction() {
        let db_err = dbnexus::DbError::Transaction("deadlock".to_string());
        let repo_err: AuditRepositoryError = db_err.into();
        match repo_err {
            AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(msg)) => {
                assert!(msg.contains("Transaction"));
                assert!(msg.contains("deadlock"));
            }
            other => panic!("expected DatabaseError(Custom), got {:?}", other),
        }
    }

    #[test]
    fn test_audit_repository_error_from_dbnexus_db_error_migration() {
        let db_err = dbnexus::DbError::Migration("schema mismatch".to_string());
        let repo_err: AuditRepositoryError = db_err.into();
        match repo_err {
            AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(msg)) => {
                assert!(msg.contains("Migration"));
                assert!(msg.contains("schema mismatch"));
            }
            other => panic!("expected DatabaseError(Custom), got {:?}", other),
        }
    }

    // ========== AuditDecision display ==========

    #[test]
    fn test_audit_decision_allow_display() {
        assert_eq!(format!("{}", AuditDecision::Allow), "ALLOW");
    }

    #[test]
    fn test_audit_decision_deny_display() {
        assert_eq!(format!("{}", AuditDecision::Deny), "DENY");
    }

    // ========== AuditLogEntry construction ==========

    #[test]
    fn test_audit_log_entry_construction_does_not_panic() {
        let entry = sample_audit_log_entry();
        assert_eq!(entry.decision, AuditDecision::Allow);
        assert_eq!(entry.requested_action, "crawl.create");
        assert!(entry.scope_used.is_some());
        assert!(entry.ip_address.is_some());
    }

    #[test]
    fn test_audit_log_entry_with_deny_decision() {
        let mut entry = sample_audit_log_entry();
        entry.decision = AuditDecision::Deny;
        entry.denial_reason = Some("insufficient scope".to_string());
        assert_eq!(entry.decision.to_string(), "DENY");
        assert!(entry.denial_reason.is_some());
    }
}
