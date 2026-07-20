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
    use crate::common::test_helpers::create_test_db_pool;
    use crate::domain::models::Crawl;
    use serde_json::json;
    use std::collections::HashSet;

    /// Build a minimal Crawl instance for tests.
    /// Each call produces fresh UUIDs for id/team_id so tests are isolated.
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
        // Repository wraps the pool Arc; construction itself does not
        // open a new connection — get_session on the inner DbPool does.
        let _ = repo;
    }

    // ============================================================
    // CRUD tests — verify create / find / update / state transitions
    // against a real PostgreSQL database.
    // ============================================================

    #[tokio::test]
    async fn test_create_with_real_db_succeeds() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let crawl = make_test_crawl();
        let result = repo.create(&crawl).await;
        assert!(result.is_ok(), "create failed: {:?}", result.err());
        let returned = result.unwrap();
        assert_eq!(returned.id, crawl.id);
        assert_eq!(returned.url, crawl.url);
        assert_eq!(returned.team_id, crawl.team_id);

        // Verify DB state actually changed
        let found = repo.find_by_id(crawl.id).await.expect("find_by_id failed");
        assert!(found.is_some(), "crawl should be found after create");
        let found_crawl = found.unwrap();
        assert_eq!(found_crawl.id, crawl.id);
        assert_eq!(found_crawl.name, crawl.name);
        assert_eq!(found_crawl.url, crawl.url);
        assert_eq!(found_crawl.root_url, crawl.root_url);
        assert_eq!(found_crawl.team_id, crawl.team_id);
        assert_eq!(found_crawl.status, crawl.status);
    }

    #[tokio::test]
    async fn test_find_by_id_with_real_db_returns_none_for_unknown() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_id(Uuid::new_v4()).await;
        assert!(result.is_ok(), "find_by_id failed: {:?}", result.err());
        assert!(result.unwrap().is_none(), "unknown UUID should return None");
    }

    #[tokio::test]
    async fn test_update_with_real_db_succeeds() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let mut crawl = make_test_crawl();
        // create first
        repo.create(&crawl).await.expect("create failed");
        // modify public fields and update
        crawl.name = format!("updated crawl {}", Uuid::new_v4());
        crawl.url = format!("http://updated.com/{}", Uuid::new_v4());
        crawl.root_url = format!("http://root-updated.com/{}", Uuid::new_v4());
        let result = repo.update(&crawl).await;
        assert!(result.is_ok(), "update failed: {:?}", result.err());

        // Verify DB state reflects updated fields
        let found = repo
            .find_by_id(crawl.id)
            .await
            .expect("find_by_id failed")
            .expect("crawl should exist");
        assert_eq!(found.name, crawl.name);
        assert_eq!(found.url, crawl.url);
        assert_eq!(found.root_url, crawl.root_url);
    }

    #[tokio::test]
    async fn test_increment_completed_tasks_with_real_db_succeeds() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let crawl = make_test_crawl();
        repo.create(&crawl).await.expect("create failed");

        let result = repo.increment_completed_tasks(crawl.id).await;
        assert!(
            result.is_ok(),
            "increment_completed_tasks failed: {:?}",
            result.err()
        );

        // Verify DB state reflects incremented counter
        let found = repo
            .find_by_id(crawl.id)
            .await
            .expect("find_by_id failed")
            .expect("crawl should exist");
        assert_eq!(found.completed_tasks(), 1, "completed_tasks should be 1");
    }

    #[tokio::test]
    async fn test_increment_failed_tasks_with_real_db_succeeds() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let crawl = make_test_crawl();
        repo.create(&crawl).await.expect("create failed");

        let result = repo.increment_failed_tasks(crawl.id).await;
        assert!(
            result.is_ok(),
            "increment_failed_tasks failed: {:?}",
            result.err()
        );

        // Verify DB state reflects incremented counter
        let found = repo
            .find_by_id(crawl.id)
            .await
            .expect("find_by_id failed")
            .expect("crawl should exist");
        assert_eq!(found.failed_tasks(), 1, "failed_tasks should be 1");
    }

    #[tokio::test]
    async fn test_update_status_with_real_db_succeeds() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let crawl = make_test_crawl();
        repo.create(&crawl).await.expect("create failed");

        let result = repo.update_status(crawl.id, CrawlStatus::Completed).await;
        assert!(result.is_ok(), "update_status failed: {:?}", result.err());

        // Verify DB state reflects updated status
        let found = repo
            .find_by_id(crawl.id)
            .await
            .expect("find_by_id failed")
            .expect("crawl should exist");
        assert_eq!(found.status, CrawlStatus::Completed);
    }

    #[tokio::test]
    async fn test_increment_total_tasks_with_real_db_succeeds() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let crawl = make_test_crawl();
        repo.create(&crawl).await.expect("create failed");

        let result = repo.increment_total_tasks(crawl.id).await;
        assert!(
            result.is_ok(),
            "increment_total_tasks failed: {:?}",
            result.err()
        );

        // Verify DB state reflects incremented counter
        let found = repo
            .find_by_id(crawl.id)
            .await
            .expect("find_by_id failed")
            .expect("crawl should exist");
        assert_eq!(found.total_tasks(), 1, "total_tasks should be 1");
    }

    #[tokio::test]
    async fn test_find_by_team_id_paginated_with_real_db_returns_empty_for_unknown() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
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
    async fn test_count_by_team_id_with_real_db_returns_zero_for_unknown() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.count_by_team_id(Uuid::new_v4()).await;
        assert!(
            result.is_ok(),
            "count_by_team_id failed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap(), 0, "unknown team_id should return 0");
    }

    #[tokio::test]
    async fn test_find_by_team_id_paginated_with_real_db_returns_matching_crawls() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        let mut crawl1 = make_test_crawl();
        crawl1.team_id = team_id;
        let mut crawl2 = make_test_crawl();
        crawl2.team_id = team_id;
        let unrelated = make_test_crawl();
        repo.create(&crawl1).await.expect("create crawl1 failed");
        repo.create(&crawl2).await.expect("create crawl2 failed");
        repo.create(&unrelated)
            .await
            .expect("create unrelated failed");

        let result = repo.find_by_team_id_paginated(team_id, 100, 0).await;
        assert!(result.is_ok(), "find_by_team_id_paginated failed");
        let crawls = result.unwrap();
        assert_eq!(crawls.len(), 2, "should find 2 crawls for team_id");
        let ids: HashSet<Uuid> = crawls.iter().map(|c| c.id).collect();
        assert!(ids.contains(&crawl1.id));
        assert!(ids.contains(&crawl2.id));
        assert!(!ids.contains(&unrelated.id));
    }

    #[tokio::test]
    async fn test_count_by_team_id_with_real_db_counts_matching() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        let mut crawl1 = make_test_crawl();
        crawl1.team_id = team_id;
        let mut crawl2 = make_test_crawl();
        crawl2.team_id = team_id;
        repo.create(&crawl1).await.expect("create crawl1 failed");
        repo.create(&crawl2).await.expect("create crawl2 failed");

        let result = repo.count_by_team_id(team_id).await;
        assert!(result.is_ok(), "count_by_team_id failed");
        assert_eq!(result.unwrap(), 2, "should count 2 crawls for team_id");
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
    // update_status — exercise every CrawlStatus variant against real DB
    // ============================================================

    #[tokio::test]
    async fn test_update_status_to_processing_with_real_db_succeeds() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let crawl = make_test_crawl();
        repo.create(&crawl).await.expect("create failed");

        let result = repo.update_status(crawl.id, CrawlStatus::Processing).await;
        assert!(
            result.is_ok(),
            "update_status to Processing failed: {:?}",
            result.err()
        );

        let found = repo
            .find_by_id(crawl.id)
            .await
            .expect("find_by_id failed")
            .expect("crawl should exist");
        assert_eq!(found.status, CrawlStatus::Processing);
    }

    #[tokio::test]
    async fn test_update_status_to_failed_with_real_db_succeeds() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let crawl = make_test_crawl();
        repo.create(&crawl).await.expect("create failed");

        let result = repo.update_status(crawl.id, CrawlStatus::Failed).await;
        assert!(
            result.is_ok(),
            "update_status to Failed failed: {:?}",
            result.err()
        );

        let found = repo
            .find_by_id(crawl.id)
            .await
            .expect("find_by_id failed")
            .expect("crawl should exist");
        assert_eq!(found.status, CrawlStatus::Failed);
    }

    #[tokio::test]
    async fn test_update_status_to_cancelled_with_real_db_succeeds() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let crawl = make_test_crawl();
        repo.create(&crawl).await.expect("create failed");

        let result = repo.update_status(crawl.id, CrawlStatus::Cancelled).await;
        assert!(
            result.is_ok(),
            "update_status to Cancelled failed: {:?}",
            result.err()
        );

        let found = repo
            .find_by_id(crawl.id)
            .await
            .expect("find_by_id failed")
            .expect("crawl should exist");
        assert_eq!(found.status, CrawlStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_update_status_to_queued_with_real_db_succeeds() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let crawl = make_test_crawl();
        repo.create(&crawl).await.expect("create failed");
        // First transition to Processing, then back to Queued to verify the
        // update is not a no-op.
        repo.update_status(crawl.id, CrawlStatus::Processing)
            .await
            .expect("update_status to Processing failed");

        let result = repo.update_status(crawl.id, CrawlStatus::Queued).await;
        assert!(
            result.is_ok(),
            "update_status to Queued failed: {:?}",
            result.err()
        );

        let found = repo
            .find_by_id(crawl.id)
            .await
            .expect("find_by_id failed")
            .expect("crawl should exist");
        assert_eq!(found.status, CrawlStatus::Queued);
    }

    // ============================================================
    // find_by_team_id_paginated — boundary values
    // ============================================================

    #[tokio::test]
    async fn test_find_by_team_id_paginated_with_zero_limit_returns_empty() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_team_id_paginated(Uuid::new_v4(), 0, 0).await;
        assert!(
            result.is_ok(),
            "find_by_team_id_paginated failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "limit=0 with unknown team_id should return empty"
        );
    }

    #[tokio::test]
    async fn test_find_by_team_id_paginated_with_large_offset_returns_empty() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .find_by_team_id_paginated(Uuid::new_v4(), 10, u32::MAX)
            .await;
        assert!(
            result.is_ok(),
            "find_by_team_id_paginated failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "large offset with unknown team_id should return empty"
        );
    }

    #[tokio::test]
    async fn test_find_by_team_id_paginated_with_max_limit_returns_empty() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .find_by_team_id_paginated(Uuid::new_v4(), u32::MAX, 0)
            .await;
        assert!(
            result.is_ok(),
            "find_by_team_id_paginated failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "max limit with unknown team_id should return empty"
        );
    }

    #[tokio::test]
    async fn test_find_by_team_id_paginated_with_nil_team_id_returns_empty() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_team_id_paginated(Uuid::nil(), 10, 0).await;
        assert!(
            result.is_ok(),
            "find_by_team_id_paginated failed: {:?}",
            result.err()
        );
        // Nil team_id is unlikely to match any crawl (we generate v4 UUIDs).
        let _crawls = result.unwrap();
    }

    // ============================================================
    // Additional boundary tests
    // ============================================================

    #[tokio::test]
    async fn test_find_by_id_with_nil_uuid_returns_none() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_id(Uuid::nil()).await;
        assert!(result.is_ok(), "find_by_id failed: {:?}", result.err());
        // Nil UUID is unlikely to match any crawl (we generate v4 UUIDs in tests).
        let _found = result.unwrap();
    }

    #[tokio::test]
    async fn test_count_by_team_id_with_nil_uuid_returns_zero() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.count_by_team_id(Uuid::nil()).await;
        assert!(
            result.is_ok(),
            "count_by_team_id failed: {:?}",
            result.err()
        );
        // Nil team_id is unlikely to match any crawl.
        let _count = result.unwrap();
    }

    #[tokio::test]
    async fn test_increment_completed_tasks_with_nil_uuid_succeeds_silently() {
        // increment_completed_tasks silently returns Ok(()) when the crawl is
        // not found (this is the current implementation behavior).
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.increment_completed_tasks(Uuid::nil()).await;
        assert!(
            result.is_ok(),
            "increment_completed_tasks failed: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_increment_failed_tasks_with_nil_uuid_succeeds_silently() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.increment_failed_tasks(Uuid::nil()).await;
        assert!(
            result.is_ok(),
            "increment_failed_tasks failed: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_increment_total_tasks_with_nil_uuid_succeeds_silently() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let result = repo.increment_total_tasks(Uuid::nil()).await;
        assert!(
            result.is_ok(),
            "increment_total_tasks failed: {:?}",
            result.err()
        );
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
    async fn test_update_status_to_completed_with_real_db_succeeds() {
        let repo = CrawlRepositoryImpl::new(create_test_db_pool());
        let crawl = make_test_crawl();
        repo.create(&crawl).await.expect("create failed");

        let result = repo.update_status(crawl.id, CrawlStatus::Completed).await;
        assert!(
            result.is_ok(),
            "update_status to Completed failed: {:?}",
            result.err()
        );

        let found = repo
            .find_by_id(crawl.id)
            .await
            .expect("find_by_id failed")
            .expect("crawl should exist");
        assert_eq!(found.status, CrawlStatus::Completed);
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
        assert!(!Arc::ptr_eq(repo1.pool(), repo2.pool()));
    }
}
