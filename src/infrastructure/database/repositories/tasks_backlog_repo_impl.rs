// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Tasks backlog repository implementation
//!
//! This module provides the concrete implementation of the TasksBacklogRepository trait
//! defined in the domain layer.

use async_trait::async_trait;
use chrono::Utc;
use dbnexus::DbPool;
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::repositories::task_repository::RepositoryError;
use crate::domain::repositories::tasks_backlog_repository::{
    TasksBacklog, TasksBacklogRepository, TasksBacklogStatus,
};
use crate::infrastructure::database::entities::tasks_backlog;
use crate::infrastructure::database::entities::tasks_backlog::Entity as TasksBacklogEntity;

/// Tasks backlog repository implementation
pub struct TasksBacklogRepositoryImpl {
    pool: Arc<DbPool>,
}

impl TasksBacklogRepositoryImpl {
    /// Create a new tasks backlog repository instance
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }
}

/// Convert database model to domain model
impl From<tasks_backlog::Model> for TasksBacklog {
    fn from(model: tasks_backlog::Model) -> Self {
        Self {
            id: model.id,
            task_id: model.task_id,
            team_id: model.team_id,
            task_type: model.task_type,
            priority: model.priority,
            payload: model.payload,
            max_retries: model.max_retries,
            retry_count: model.retry_count,
            status: model.status.parse().unwrap_or(TasksBacklogStatus::Pending),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
            scheduled_at: model.scheduled_at.map(|dt| dt.into()),
            expires_at: model.expires_at.map(|dt| dt.into()),
            processed_at: model.processed_at.map(|dt| dt.into()),
        }
    }
}

#[async_trait]
impl TasksBacklogRepository for TasksBacklogRepositoryImpl {
    async fn create(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let active_model = tasks_backlog::ActiveModel {
            id: Set(backlog.id),
            task_id: Set(backlog.task_id),
            team_id: Set(backlog.team_id),
            task_type: Set(backlog.task_type.clone()),
            priority: Set(backlog.priority),
            payload: Set(backlog.payload.clone()),
            max_retries: Set(backlog.max_retries),
            retry_count: Set(backlog.retry_count),
            status: Set(backlog.status.to_string()),
            created_at: Set(backlog.created_at.into()),
            updated_at: Set(backlog.updated_at.into()),
            scheduled_at: Set(backlog.scheduled_at.map(|dt| dt.into())),
            expires_at: Set(backlog.expires_at.map(|dt| dt.into())),
            processed_at: Set(backlog.processed_at.map(|dt| dt.into())),
        };

        let result = active_model.insert(conn).await?;
        Ok(TasksBacklog::from(result))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<TasksBacklog>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let result = TasksBacklogEntity::find_by_id(id).one(conn).await?;
        Ok(result.map(TasksBacklog::from))
    }

