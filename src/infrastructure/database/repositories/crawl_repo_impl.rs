// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl repository implementation using Sea-ORM with Mapper

use crate::domain::models::{Crawl, CrawlStatus};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::task_repository::RepositoryError;
use crate::infrastructure::database::entities::crawl;
use crate::infrastructure::persistence::mappers::CrawlMapper;
use async_trait::async_trait;
use dbnexus::DbPool;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use std::sync::Arc;
use uuid::Uuid;

/// Crawl repository implementation using Sea-ORM
pub struct CrawlRepositoryImpl {
    /// Database pool
    pool: Arc<DbPool>,
}

impl CrawlRepositoryImpl {
    /// Create new crawl repository instance
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }

    /// Get database pool reference
    pub fn pool(&self) -> &Arc<DbPool> {
        &self.pool
    }
}

#[async_trait]
impl CrawlRepository for CrawlRepositoryImpl {
    async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entity = CrawlMapper::to_entity(crawl);
        let active_model = crawl::ActiveModel::from(entity);

        active_model
            .insert(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(crawl.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entity = crawl::Entity::find_by_id(id)
            .one(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(entity.map(CrawlMapper::to_domain))
    }

    async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let active_model = CrawlMapper::to_active_model(crawl);

        active_model
            .update(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(crawl.clone())
    }

    async fn increment_completed_tasks(&self, id: Uuid) -> Result<(), RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if let Some(entity) = crawl::Entity::find_by_id(id)
            .one(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = CrawlMapper::to_domain(entity);
            domain.increment_completed_tasks();

            let active_model = CrawlMapper::to_active_model(&domain);

            active_model
                .update(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn increment_failed_tasks(&self, id: Uuid) -> Result<(), RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if let Some(entity) = crawl::Entity::find_by_id(id)
            .one(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = CrawlMapper::to_domain(entity);
            domain.increment_failed_tasks();

            let active_model = CrawlMapper::to_active_model(&domain);

            active_model
                .update(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn update_status(&self, id: Uuid, status: CrawlStatus) -> Result<(), RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if let Some(entity) = crawl::Entity::find_by_id(id)
            .one(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = CrawlMapper::to_domain(entity);
            domain.status = status;
            domain.updated_at = chrono::Utc::now();

            let active_model = CrawlMapper::to_active_model(&domain);

            active_model
                .update(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn increment_total_tasks(&self, id: Uuid) -> Result<(), RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if let Some(entity) = crawl::Entity::find_by_id(id)
            .one(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = CrawlMapper::to_domain(entity);
            domain.increment_total_tasks();

            let active_model = CrawlMapper::to_active_model(&domain);

            active_model
                .update(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn find_by_team_id_paginated(
        &self,
        team_id: Uuid,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Crawl>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entities = crawl::Entity::find()
            .filter(crawl::Column::TeamId.eq(team_id))
            .order_by_desc(crawl::Column::CreatedAt)
            .limit(limit as u64)
            .offset(offset as u64)
            .all(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(CrawlMapper::to_domain_list(entities))
    }

    async fn count_by_team_id(&self, team_id: Uuid) -> Result<u64, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let count = crawl::Entity::find()
            .filter(crawl::Column::TeamId.eq(team_id))
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
    use crate::domain::models::Crawl;
    use serde_json::json;

    /// Create a lazy DbPool that does not establish a real database connection.
    /// `get_session()` calls will fail, allowing us to test error paths without
    /// requiring a running PostgreSQL instance.
    fn create_test_db_pool() -> Arc<DbPool> {
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool construction");
                let _guard = rt.enter();
                dbnexus::DbPool::try_from(&dbnexus::DbConfig::default())
                    .expect("failed to create lazy DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool construction thread panicked"))
        })
    }

    /// Build a minimal Crawl instance for tests that need to pass one in.
    /// The fields don't matter because the DB call fails before the entity is used.
    fn make_test_crawl() -> Crawl {
        Crawl::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "test crawl".to_string(),
            "http://example.com".to_string(),
            "http://example.com".to_string(),
            json!({}),
        )
    }

    // ============================================================
    // Construction tests
    // ============================================================

    #[test]
    fn test_new_creates_repository_instance() {
        let pool = create_test_db_pool();
        let repo = CrawlRepositoryImpl::new(pool);
        // Repository should be constructible without connecting to DB
        // (pool is lazy, no connection until get_session is called)
        let _ = repo;
    }

    // ============================================================
    // Error path tests — all methods should fail gracefully when
    // the lazy pool cannot provide a real session.
    // ============================================================

    #[tokio::test]
    async fn test_create_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = CrawlRepositoryImpl::new(pool);
        let crawl = make_test_crawl();
        let result = repo.create(&crawl).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, RepositoryError::Database(_)),
            "Expected Database, got {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_find_by_id_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = CrawlRepositoryImpl::new(pool);
        let result = repo.find_by_id(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_update_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = CrawlRepositoryImpl::new(pool);
        let crawl = make_test_crawl();
        let result = repo.update(&crawl).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_increment_completed_tasks_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = CrawlRepositoryImpl::new(pool);
        let result = repo.increment_completed_tasks(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_increment_failed_tasks_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = CrawlRepositoryImpl::new(pool);
        let result = repo.increment_failed_tasks(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_update_status_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = CrawlRepositoryImpl::new(pool);
        let result = repo
            .update_status(Uuid::new_v4(), CrawlStatus::Completed)
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_increment_total_tasks_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = CrawlRepositoryImpl::new(pool);
        let result = repo.increment_total_tasks(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_find_by_team_id_paginated_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = CrawlRepositoryImpl::new(pool);
        let result = repo.find_by_team_id_paginated(Uuid::new_v4(), 10, 0).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_count_by_team_id_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = CrawlRepositoryImpl::new(pool);
        let result = repo.count_by_team_id(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    // ============================================================
    // RepositoryError variant tests (defined in task_repository)
    // ============================================================

    #[test]
    fn test_error_database_display() {
        let err = RepositoryError::Database(anyhow::anyhow!("conn refused"));
        assert!(err.to_string().contains("Database error"));
        assert!(err.to_string().contains("conn refused"));
    }

    #[test]
    fn test_error_not_found_display() {
        let err = RepositoryError::NotFound;
        assert!(err.to_string().contains("Record not found"));
    }

    #[test]
    fn test_from_dberr_to_repository_error() {
        let db_err = sea_orm::DbErr::Custom("query failed".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("query failed"));
    }

    // ============================================================
    // Additional construction & accessor tests
    // ============================================================

    #[test]
    fn test_pool_accessor_returns_reference_to_same_pool() {
        let pool = create_test_db_pool();
        let repo = CrawlRepositoryImpl::new(pool.clone());
        let pool_ref = repo.pool();
        assert!(Arc::ptr_eq(pool_ref, &pool));
    }

    #[test]
    fn test_make_test_crawl_construction() {
        let crawl = make_test_crawl();
        assert_eq!(crawl.name, "test crawl");
        assert_eq!(crawl.url, "http://example.com");
        assert_eq!(crawl.root_url, "http://example.com");
    }

    // ============================================================
    // update_status — exercise every CrawlStatus variant (error path)
    // ============================================================

    #[tokio::test]
    async fn test_update_status_to_processing_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .update_status(Uuid::new_v4(), CrawlStatus::Processing)
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_update_status_to_failed_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .update_status(Uuid::new_v4(), CrawlStatus::Failed)
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_update_status_to_cancelled_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .update_status(Uuid::new_v4(), CrawlStatus::Cancelled)
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_update_status_to_queued_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .update_status(Uuid::new_v4(), CrawlStatus::Queued)
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    // ============================================================
    // find_by_team_id_paginated — boundary values
    // ============================================================

    #[tokio::test]
    async fn test_find_by_team_id_paginated_with_zero_limit_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_team_id_paginated(Uuid::new_v4(), 0, 0).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_find_by_team_id_paginated_with_large_offset_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .find_by_team_id_paginated(Uuid::new_v4(), 10, u32::MAX)
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_find_by_team_id_paginated_with_max_limit_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .find_by_team_id_paginated(Uuid::new_v4(), u32::MAX, 0)
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_find_by_team_id_paginated_with_nil_team_id_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_team_id_paginated(Uuid::nil(), 10, 0).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    // ============================================================
    // Additional boundary tests
    // ============================================================

    #[tokio::test]
    async fn test_find_by_id_with_nil_uuid_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_id(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_count_by_team_id_with_nil_uuid_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.count_by_team_id(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_increment_completed_tasks_with_nil_uuid_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.increment_completed_tasks(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_increment_failed_tasks_with_nil_uuid_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.increment_failed_tasks(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    async fn test_increment_total_tasks_with_nil_uuid_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.increment_total_tasks(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    // ============================================================
    // Additional From<sea_orm::DbErr> variant coverage
    // ============================================================

    #[test]
    fn test_from_dberr_record_not_found_to_repository_error() {
        let db_err = sea_orm::DbErr::RecordNotFound("crawl missing".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("crawl missing"));
    }

    #[test]
    fn test_from_dberr_connection_acquire_to_repository_error() {
        let db_err = sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout);
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_from_dberr_record_not_inserted_to_repository_error() {
        let db_err = sea_orm::DbErr::RecordNotInserted;
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_from_dberr_query_runtime_to_repository_error() {
        let db_err =
            sea_orm::DbErr::Query(sea_orm::RuntimeErr::Internal("syntax error".to_string()));
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("syntax error"));
    }

    // ============================================================
    // CrawlStatus display exhaustive
    // ============================================================

    #[test]
    fn test_crawl_status_queued_display() {
        assert_eq!(format!("{}", CrawlStatus::Queued), "queued");
    }

    #[test]
    fn test_crawl_status_processing_display() {
        assert_eq!(format!("{}", CrawlStatus::Processing), "processing");
    }

    #[test]
    fn test_crawl_status_completed_display() {
        assert_eq!(format!("{}", CrawlStatus::Completed), "completed");
    }

    #[test]
    fn test_crawl_status_failed_display() {
        assert_eq!(format!("{}", CrawlStatus::Failed), "failed");
    }

    #[test]
    fn test_crawl_status_cancelled_display() {
        assert_eq!(format!("{}", CrawlStatus::Cancelled), "cancelled");
    }

    // ============================================================
    // RepositoryError::NotFound display
    // ============================================================

    #[test]
    fn test_error_not_found_display_exact() {
        let err = RepositoryError::NotFound;
        assert_eq!(format!("{}", err), "Record not found");
    }

    // ============================================================
    // Additional From<sea_orm::DbErr> variant coverage (exhaustive)
    // 覆盖 sea_orm::DbErr 所有未在前面测试的变体到 RepositoryError::Database 的转换
    // ============================================================

    #[test]
    fn test_from_dberr_connection_acquire_closed_to_repository_error() {
        let db_err = sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::ConnectionClosed);
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_from_dberr_record_not_updated_to_repository_error() {
        let db_err = sea_orm::DbErr::RecordNotUpdated;
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_from_dberr_query_sqlx_error_to_repository_error() {
        let inner = sea_orm::sqlx::Error::RowNotFound;
        let db_err =
            sea_orm::DbErr::Query(sea_orm::RuntimeErr::SqlxError(std::sync::Arc::new(inner)));
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_from_dberr_conn_runtime_to_repository_error() {
        let db_err = sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal("conn lost".to_string()));
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("conn lost"));
    }

    #[test]
    fn test_from_dberr_exec_runtime_to_repository_error() {
        let db_err = sea_orm::DbErr::Exec(sea_orm::RuntimeErr::Internal("exec failed".to_string()));
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("exec failed"));
    }

    #[test]
    fn test_from_dberr_type_to_repository_error() {
        let db_err = sea_orm::DbErr::Type("invalid type".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("invalid type"));
    }

    #[test]
    fn test_from_dberr_json_to_repository_error() {
        let db_err = sea_orm::DbErr::Json("parse error".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("parse error"));
    }

    #[test]
    fn test_from_dberr_attr_not_set_to_repository_error() {
        let db_err = sea_orm::DbErr::AttrNotSet("name".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("name"));
    }

    #[test]
    fn test_from_dberr_convert_from_u64_to_repository_error() {
        let db_err = sea_orm::DbErr::ConvertFromU64("String");
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_from_dberr_unpack_insert_id_to_repository_error() {
        let db_err = sea_orm::DbErr::UnpackInsertId;
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_from_dberr_update_get_primary_key_to_repository_error() {
        let db_err = sea_orm::DbErr::UpdateGetPrimaryKey;
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_from_dberr_migration_to_repository_error() {
        let db_err = sea_orm::DbErr::Migration("schema mismatch".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("schema mismatch"));
    }

    #[test]
    fn test_from_dberr_mutex_poison_error_to_repository_error() {
        let db_err = sea_orm::DbErr::MutexPoisonError;
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_from_dberr_rbac_error_to_repository_error() {
        let db_err = sea_orm::DbErr::RbacError("forbidden".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("forbidden"));
    }

    #[test]
    fn test_from_dberr_access_denied_to_repository_error() {
        let db_err = sea_orm::DbErr::AccessDenied {
            permission: "write".to_string(),
            resource: "crawl".to_string(),
        };
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("write"));
        assert!(repo_err.to_string().contains("crawl"));
    }

    #[test]
    fn test_from_dberr_backend_not_supported_to_repository_error() {
        let db_err = sea_orm::DbErr::BackendNotSupported {
            db: "mysql",
            ctx: "not configured",
        };
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_from_dberr_try_into_err_to_repository_error() {
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
    fn test_from_dberr_key_arity_mismatch_to_repository_error() {
        let db_err = sea_orm::DbErr::KeyArityMismatch {
            expected: 2,
            received: 1,
        };
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    #[test]
    fn test_from_dberr_primary_key_not_set_to_repository_error() {
        let db_err = sea_orm::DbErr::PrimaryKeyNotSet { ctx: "update" };
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
    }

    // ============================================================
    // Additional RepositoryError Display tests
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

    // ============================================================
    // update_status boundary — all CrawlStatus variants exhaustive
    // ============================================================

    #[tokio::test]
    async fn test_update_status_to_completed_returns_db_error() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .update_status(Uuid::new_v4(), CrawlStatus::Completed)
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    // ============================================================
    // RepositoryImpl accessor — verify pool identity is preserved
    // ============================================================

    #[test]
    fn test_new_with_distinct_pools_do_not_share_identity() {
        let pool1 = create_test_db_pool();
        let pool2 = create_test_db_pool();
        let repo1 = CrawlRepositoryImpl::new(pool1);
        let repo2 = CrawlRepositoryImpl::new(pool2);
        assert!(!Arc::ptr_eq(&repo1.pool(), &repo2.pool()));
    }
}
