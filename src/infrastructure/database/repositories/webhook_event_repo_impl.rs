// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook event repository implementation using Sea-ORM with Mapper

use crate::domain::models::WebhookEvent;
use crate::domain::repositories::task_repository::RepositoryError;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::infrastructure::database::entities::webhook_event;
use crate::infrastructure::persistence::mappers::WebhookEventMapper;
use async_trait::async_trait;
use chrono::Utc;
use dbnexus::DbPool;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use std::sync::Arc;
use uuid::Uuid;

/// Webhook event repository implementation using Sea-ORM
#[derive(Clone)]
pub struct WebhookEventRepoImpl {
    /// Database pool
    pool: Arc<DbPool>,
}

impl WebhookEventRepoImpl {
    /// Create new webhook event repository instance
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WebhookEventRepository for WebhookEventRepoImpl {
    async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entity = WebhookEventMapper::to_entity(event);
        let active_model = webhook_event::ActiveModel::from(entity);

        active_model
            .insert(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(event.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<WebhookEvent>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entity = webhook_event::Entity::find_by_id(id)
            .one(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(entity.map(WebhookEventMapper::to_domain))
    }

    async fn find_pending(&self, limit: u64) -> Result<Vec<WebhookEvent>, RepositoryError> {
        let now = Utc::now();

        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        // Find pending events
        let pending = webhook_event::Entity::find()
            .filter(webhook_event::Column::Status.eq(webhook_event::SeaWebhookStatus::Pending))
            .limit(limit)
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        // Also get failed events that are ready for retry
        let failed_retry = webhook_event::Entity::find()
            .filter(webhook_event::Column::Status.eq(webhook_event::SeaWebhookStatus::Failed))
            .filter(webhook_event::Column::NextRetryAt.lt(now))
            .limit(limit)
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let mut events = pending;
        events.extend(failed_retry);

        Ok(WebhookEventMapper::to_domain_list(events))
    }

    async fn update(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let active_model = WebhookEventMapper::to_active_model(event);

        active_model
            .update(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(event.clone())
    }

    async fn find_by_team_id_paginated(
        &self,
        team_id: Uuid,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<WebhookEvent>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entities = webhook_event::Entity::find()
            .filter(webhook_event::Column::TeamId.eq(team_id))
            .order_by(webhook_event::Column::CreatedAt, sea_orm::Order::Desc)
            .limit(limit as u64)
            .offset(offset as u64)
            .all(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(WebhookEventMapper::to_domain_list(entities))
    }

    async fn count_by_team_id(&self, team_id: Uuid) -> Result<u64, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let count = webhook_event::Entity::find()
            .filter(webhook_event::Column::TeamId.eq(team_id))
            .count(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::create_test_db_pool;
    use crate::domain::models::webhook_model::{WebhookEventType, WebhookStatus};

    fn sample_webhook_event() -> WebhookEvent {
        WebhookEvent::with_all_fields(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            WebhookEventType::CrawlCompleted,
            serde_json::json!({"crawl_id": "c-1"}),
            "https://example.com/hook".to_string(),
            WebhookStatus::Pending,
            0,
            3,
            None,
            None,
            None,
            None,
            Utc::now(),
            Utc::now(),
            None,
        )
    }

    // ========== construction ==========

    #[test]
    fn test_new_creates_repository_instance() {
        let pool = create_test_db_pool();
        let repo = WebhookEventRepoImpl::new(pool);
        let _clone = repo.clone();
    }

    // ========== CRUD against real DB ==========

    #[tokio::test]
    async fn test_create_inserts_record() {
        let repo = WebhookEventRepoImpl::new(create_test_db_pool());
        let event = sample_webhook_event();
        let result = repo.create(&event).await;
        assert!(result.is_ok(), "create failed: {:?}", result.err());

        // Verify DB state: find_by_id should return the created event
        let found = repo
            .find_by_id(event.id)
            .await
            .expect("find_by_id failed")
            .expect("event should exist after create");
        assert_eq!(found.id, event.id);
        assert_eq!(found.team_id, event.team_id);
        assert_eq!(found.event_type, event.event_type);
        assert_eq!(found.webhook_url, event.webhook_url);
        assert_eq!(found.status, event.status);
    }

    #[tokio::test]
    async fn test_find_by_id_returns_none_for_unknown() {
        let repo = WebhookEventRepoImpl::new(create_test_db_pool());
        let result = repo.find_by_id(Uuid::new_v4()).await;
        assert!(result.is_ok(), "find_by_id failed: {:?}", result.err());
        assert!(result.unwrap().is_none(), "unknown id should return None");
    }

    #[tokio::test]
    async fn test_find_pending_returns_ok() {
        let repo = WebhookEventRepoImpl::new(create_test_db_pool());
        let result = repo.find_pending(10).await;
        assert!(result.is_ok(), "find_pending failed: {:?}", result.err());
        // find_pending may return events from other tests; we only verify it
        // does not error against a real DB.
    }

    #[tokio::test]
    async fn test_update_modifies_record() {
        let repo = WebhookEventRepoImpl::new(create_test_db_pool());
        let mut event = sample_webhook_event();
        // First create
        repo.create(&event).await.expect("create failed");
        // Then update with new status
        event.status = WebhookStatus::Delivered;
        let result = repo.update(&event).await;
        assert!(result.is_ok(), "update failed: {:?}", result.err());

        // Verify DB state: find_by_id should return updated status
        let found = repo
            .find_by_id(event.id)
            .await
            .expect("find_by_id failed")
            .expect("event should exist after update");
        assert_eq!(found.status, WebhookStatus::Delivered);
    }

    #[tokio::test]
    async fn test_find_by_team_id_paginated_returns_empty_for_unknown() {
        let repo = WebhookEventRepoImpl::new(create_test_db_pool());
        let result = repo.find_by_team_id_paginated(Uuid::new_v4(), 10, 0).await;
        assert!(
            result.is_ok(),
            "find_by_team_id_paginated failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "unknown team_id should return empty vec"
        );
    }

    #[tokio::test]
    async fn test_count_by_team_id_returns_zero_for_unknown() {
        let repo = WebhookEventRepoImpl::new(create_test_db_pool());
        let result = repo.count_by_team_id(Uuid::new_v4()).await;
        assert!(
            result.is_ok(),
            "count_by_team_id failed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap(), 0, "unknown team_id should return 0");
    }

    // ========== RepositoryError variant display exhaustive ==========

    #[test]
    fn test_repository_error_database_display() {
        let err = RepositoryError::Database(anyhow::anyhow!("connection refused"));
        let msg = format!("{}", err);
        assert!(msg.contains("Database error"));
        assert!(msg.contains("connection refused"));
    }

    #[test]
    fn test_repository_error_not_found_display() {
        let err = RepositoryError::NotFound;
        assert_eq!(format!("{}", err), "Record not found");
    }

    // ========== From<sea_orm::DbErr> exhaustive ==========

    #[test]
    fn test_repository_error_from_dberr_record_not_found() {
        let db_err = sea_orm::DbErr::RecordNotFound("event missing".to_string());
        let repo_err: RepositoryError = db_err.into();
        match repo_err {
            RepositoryError::Database(_) => {}
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_repository_error_from_dberr_query_runtime() {
        let db_err =
            sea_orm::DbErr::Query(sea_orm::RuntimeErr::Internal("syntax error".to_string()));
        let repo_err: RepositoryError = db_err.into();
        match repo_err {
            RepositoryError::Database(_) => {}
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_repository_error_from_dberr_connection_acquire() {
        let db_err = sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout);
        let repo_err: RepositoryError = db_err.into();
        match repo_err {
            RepositoryError::Database(_) => {}
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_repository_error_from_dberr_record_not_inserted() {
        let db_err = sea_orm::DbErr::RecordNotInserted;
        let repo_err: RepositoryError = db_err.into();
        match repo_err {
            RepositoryError::Database(_) => {}
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    // ========== Production conversion path: dbnexus::DbError -> anyhow -> RepositoryError ==========

    #[test]
    fn test_repository_error_from_dbnexus_db_error_connection_path() {
        // Mirrors the production `.map_err(|e| RepositoryError::Database(e.into()))` path
        let inner = sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout);
        let db_err = dbnexus::DbError::Connection(inner);
        let any_err: anyhow::Error = db_err.into();
        let repo_err = RepositoryError::Database(any_err);
        let msg = format!("{}", repo_err);
        assert!(msg.contains("Database error"));
    }

    #[test]
    fn test_repository_error_from_dbnexus_db_error_config_path() {
        let db_err = dbnexus::DbError::Config("invalid url".to_string());
        let any_err: anyhow::Error = db_err.into();
        let repo_err = RepositoryError::Database(any_err);
        let msg = format!("{}", repo_err);
        assert!(msg.contains("Database error"));
    }

    #[test]
    fn test_repository_error_from_dbnexus_db_error_permission_path() {
        let db_err = dbnexus::DbError::Permission("forbidden".to_string());
        let any_err: anyhow::Error = db_err.into();
        let repo_err = RepositoryError::Database(any_err);
        let msg = format!("{}", repo_err);
        assert!(msg.contains("Database error"));
    }

    #[test]
    fn test_repository_error_from_dbnexus_db_error_transaction_path() {
        let db_err = dbnexus::DbError::Transaction("deadlock".to_string());
        let any_err: anyhow::Error = db_err.into();
        let repo_err = RepositoryError::Database(any_err);
        let msg = format!("{}", repo_err);
        assert!(msg.contains("Database error"));
    }

    #[test]
    fn test_repository_error_from_dbnexus_db_error_migration_path() {
        let db_err = dbnexus::DbError::Migration("schema mismatch".to_string());
        let any_err: anyhow::Error = db_err.into();
        let repo_err = RepositoryError::Database(any_err);
        let msg = format!("{}", repo_err);
        assert!(msg.contains("Database error"));
    }
}