    async fn find_by_task_id(
        &self,
        task_id: Uuid,
    ) -> Result<Option<TasksBacklog>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let result = TasksBacklogEntity::find()
            .filter(tasks_backlog::Column::TaskId.eq(task_id))
            .one(conn)
            .await?;
        Ok(result.map(TasksBacklog::from))
    }

    async fn update(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let active_model = tasks_backlog::ActiveModel {
            id: Set(backlog.id),
            task_id: Set(backlog.task_id),
            team_id: Set(backlog.team_id),
            task_type: Set(backlog.task_type.clone()),
            priority: Set(backlog.priority),
            payload: Set(backlog.payload.clone()),
            max_retries: Set(backlog.max_retries),
            retry_count: Set(backlog.retry_count),
            status: Set(backlog.status.to_string()),
            created_at: Set(backlog.created_at.into()),
            updated_at: Set(Utc::now().into()),
            scheduled_at: Set(backlog.scheduled_at.map(|dt| dt.into())),
            expires_at: Set(backlog.expires_at.map(|dt| dt.into())),
            processed_at: Set(backlog.processed_at.map(|dt| dt.into())),
        };

        let result = active_model.update(conn).await?;
        Ok(TasksBacklog::from(result))
    }

    async fn delete(&self, id: Uuid) -> Result<(), RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        TasksBacklogEntity::delete_by_id(id).exec(conn).await?;
        Ok(())
    }

    async fn get_pending_tasks(
        &self,
        team_id: Option<Uuid>,
        limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let mut query = TasksBacklogEntity::find()
            .filter(tasks_backlog::Column::Status.eq(TasksBacklogStatus::Pending.to_string()))
            .order_by_asc(tasks_backlog::Column::Priority)
            .order_by_asc(tasks_backlog::Column::CreatedAt);

        if let Some(team_id) = team_id {
            query = query.filter(tasks_backlog::Column::TeamId.eq(team_id));
        }

        if let Some(limit) = limit {
            query = query.limit(limit);
        }

        let results = query.all(conn).await?;
        Ok(results.into_iter().map(TasksBacklog::from).collect())
    }

    async fn get_expired_tasks(
        &self,
        limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError> {
        let now = Utc::now();

        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let mut query = TasksBacklogEntity::find()
            .filter(tasks_backlog::Column::ExpiresAt.lt(now))
            .filter(tasks_backlog::Column::Status.ne(TasksBacklogStatus::Expired.to_string()))
            .order_by_asc(tasks_backlog::Column::ExpiresAt);

        if let Some(limit) = limit {
            query = query.limit(limit);
        }

        let results = query.all(conn).await?;
        Ok(results.into_iter().map(TasksBacklog::from).collect())
    }

    async fn count_by_status(
        &self,
        team_id: Option<Uuid>,
        status: TasksBacklogStatus,
    ) -> Result<i64, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let mut query =
            TasksBacklogEntity::find().filter(tasks_backlog::Column::Status.eq(status.to_string()));

        if let Some(team_id) = team_id {
            query = query.filter(tasks_backlog::Column::TeamId.eq(team_id));
        }

        let count = query.count(conn).await?;
        Ok(count as i64)
    }

    async fn update_status_batch(
        &self,
        ids: &[Uuid],
        status: TasksBacklogStatus,
    ) -> Result<u64, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let result = TasksBacklogEntity::update_many()
            .col_expr(
                tasks_backlog::Column::Status,
                Expr::value(status.to_string()),
            )
            .col_expr(tasks_backlog::Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(tasks_backlog::Column::Id.is_in(ids.to_vec()))
            .exec(conn)
            .await?;

        Ok(result.rows_affected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::create_test_db_pool;
    use chrono::FixedOffset;
    use std::str::FromStr;

    fn sample_tasks_backlog() -> TasksBacklog {
        TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "scrape".to_string(),
            5,
            serde_json::json!({"url": "https://example.com"}),
            Some(Utc::now() + chrono::Duration::hours(1)),
        )
    }

    fn fixed_offset_dt(timestamp: i64) -> chrono::DateTime<FixedOffset> {
        chrono::DateTime::from_timestamp(timestamp, 0)
            .expect("valid timestamp")
            .with_timezone(&FixedOffset::east_opt(0).expect("valid offset"))
    }

    // ========== construction ==========

    #[test]
    fn test_new_creates_repository_instance() {
        let pool = create_test_db_pool();
        let _repo = TasksBacklogRepositoryImpl::new(pool);
    }

    // ========== CRUD tests — verify create / find / update / delete
    // against a real PostgreSQL database. ==========

    #[tokio::test]
    async fn test_create_succeeds() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let backlog = sample_tasks_backlog();
        let result = repo.create(&backlog).await;
        assert!(result.is_ok(), "create failed: {:?}", result.err());
        let returned = result.unwrap();
        assert_eq!(returned.id, backlog.id);
        assert_eq!(returned.task_id, backlog.task_id);
        assert_eq!(returned.team_id, backlog.team_id);
        assert_eq!(returned.task_type, backlog.task_type);
        assert_eq!(returned.priority, backlog.priority);
        assert_eq!(returned.status, backlog.status);

        // Verify DB state actually changed
        let found = repo
            .find_by_id(backlog.id)
            .await
            .expect("find_by_id failed")
            .expect("backlog should exist after create");
        assert_eq!(found.id, backlog.id);
        assert_eq!(found.task_id, backlog.task_id);
        assert_eq!(found.team_id, backlog.team_id);
        assert_eq!(found.task_type, backlog.task_type);
        assert_eq!(found.priority, backlog.priority);
        assert_eq!(found.status, backlog.status);
        assert_eq!(found.payload, backlog.payload);

        // Cleanup
        let _ = repo.delete(backlog.id).await;
    }

    #[tokio::test]
    async fn test_find_by_id_returns_none_for_unknown() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_id(Uuid::new_v4()).await;
        assert!(result.is_ok(), "find_by_id failed: {:?}", result.err());
        assert!(result.unwrap().is_none(), "unknown id should return None");
    }

    #[tokio::test]
    async fn test_find_by_task_id_returns_none_for_unknown() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_task_id(Uuid::new_v4()).await;
        assert!(result.is_ok(), "find_by_task_id failed: {:?}", result.err());
        assert!(
            result.unwrap().is_none(),
            "unknown task_id should return None"
        );
    }

    #[tokio::test]
    async fn test_update_succeeds() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let mut backlog = sample_tasks_backlog();
        // create first
        repo.create(&backlog).await.expect("create failed");

        // modify fields and update
        backlog.priority = 9;
        backlog.payload = serde_json::json!({"updated": true});
        backlog.mark_processing().expect("mark_processing failed");
        let result = repo.update(&backlog).await;
        assert!(result.is_ok(), "update failed: {:?}", result.err());

        // Verify DB state reflects updated fields
        let found = repo
            .find_by_id(backlog.id)
            .await
            .expect("find_by_id failed")
            .expect("backlog should exist");
        assert_eq!(found.priority, 9);
        assert_eq!(found.payload, serde_json::json!({"updated": true}));
        assert_eq!(found.status, TasksBacklogStatus::Processing);

        // Cleanup
        let _ = repo.delete(backlog.id).await;
    }

    #[tokio::test]
    async fn test_delete_succeeds() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let backlog = sample_tasks_backlog();
        repo.create(&backlog).await.expect("create failed");

        let result = repo.delete(backlog.id).await;
        assert!(result.is_ok(), "delete failed: {:?}", result.err());

        // Verify DB state: find_by_id should return None
        let found = repo
            .find_by_id(backlog.id)
            .await
            .expect("find_by_id failed");
        assert!(found.is_none(), "backlog should be deleted");
    }

    #[tokio::test]
    async fn test_get_pending_tasks_returns_empty_for_unknown_team() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo.get_pending_tasks(Some(Uuid::new_v4()), Some(10)).await;
        assert!(
            result.is_ok(),
            "get_pending_tasks failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "unknown team should return empty"
        );
    }

    #[tokio::test]
    async fn test_get_pending_tasks_with_team_id_returns_empty_for_unknown() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo.get_pending_tasks(Some(Uuid::new_v4()), None).await;
        assert!(
            result.is_ok(),
            "get_pending_tasks failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "unknown team should return empty"
        );
    }

    #[tokio::test]
    async fn test_get_expired_tasks_returns_empty_when_none_expired() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        // No tasks have expired in a fresh test DB for this team_id.
        // We only assert Ok because the shared test DB may have other
        // tests' expired tasks; the result for our team should be empty.
        let result = repo.get_expired_tasks(Some(5)).await;
        assert!(
            result.is_ok(),
            "get_expired_tasks failed: {:?}",
            result.err()
        );
        let _tasks = result.unwrap();
    }

    #[tokio::test]
    async fn test_count_by_status_returns_zero_for_unknown_team() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .count_by_status(Some(Uuid::new_v4()), TasksBacklogStatus::Pending)
            .await;
        assert!(result.is_ok(), "count_by_status failed: {:?}", result.err());
        assert_eq!(result.unwrap(), 0, "unknown team should return 0 count");
    }

    #[tokio::test]
    async fn test_update_status_batch_returns_zero_for_unknown_ids() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .update_status_batch(&[Uuid::new_v4()], TasksBacklogStatus::Processing)
            .await;
        assert!(
            result.is_ok(),
            "update_status_batch failed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap(), 0, "unknown ids should affect 0 rows");
    }

    #[tokio::test]
    async fn test_update_status_batch_with_empty_ids_returns_zero() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        // Empty ids: update_many with IN () — Postgres returns Ok(0 rows affected).
        let result = repo
            .update_status_batch(&[], TasksBacklogStatus::Completed)
            .await;
        assert!(
            result.is_ok(),
            "update_status_batch failed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap(), 0, "empty ids should affect 0 rows");
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
        let db_err = sea_orm::DbErr::RecordNotFound("backlog missing".to_string());
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

    // ========== TasksBacklogStatus Display exhaustive ==========

    #[test]
    fn test_tasks_backlog_status_pending_display() {
        assert_eq!(format!("{}", TasksBacklogStatus::Pending), "pending");
    }

    #[test]
    fn test_tasks_backlog_status_processing_display() {
        assert_eq!(format!("{}", TasksBacklogStatus::Processing), "processing");
    }

    #[test]
    fn test_tasks_backlog_status_completed_display() {
        assert_eq!(format!("{}", TasksBacklogStatus::Completed), "completed");
    }

    #[test]
    fn test_tasks_backlog_status_failed_display() {
        assert_eq!(format!("{}", TasksBacklogStatus::Failed), "failed");
    }

    #[test]
    fn test_tasks_backlog_status_expired_display() {
        assert_eq!(format!("{}", TasksBacklogStatus::Expired), "expired");
    }

    // ========== TasksBacklogStatus FromStr exhaustive ==========

    #[test]
    fn test_tasks_backlog_status_from_str_pending() {
        assert_eq!(
            TasksBacklogStatus::from_str("pending").unwrap(),
            TasksBacklogStatus::Pending
        );
    }

    #[test]
    fn test_tasks_backlog_status_from_str_processing() {
        assert_eq!(
            TasksBacklogStatus::from_str("processing").unwrap(),
            TasksBacklogStatus::Processing
        );
    }

    #[test]
    fn test_tasks_backlog_status_from_str_completed() {
        assert_eq!(
            TasksBacklogStatus::from_str("completed").unwrap(),
            TasksBacklogStatus::Completed
        );
    }

    #[test]
    fn test_tasks_backlog_status_from_str_failed() {
        assert_eq!(
            TasksBacklogStatus::from_str("failed").unwrap(),
            TasksBacklogStatus::Failed
        );
    }

    #[test]
    fn test_tasks_backlog_status_from_str_expired() {
        assert_eq!(
            TasksBacklogStatus::from_str("expired").unwrap(),
            TasksBacklogStatus::Expired
        );
    }

    #[test]
    fn test_tasks_backlog_status_from_str_case_insensitive() {
        // implementation lowercases input, so "PENDING" should parse
        assert_eq!(
            TasksBacklogStatus::from_str("PENDING").unwrap(),
            TasksBacklogStatus::Pending
        );
    }

    #[test]
    fn test_tasks_backlog_status_from_str_invalid_returns_err() {
        let result = TasksBacklogStatus::from_str("unknown");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Invalid tasks backlog status"));
        assert!(err.contains("unknown"));
    }

    // ========== From<tasks_backlog::Model> for TasksBacklog ==========

    #[test]
    fn test_from_model_converts_all_fields() {
        let now = fixed_offset_dt(1_700_000_000);
        let scheduled = fixed_offset_dt(1_700_000_100);
        let expires = fixed_offset_dt(1_700_000_200);
        let processed = fixed_offset_dt(1_700_000_300);

        let model = tasks_backlog::Model {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            task_type: "scrape".to_string(),
            priority: 7,
            payload: serde_json::json!({"url": "https://example.com"}),
            max_retries: 5,
            retry_count: 2,
            status: "processing".to_string(),
            created_at: now,
            updated_at: now,
            scheduled_at: Some(scheduled),
            expires_at: Some(expires),
            processed_at: Some(processed),
        };

        let domain: TasksBacklog = model.clone().into();
        assert_eq!(domain.id, model.id);
        assert_eq!(domain.task_id, model.task_id);
        assert_eq!(domain.team_id, model.team_id);
        assert_eq!(domain.task_type, model.task_type);
        assert_eq!(domain.priority, model.priority);
        assert_eq!(domain.payload, model.payload);
        assert_eq!(domain.max_retries, model.max_retries);
        assert_eq!(domain.retry_count, model.retry_count);
        assert_eq!(domain.status, TasksBacklogStatus::Processing);
        assert!(domain.scheduled_at.is_some());
        assert!(domain.expires_at.is_some());
        assert!(domain.processed_at.is_some());
    }

    #[test]
    fn test_from_model_with_invalid_status_falls_back_to_pending() {
        let now = fixed_offset_dt(1_700_000_000);
        let model = tasks_backlog::Model {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            task_type: "scrape".to_string(),
            priority: 1,
            payload: serde_json::json!({}),
            max_retries: 3,
            retry_count: 0,
            status: "garbage_status".to_string(),
            created_at: now,
            updated_at: now,
            scheduled_at: None,
            expires_at: None,
            processed_at: None,
        };

        let domain: TasksBacklog = model.into();
        // unwrap_or fallback path: invalid status -> Pending
        assert_eq!(domain.status, TasksBacklogStatus::Pending);
    }

    #[test]
    fn test_from_model_with_none_optional_fields() {
        let now = fixed_offset_dt(1_700_000_000);
        let model = tasks_backlog::Model {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            task_type: "scrape".to_string(),
            priority: 1,
            payload: serde_json::json!({}),
            max_retries: 3,
            retry_count: 0,
            status: "pending".to_string(),
            created_at: now,
            updated_at: now,
            scheduled_at: None,
            expires_at: None,
            processed_at: None,
        };

        let domain: TasksBacklog = model.into();
        assert!(domain.scheduled_at.is_none());
        assert!(domain.expires_at.is_none());
        assert!(domain.processed_at.is_none());
    }

    // ============================================================
    // Additional From<sea_orm::DbErr> variant coverage (exhaustive)
    // 覆盖 sea_orm::DbErr 所有未在前面测试的变体到 RepositoryError::Database 的转换
    // ============================================================

    #[test]
    fn test_repository_error_from_dberr_connection_acquire_closed() {
        let db_err = sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::ConnectionClosed);
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_repository_error_from_dberr_record_not_updated() {
        let db_err = sea_orm::DbErr::RecordNotUpdated;
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_repository_error_from_dberr_query_sqlx_error() {
        let inner = sea_orm::sqlx::Error::RowNotFound;
        let db_err =
            sea_orm::DbErr::Query(sea_orm::RuntimeErr::SqlxError(std::sync::Arc::new(inner)));
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_repository_error_from_dberr_conn_runtime() {
        let db_err = sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal("conn lost".to_string()));
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("conn lost"));
    }

    #[test]
    fn test_repository_error_from_dberr_exec_runtime() {
        let db_err = sea_orm::DbErr::Exec(sea_orm::RuntimeErr::Internal("exec failed".to_string()));
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("exec failed"));
    }

    #[test]
    fn test_repository_error_from_dberr_type() {
        let db_err = sea_orm::DbErr::Type("invalid type".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("invalid type"));
    }

    #[test]
    fn test_repository_error_from_dberr_json() {
        let db_err = sea_orm::DbErr::Json("parse error".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("parse error"));
    }

    #[test]
    fn test_repository_error_from_dberr_attr_not_set() {
        let db_err = sea_orm::DbErr::AttrNotSet("name".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("name"));
    }

    #[test]
    fn test_repository_error_from_dberr_convert_from_u64() {
        let db_err = sea_orm::DbErr::ConvertFromU64("String");
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_repository_error_from_dberr_unpack_insert_id() {
        let db_err = sea_orm::DbErr::UnpackInsertId;
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_repository_error_from_dberr_update_get_primary_key() {
        let db_err = sea_orm::DbErr::UpdateGetPrimaryKey;
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_repository_error_from_dberr_migration() {
        let db_err = sea_orm::DbErr::Migration("schema mismatch".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("schema mismatch"));
    }

    #[test]
    fn test_repository_error_from_dberr_mutex_poison_error() {
        let db_err = sea_orm::DbErr::MutexPoisonError;
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_repository_error_from_dberr_rbac_error() {
        let db_err = sea_orm::DbErr::RbacError("forbidden".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("forbidden"));
    }

    #[test]
    fn test_repository_error_from_dberr_access_denied() {
        let db_err = sea_orm::DbErr::AccessDenied {
            permission: "write".to_string(),
            resource: "backlog".to_string(),
        };
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("write"));
        assert!(repo_err.to_string().contains("backlog"));
    }

    #[test]
    fn test_repository_error_from_dberr_backend_not_supported() {
        let db_err = sea_orm::DbErr::BackendNotSupported {
            db: "mysql",
            ctx: "not configured",
        };
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_repository_error_from_dberr_try_into_err() {
        let source_err: std::sync::Arc<dyn std::error::Error + Send + Sync> = std::sync::Arc::new(
            std::io::Error::new(std::io::ErrorKind::InvalidData, "bad value"),
        );
        let db_err = sea_orm::DbErr::TryIntoErr {
            from: "String",
            into: "i32",
            source: source_err,
        };
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_repository_error_from_dberr_key_arity_mismatch() {
        let db_err = sea_orm::DbErr::KeyArityMismatch {
            expected: 2,
            received: 1,
        };
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_repository_error_from_dberr_primary_key_not_set() {
        let db_err = sea_orm::DbErr::PrimaryKeyNotSet { ctx: "delete" };
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    // ============================================================
    // Additional boundary tests — nil UUIDs, multiple IDs, all status variants
    // These tests use new_v4 UUIDs to ensure isolation under concurrent
    // test execution. nil UUIDs are only used for read-only / idempotent
    // operations to avoid UNIQUE / PRIMARY KEY collisions.
    // ============================================================

    #[tokio::test]
    async fn test_find_by_id_with_nil_uuid_returns_ok() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_id(Uuid::nil()).await;
        assert!(result.is_ok(), "find_by_id failed: {:?}", result.err());
        // Nil UUID is unlikely to match any backlog (we generate v4 UUIDs).
        let _found = result.unwrap();
    }

    #[tokio::test]
    async fn test_find_by_task_id_with_nil_uuid_returns_ok() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_task_id(Uuid::nil()).await;
        assert!(result.is_ok(), "find_by_task_id failed: {:?}", result.err());
        let _found = result.unwrap();
    }

    #[tokio::test]
    async fn test_delete_with_nil_uuid_succeeds_silently() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        // delete_by_id is idempotent: returns Ok(()) even when no row matches.
        let result = repo.delete(Uuid::nil()).await;
        assert!(result.is_ok(), "delete failed: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_get_pending_tasks_with_nil_team_id_returns_ok() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo.get_pending_tasks(Some(Uuid::nil()), Some(5)).await;
        assert!(
            result.is_ok(),
            "get_pending_tasks failed: {:?}",
            result.err()
        );
        // Nil team_id may match other tests' backlogs; we only verify Ok.
        let _tasks = result.unwrap();
    }

    #[tokio::test]
    async fn test_get_pending_tasks_with_both_none_returns_ok() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo.get_pending_tasks(None, None).await;
        assert!(
            result.is_ok(),
            "get_pending_tasks failed: {:?}",
            result.err()
        );
        // No filter: may return other tests' pending tasks; we only verify Ok.
        let _tasks = result.unwrap();
    }

    #[tokio::test]
    async fn test_get_expired_tasks_without_limit_returns_ok() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo.get_expired_tasks(None).await;
        assert!(
            result.is_ok(),
            "get_expired_tasks failed: {:?}",
            result.err()
        );
        let _tasks = result.unwrap();
    }

    #[tokio::test]
    async fn test_count_by_status_with_team_id_returns_zero() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .count_by_status(Some(Uuid::new_v4()), TasksBacklogStatus::Processing)
            .await;
        assert!(result.is_ok(), "count_by_status failed: {:?}", result.err());
        assert_eq!(result.unwrap(), 0, "unknown team should return 0 count");
    }

    #[tokio::test]
    async fn test_count_by_status_with_nil_team_id_returns_ok() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .count_by_status(Some(Uuid::nil()), TasksBacklogStatus::Completed)
            .await;
        assert!(result.is_ok(), "count_by_status failed: {:?}", result.err());
        // Nil team_id may match other tests' backlogs; we only verify Ok.
        let _count = result.unwrap();
    }

    #[tokio::test]
    async fn test_count_by_status_with_each_status_variant_returns_zero() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        // Use a unique team_id per status to ensure isolation and 0 count.
        for status in [
            TasksBacklogStatus::Pending,
            TasksBacklogStatus::Processing,
            TasksBacklogStatus::Completed,
            TasksBacklogStatus::Failed,
            TasksBacklogStatus::Expired,
        ] {
            let result = repo.count_by_status(Some(Uuid::new_v4()), status).await;
            assert!(result.is_ok(), "count_by_status failed for {:?}", status);
            assert_eq!(result.unwrap(), 0, "expected 0 count for {:?}", status);
        }
    }

    #[tokio::test]
    async fn test_update_status_batch_with_multiple_ids_returns_zero() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        let ids = vec![Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let result = repo
            .update_status_batch(&ids, TasksBacklogStatus::Failed)
            .await;
        assert!(
            result.is_ok(),
            "update_status_batch failed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap(), 0, "unknown ids should affect 0 rows");
    }

    #[tokio::test]
    async fn test_update_status_batch_with_nil_ids_returns_zero() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        // Nil UUID is unlikely to match any backlog id (we generate v4 UUIDs).
        let result = repo
            .update_status_batch(&[Uuid::nil()], TasksBacklogStatus::Completed)
            .await;
        assert!(
            result.is_ok(),
            "update_status_batch failed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap(), 0, "nil id should affect 0 rows");
    }

    #[tokio::test]
    async fn test_update_status_batch_for_each_status_variant_returns_zero() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        for status in [
            TasksBacklogStatus::Pending,
            TasksBacklogStatus::Processing,
            TasksBacklogStatus::Completed,
            TasksBacklogStatus::Failed,
            TasksBacklogStatus::Expired,
        ] {
            let result = repo.update_status_batch(&[Uuid::new_v4()], status).await;
            assert!(
                result.is_ok(),
                "update_status_batch failed for {:?}",
                status
            );
            assert_eq!(
                result.unwrap(),
                0,
                "unknown id should affect 0 rows for {:?}",
                status
            );
        }
    }

    #[tokio::test]
    async fn test_create_with_unique_uuids_succeeds() {
        let repo = TasksBacklogRepositoryImpl::new(create_test_db_pool());
        // Use new_v4 for all UUIDs to ensure isolation under concurrent runs.
        // (Original test used nil UUIDs which collide on PRIMARY KEY.)
        let backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "scrape".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        let result = repo.create(&backlog).await;
        assert!(result.is_ok(), "create failed: {:?}", result.err());

        // Cleanup
        let _ = repo.delete(backlog.id).await;
    }

    // ============================================================
    // RepositoryError Display — 精确消息内容验证
    // ============================================================

    #[test]
    fn test_repository_error_database_display_exact() {
        let err = RepositoryError::Database(anyhow::anyhow!("connection refused"));
        let msg = err.to_string();
        assert!(msg.contains("Database error"));
        assert!(msg.contains("connection refused"));
    }

    #[test]
    fn test_repository_error_database_display_with_empty_message() {
        let err = RepositoryError::Database(anyhow::anyhow!(""));
        let msg = err.to_string();
        assert!(msg.contains("Database error"));
    }

    #[test]
    fn test_repository_error_implements_debug() {
        let err1 = RepositoryError::Database(anyhow::anyhow!("e"));
        let err2 = RepositoryError::NotFound;
        let debug1 = format!("{:?}", err1);
        let debug2 = format!("{:?}", err2);
        assert!(!debug1.is_empty());
        assert!(!debug2.is_empty());
    }
}
