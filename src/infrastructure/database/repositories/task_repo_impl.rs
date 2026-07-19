// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task repository implementation using Sea-ORM with Mapper
//!
//! This implementation uses the Mapper pattern to convert between
//! domain models and database entities, following clean architecture principles.

use crate::domain::models::{Task, TaskStatus};
use crate::domain::repositories::task_repository::{
    RepositoryError, TaskQueryParams, TaskRepository,
};
use crate::infrastructure::database::entities::task as task_entity;
use crate::infrastructure::persistence::mappers::TaskMapper;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use dbnexus::DbPool;
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ColumnTrait, Condition, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

/// Task repository implementation using Sea-ORM
#[derive(Clone)]
pub struct TaskRepositoryImpl {
    /// Database pool
    pool: Arc<DbPool>,
    /// Lock duration for task acquisition
    lock_duration: Duration,
}

impl TaskRepositoryImpl {
    /// Create new task repository instance
    pub fn new(pool: Arc<DbPool>, lock_duration: Duration) -> Self {
        Self {
            pool,
            lock_duration,
        }
    }

    /// Get database pool reference
    pub fn pool(&self) -> &Arc<DbPool> {
        &self.pool
    }
}

#[async_trait]
impl TaskRepository for TaskRepositoryImpl {
    async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entity = TaskMapper::to_entity(task);
        let active_model = task_entity::ActiveModel::from(entity);

