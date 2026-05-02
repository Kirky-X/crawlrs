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
    sea_query::Expr, ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
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

        let entity = TaskMapper::to_entity(task);
        let active_model = task_entity::ActiveModel::from(entity);

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

            let updated_entity = TaskMapper::to_entity(&domain);
            let active_model = task_entity::ActiveModel::from(updated_entity);

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

            let updated_entity = TaskMapper::to_entity(&domain);
            let active_model = task_entity::ActiveModel::from(updated_entity);

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

            let updated_entity = TaskMapper::to_entity(&domain);
            let active_model = task_entity::ActiveModel::from(updated_entity);

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

            let updated_entity = TaskMapper::to_entity(&domain);
            let active_model = task_entity::ActiveModel::from(updated_entity);

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

        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        // PERF: 使用批量更新代替 N+1 查询
        // 获取所有需要过期处理的任务 ID
        let task_ids: Vec<Uuid> = task_entity::Entity::find()
            .select_only()
            .column_as(task_entity::Column::Id, "id")
            .filter(task_entity::Column::Status.eq(TaskStatus::Queued.to_string()))
            .filter(task_entity::Column::ExpiresAt.lt(now))
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
