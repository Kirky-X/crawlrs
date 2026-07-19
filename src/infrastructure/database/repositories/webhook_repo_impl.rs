// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook repository implementation using Sea-ORM with Mapper

use crate::domain::models::Webhook;
use crate::domain::repositories::task_repository::RepositoryError;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::infrastructure::database::entities::webhook;
use crate::infrastructure::persistence::mappers::WebhookMapper;
use async_trait::async_trait;
use dbnexus::DbPool;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter};
use std::sync::Arc;
use uuid::Uuid;

/// Webhook repository implementation
#[derive(Clone)]
pub struct WebhookRepoImpl {
    pool: Arc<DbPool>,
}

impl WebhookRepoImpl {
    /// Create new webhook repository instance
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WebhookRepository for WebhookRepoImpl {
    async fn create(&self, webhook: &Webhook) -> Result<Webhook, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entity = WebhookMapper::to_entity(webhook);
        let active_model = webhook::ActiveModel::from(entity);

        active_model
            .insert(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(webhook.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Webhook>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entity = webhook::Entity::find_by_id(id)
            .one(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(entity.map(WebhookMapper::to_domain))
    }

    async fn find_by_team_id(&self, team_id: Uuid) -> Result<Vec<Webhook>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entities = webhook::Entity::find()
            .filter(webhook::Column::TeamId.eq(team_id))
            .all(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(WebhookMapper::to_domain_list(entities))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::create_test_db_pool;

    fn sample_webhook() -> Webhook {
        Webhook::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com/webhook".to_string(),
        )
    }

    // ========== construction ==========

    #[test]
    #[ignore = "requires TEST_DATABASE_URL"]
    fn test_new_creates_repository_instance() {
        let pool = create_test_db_pool();
        let repo = WebhookRepoImpl::new(pool);
        let _clone = repo.clone();
    }

    #[test]
    fn test_webhook_new_construction_does_not_panic() {
        let hook = sample_webhook();
        assert!(!hook.url.is_empty());
    }

    // ========== error paths (lazy pool: get_session fails) ==========

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_create_returns_error_with_real_db() {
        let repo = WebhookRepoImpl::new(create_test_db_pool());
        let webhook = sample_webhook();
        let result = repo.create(&webhook).await;
        assert!(
            result.is_err(),
            "create should fail without a real database"
        );
        match result.unwrap_err() {
            RepositoryError::Database(_) => {}
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_find_by_id_returns_error_with_real_db() {
        let repo = WebhookRepoImpl::new(create_test_db_pool());
        let result = repo.find_by_id(Uuid::new_v4()).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            RepositoryError::Database(_) => {}
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_find_by_team_id_returns_error_with_real_db() {
        let repo = WebhookRepoImpl::new(create_test_db_pool());
        let result = repo.find_by_team_id(Uuid::new_v4()).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            RepositoryError::Database(_) => {}
            other => panic!("expected Database variant, got {:?}", other),
        }
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
        let db_err = sea_orm::DbErr::RecordNotFound("webhook missing".to_string());
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