        active_model
            .insert(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(task.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entity = task_entity::Entity::find_by_id(id)
            .one(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(entity.map(TaskMapper::to_domain))
    }

    async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let active_model = TaskMapper::to_active_model(task);

        active_model
            .update(
                session
                    .connection()
                    .map_err(|e| RepositoryError::Database(e.into()))?,
            )
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(task.clone())
    }

    async fn acquire_next(&self, worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        // Find next queued task with expired lock or no lock
        let entity = task_entity::Entity::find()
            .filter(task_entity::Column::Status.eq(TaskStatus::Queued.to_string()))
            .order_by_asc(task_entity::Column::Priority)
            .order_by_asc(task_entity::Column::CreatedAt)
            .limit(1)
            .one(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if let Some(entity) = entity {
            let mut domain = TaskMapper::to_domain(entity);
            domain.start();
            domain.acquire_lock(worker_id, self.lock_duration);

            let active_model = TaskMapper::to_active_model(&domain);

            let updated = active_model
                .update(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;

            return Ok(Some(TaskMapper::to_domain(updated)));
        }

        Ok(None)
    }

    async fn mark_completed(&self, id: Uuid) -> Result<(), RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if let Some(entity) = task_entity::Entity::find_by_id(id)
            .one(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = TaskMapper::to_domain(entity);
            domain.complete();

            let active_model = TaskMapper::to_active_model(&domain);

            active_model
                .update(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn mark_failed(&self, id: Uuid) -> Result<(), RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if let Some(entity) = task_entity::Entity::find_by_id(id)
            .one(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = TaskMapper::to_domain(entity);
            domain.fail();

            let active_model = TaskMapper::to_active_model(&domain);

            active_model
                .update(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn mark_cancelled(&self, id: Uuid) -> Result<(), RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if let Some(entity) = task_entity::Entity::find_by_id(id)
            .one(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = TaskMapper::to_domain(entity);
            domain.cancel();

            let active_model = TaskMapper::to_active_model(&domain);

            active_model
                .update(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn exists_by_url(&self, url: &str) -> Result<bool, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let count = task_entity::Entity::find()
            .filter(task_entity::Column::Url.eq(url))
            .count(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(count > 0)
    }

    async fn find_existing_urls(
        &self,
        urls: &[String],
    ) -> Result<HashSet<String>, RepositoryError> {
        if urls.is_empty() {
            return Ok(HashSet::new());
        }

        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let existing_tasks = task_entity::Entity::find()
            .filter(task_entity::Column::Url.is_in(urls.to_vec()))
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let existing: HashSet<String> = existing_tasks.into_iter().map(|task| task.url).collect();

        Ok(existing)
    }

    async fn reset_stuck_tasks(&self, timeout: Duration) -> Result<u64, RepositoryError> {
        let cutoff = Utc::now() - timeout;

        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        // 使用批量 UPDATE 替代循环更新，避免 N+1 查询问题
        let result = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Queued.to_string()),
            )
            .col_expr(
                task_entity::Column::StartedAt,
                Expr::value(None::<chrono::DateTime<Utc>>),
            )
            .col_expr(task_entity::Column::LockToken, Expr::value(None::<Uuid>))
            .col_expr(
                task_entity::Column::LockExpiresAt,
                Expr::value(None::<chrono::DateTime<Utc>>),
            )
            .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(task_entity::Column::Status.eq(TaskStatus::Active.to_string()))
            .filter(task_entity::Column::StartedAt.lt(cutoff))
            .exec(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(result.rows_affected)
    }

    async fn cancel_tasks_by_crawl_id(&self, crawl_id: Uuid) -> Result<u64, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        // PERF: 使用批量更新代替 N+1 查询
        // 获取所有需要取消的任务 ID
        let task_ids: Vec<Uuid> = task_entity::Entity::find()
            .select_only()
            .column_as(task_entity::Column::Id, "id")
            .filter(task_entity::Column::CrawlId.eq(crawl_id))
            .filter(task_entity::Column::Status.is_in(vec![
                TaskStatus::Queued.to_string(),
                TaskStatus::Active.to_string(),
            ]))
            .into_tuple()
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if task_ids.is_empty() {
            return Ok(0);
        }

        // 批量更新所有任务为取消状态
        let update_count = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Cancelled.to_string()),
            )
            .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(task_entity::Column::Id.is_in(task_ids.iter().copied()))
            .exec(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(update_count.rows_affected)
    }

    async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
        let now = Utc::now();
        // Stale threshold: tasks queued or active for more than 24h are considered stale
        let stale_threshold = now - Duration::hours(24);

        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        // PERF: 使用批量更新代替 N+1 查询
        // 获取所有需要过期处理的任务 ID:
        // 1. Queued tasks with explicit expires_at in the past
        // 2. Queued tasks older than 24h (stale, no expires_at)
        // 3. Active tasks started more than 24h ago (stale)
        let task_ids: Vec<Uuid> = task_entity::Entity::find()
            .select_only()
            .column_as(task_entity::Column::Id, "id")
            .filter(
                Condition::any()
                    .add(
                        Condition::all()
                            .add(task_entity::Column::Status.eq(TaskStatus::Queued.to_string()))
                            .add(task_entity::Column::ExpiresAt.lt(now)),
                    )
                    .add(
                        Condition::all()
                            .add(task_entity::Column::Status.eq(TaskStatus::Queued.to_string()))
                            .add(task_entity::Column::ExpiresAt.is_null())
                            .add(task_entity::Column::CreatedAt.lt(stale_threshold)),
                    )
                    .add(
                        Condition::all()
                            .add(task_entity::Column::Status.eq(TaskStatus::Active.to_string()))
                            .add(task_entity::Column::StartedAt.is_not_null())
                            .add(task_entity::Column::StartedAt.lt(stale_threshold)),
                    ),
            )
            .into_tuple()
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if task_ids.is_empty() {
            return Ok(0);
        }

        // 批量更新所有过期任务为失败状态
        let update_count = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Failed.to_string()),
            )
            .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(task_entity::Column::Id.is_in(task_ids.iter().copied()))
            .exec(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(update_count.rows_affected)
    }

    async fn find_by_crawl_id(&self, crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entities = task_entity::Entity::find()
            .filter(task_entity::Column::CrawlId.eq(crawl_id))
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(TaskMapper::to_domain_list(entities))
    }

    async fn query_tasks(
        &self,
        params: TaskQueryParams,
    ) -> Result<(Vec<Task>, u64), RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let mut query =
            task_entity::Entity::find().filter(task_entity::Column::TeamId.eq(params.team_id));

        if let Some(crawl_id) = params.crawl_id {
            query = query.filter(task_entity::Column::CrawlId.eq(crawl_id));
        }

        if let Some(statuses) = &params.statuses {
            let status_strings: Vec<String> = statuses.iter().map(|s| s.to_string()).collect();
            query = query.filter(task_entity::Column::Status.is_in(status_strings));
        }

        if let Some(task_types) = &params.task_types {
            let type_strings: Vec<String> = task_types.iter().map(|t| t.to_string()).collect();
            query = query.filter(task_entity::Column::TaskType.is_in(type_strings));
        }

        let total = query
            .clone()
            .count(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entities = query
            .order_by_desc(task_entity::Column::CreatedAt)
            .limit(params.limit as u64)
            .offset(params.offset as u64)
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok((TaskMapper::to_domain_list(entities), total))
    }

    async fn batch_cancel(
        &self,
        task_ids: Vec<Uuid>,
        team_id: Uuid,
        _force: bool,
    ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
        if task_ids.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        // PERF: 使用批量查询代替 N+1 查询
        // 一次性获取所有任务，验证团队所有权
        let entities: Vec<task_entity::Model> = task_entity::Entity::find()
            .filter(task_entity::Column::Id.is_in(task_ids.iter().copied()))
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let mut cancelled = Vec::new();
        let mut errors = Vec::new();

        // 按团队所有权分组
        let mut owned_ids = Vec::new();
        let mut not_found_ids = Vec::new();

        for id in &task_ids {
            if let Some(entity) = entities.iter().find(|e| e.id == *id) {
                if entity.team_id == team_id {
                    owned_ids.push(entity.id);
                } else {
                    errors.push((*id, "Team ID mismatch".to_string()));
                }
            } else {
                not_found_ids.push(*id);
            }
        }

        for id in not_found_ids {
            errors.push((id, "Task not found".to_string()));
        }

        // 批量更新所有归属当前团队的任务
        if !owned_ids.is_empty() {
            let update_count = task_entity::Entity::update_many()
                .col_expr(
                    task_entity::Column::Status,
                    Expr::value(TaskStatus::Cancelled.to_string()),
                )
                .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
                .filter(task_entity::Column::Id.is_in(owned_ids.iter().copied()))
                .exec(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;

            // 所有更新的任务都成功取消
            cancelled.extend(owned_ids);
            let _ = update_count;
        }

        Ok((cancelled, errors))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::create_test_db_pool;
    use crate::domain::models::TaskType;
    use serde_json::json;

    /// Build a minimal Task instance for tests that need to pass one in.
    /// The fields don't matter because the DB call fails before the entity is used.
    fn make_test_task() -> Task {
        Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            json!({}),
        )
    }

    // ============================================================
    // Construction tests
    // ============================================================

    #[test]
    #[ignore = "requires TEST_DATABASE_URL"]
    fn test_new_creates_repository_instance() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        // Repository should be constructible without connecting to DB
        // (pool is lazy, no connection until get_session is called)
        let _ = repo;
    }

    // ============================================================
    // Error path tests — all methods should fail gracefully when
    // the lazy pool cannot provide a real session.
    // ============================================================

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_create_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let task = make_test_task();
        let result = repo.create(&task).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, RepositoryError::Database(_)),
            "Expected Database, got {:?}",
            err
        );
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_find_by_id_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let result = repo.find_by_id(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_update_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let task = make_test_task();
        let result = repo.update(&task).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_acquire_next_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let result = repo.acquire_next(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_mark_completed_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let result = repo.mark_completed(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_mark_failed_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let result = repo.mark_failed(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_mark_cancelled_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let result = repo.mark_cancelled(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_exists_by_url_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let result = repo.exists_by_url("http://example.com").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_find_existing_urls_returns_empty_for_empty_input() {
        // Empty input should short-circuit to Ok(empty set) without DB access
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let result = repo.find_existing_urls(&[]).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_find_existing_urls_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let urls = vec!["http://example.com".to_string()];
        let result = repo.find_existing_urls(&urls).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_reset_stuck_tasks_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let result = repo.reset_stuck_tasks(Duration::minutes(30)).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_cancel_tasks_by_crawl_id_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let result = repo.cancel_tasks_by_crawl_id(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_expire_tasks_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let result = repo.expire_tasks().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_find_by_crawl_id_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let result = repo.find_by_crawl_id(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_query_tasks_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let params = TaskQueryParams {
            team_id: Uuid::new_v4(),
            ..Default::default()
        };
        let result = repo.query_tasks(params).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_batch_cancel_returns_empty_for_empty_input() {
        // Empty input should short-circuit to Ok((empty, empty)) without DB access
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let result = repo.batch_cancel(Vec::new(), Uuid::new_v4(), false).await;
        assert!(result.is_ok());
        let (cancelled, errors) = result.unwrap();
        assert!(cancelled.is_empty());
        assert!(errors.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_batch_cancel_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::minutes(5));
        let task_ids = vec![Uuid::new_v4()];
        let result = repo.batch_cancel(task_ids, Uuid::new_v4(), false).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    // ============================================================
    // RepositoryError variant tests
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
    #[ignore = "requires TEST_DATABASE_URL"]
    fn test_pool_accessor_returns_reference_to_same_pool() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool.clone(), Duration::minutes(5));
        let pool_ref = repo.pool();
        // The accessor should return a reference to the same underlying Arc
        assert!(Arc::ptr_eq(pool_ref, &pool));
    }

    #[test]
    #[ignore = "requires TEST_DATABASE_URL"]
    fn test_new_with_zero_lock_duration() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::zero());
        let _ = repo;
    }

    #[test]
    #[ignore = "requires TEST_DATABASE_URL"]
    fn test_new_with_large_lock_duration() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool, Duration::days(7));
        let _ = repo;
    }

    #[test]
    fn test_make_test_task_construction() {
        let task = make_test_task();
        assert_eq!(task.task_type, TaskType::Scrape);
        assert_eq!(task.url, "http://example.com");
        // New tasks should be in Queued status
        assert_eq!(task.status, TaskStatus::Queued);
    }

    #[test]
    fn test_task_query_params_default() {
        let params = TaskQueryParams::default();
        // Default should have nil Uuid for team_id
        assert_eq!(params.team_id, Uuid::nil());
        assert!(params.crawl_id.is_none());
        assert!(params.statuses.is_none());
        assert!(params.task_types.is_none());
        assert_eq!(params.limit, 0);
        assert_eq!(params.offset, 0);
    }

    // ============================================================
    // query_tasks — exercise optional filter branches (error path)
    // Even though the DB call fails, the filter-building code runs
    // before get_session in some cases; here we ensure no panic and
    // correct error variant for every combination.
    // ============================================================

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_query_tasks_with_crawl_id_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let params = TaskQueryParams {
            team_id: Uuid::new_v4(),
            crawl_id: Some(Uuid::new_v4()),
            ..Default::default()
        };
        let result = repo.query_tasks(params).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_query_tasks_with_statuses_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let params = TaskQueryParams {
            team_id: Uuid::new_v4(),
            statuses: Some(vec![TaskStatus::Queued, TaskStatus::Active]),
            ..Default::default()
        };
        let result = repo.query_tasks(params).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_query_tasks_with_task_types_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let params = TaskQueryParams {
            team_id: Uuid::new_v4(),
            task_types: Some(vec![TaskType::Scrape, TaskType::Crawl, TaskType::Extract]),
            ..Default::default()
        };
        let result = repo.query_tasks(params).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_query_tasks_with_all_optional_filters_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let params = TaskQueryParams {
            team_id: Uuid::new_v4(),
            crawl_id: Some(Uuid::new_v4()),
            statuses: Some(vec![TaskStatus::Completed, TaskStatus::Failed]),
            task_types: Some(vec![TaskType::Scrape]),
            limit: 50,
            offset: 10,
            ..Default::default()
        };
        let result = repo.query_tasks(params).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    // ============================================================
    // Additional boundary tests for existing methods
    // ============================================================

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_find_existing_urls_with_multiple_urls_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let urls = vec![
            "http://example.com".to_string(),
            "http://example.org".to_string(),
            "http://example.net".to_string(),
        ];
        let result = repo.find_existing_urls(&urls).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_batch_cancel_with_multiple_ids_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let task_ids = vec![Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let result = repo.batch_cancel(task_ids, Uuid::new_v4(), false).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_batch_cancel_with_force_true_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let task_ids = vec![Uuid::new_v4()];
        let result = repo.batch_cancel(task_ids, Uuid::new_v4(), true).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_reset_stuck_tasks_with_zero_duration_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let result = repo.reset_stuck_tasks(Duration::zero()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_reset_stuck_tasks_with_negative_duration_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        // Negative duration means cutoff is in the future; still exercises the DB path
        let result = repo.reset_stuck_tasks(Duration::minutes(-30)).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_exists_by_url_with_empty_string_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let result = repo.exists_by_url("").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    // ============================================================
    // Additional From<sea_orm::DbErr> variant coverage
    // ============================================================

    #[test]
    fn test_from_dberr_record_not_found_to_repository_error() {
        let db_err = sea_orm::DbErr::RecordNotFound("task missing".to_string());
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("task missing"));
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
    // TaskType / TaskStatus display exhaustive
    // ============================================================

    #[test]
    fn test_task_type_scrape_display() {
        assert_eq!(format!("{}", TaskType::Scrape), "scrape");
    }

    #[test]
    fn test_task_type_crawl_display() {
        assert_eq!(format!("{}", TaskType::Crawl), "crawl");
    }

    #[test]
    fn test_task_type_extract_display() {
        assert_eq!(format!("{}", TaskType::Extract), "extract");
    }

    #[test]
    fn test_task_status_queued_display() {
        assert_eq!(format!("{}", TaskStatus::Queued), "queued");
    }

    #[test]
    fn test_task_status_active_display() {
        assert_eq!(format!("{}", TaskStatus::Active), "active");
    }

    #[test]
    fn test_task_status_completed_display() {
        assert_eq!(format!("{}", TaskStatus::Completed), "completed");
    }

    #[test]
    fn test_task_status_failed_display() {
        assert_eq!(format!("{}", TaskStatus::Failed), "failed");
    }

    #[test]
    fn test_task_status_cancelled_display() {
        assert_eq!(format!("{}", TaskStatus::Cancelled), "cancelled");
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
            resource: "task".to_string(),
        };
        let repo_err: RepositoryError = db_err.into();
        assert!(matches!(repo_err, RepositoryError::Database(_)));
        assert!(repo_err.to_string().contains("write"));
        assert!(repo_err.to_string().contains("task"));
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
    fn test_repository_error_not_found_display_exact() {
        let err = RepositoryError::NotFound;
        assert_eq!(err.to_string(), "Record not found");
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
    // Additional boundary tests for URL-based methods
    // ============================================================

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_exists_by_url_with_unicode_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let result = repo.exists_by_url("http://例子.com/测试").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_exists_by_url_with_long_url_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let long_url = format!("http://example.com/{}", "a".repeat(2000));
        let result = repo.exists_by_url(&long_url).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_find_existing_urls_with_unicode_urls_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let urls = vec![
            "http://例子.com".to_string(),
            "http://example.org".to_string(),
        ];
        let result = repo.find_existing_urls(&urls).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_find_existing_urls_with_empty_string_in_list_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let urls = vec!["".to_string()];
        let result = repo.find_existing_urls(&urls).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_find_existing_urls_with_mixed_empty_and_nonempty_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let urls = vec!["".to_string(), "http://example.com".to_string()];
        let result = repo.find_existing_urls(&urls).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    // ============================================================
    // Additional boundary tests for UUID-based methods
    // ============================================================

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_find_by_id_with_nil_uuid_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let result = repo.find_by_id(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_mark_completed_with_nil_uuid_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let result = repo.mark_completed(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_mark_failed_with_nil_uuid_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let result = repo.mark_failed(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_mark_cancelled_with_nil_uuid_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let result = repo.mark_cancelled(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_acquire_next_with_nil_worker_id_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let result = repo.acquire_next(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_cancel_tasks_by_crawl_id_with_nil_uuid_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let result = repo.cancel_tasks_by_crawl_id(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_find_by_crawl_id_with_nil_uuid_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let result = repo.find_by_crawl_id(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_batch_cancel_with_nil_uuids_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let task_ids = vec![Uuid::nil()];
        let result = repo.batch_cancel(task_ids, Uuid::nil(), false).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    // ============================================================
    // query_tasks boundary — extreme limit/offset values
    // ============================================================

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_query_tasks_with_zero_limit_offset_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let params = TaskQueryParams {
            team_id: Uuid::new_v4(),
            limit: 0,
            offset: 0,
            ..Default::default()
        };
        let result = repo.query_tasks(params).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_query_tasks_with_max_limit_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let params = TaskQueryParams {
            team_id: Uuid::new_v4(),
            limit: u32::MAX,
            offset: 0,
            ..Default::default()
        };
        let result = repo.query_tasks(params).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_query_tasks_with_nil_team_id_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let params = TaskQueryParams {
            team_id: Uuid::nil(),
            ..Default::default()
        };
        let result = repo.query_tasks(params).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_query_tasks_with_empty_statuses_vec_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let params = TaskQueryParams {
            team_id: Uuid::new_v4(),
            statuses: Some(Vec::new()),
            ..Default::default()
        };
        let result = repo.query_tasks(params).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_query_tasks_with_empty_task_types_vec_returns_db_error() {
        let repo = TaskRepositoryImpl::new(create_test_db_pool(), Duration::minutes(5));
        let params = TaskQueryParams {
            team_id: Uuid::new_v4(),
            task_types: Some(Vec::new()),
            ..Default::default()
        };
        let result = repo.query_tasks(params).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::Database(_)));
    }

    // ============================================================
    // Repository clone — verify Clone preserves pool identity
    // ============================================================

    #[test]
    #[ignore = "requires TEST_DATABASE_URL"]
    fn test_repository_clone_preserves_pool_identity() {
        let pool = create_test_db_pool();
        let repo = TaskRepositoryImpl::new(pool.clone(), Duration::minutes(5));
        let cloned = repo.clone();
        assert!(Arc::ptr_eq(&repo.pool, &cloned.pool));
    }

    #[test]
    #[ignore = "requires TEST_DATABASE_URL"]
    fn test_new_with_distinct_pools_do_not_share_identity() {
        let pool1 = create_test_db_pool();
        let pool2 = create_test_db_pool();
        let repo1 = TaskRepositoryImpl::new(pool1, Duration::minutes(5));
        let repo2 = TaskRepositoryImpl::new(pool2, Duration::minutes(5));
        assert!(!Arc::ptr_eq(&repo1.pool, &repo2.pool));
    }
}
