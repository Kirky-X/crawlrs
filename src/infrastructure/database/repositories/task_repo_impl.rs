// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task repository implementation using Sea-ORM with Mapper
//!
//! This implementation uses the Mapper pattern to convert between
//! domain models and database entities, following clean architecture principles.

use crate::domain::models::{Task, TaskStatus, TaskType};
use crate::domain::repositories::task_repository::{
    RepositoryError, TaskQueryParams, TaskRepository,
};
use crate::infrastructure::database::entities::task as task_entity;
use crate::infrastructure::persistence::mappers::TaskMapper;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

/// Task repository implementation using Sea-ORM
#[derive(Clone)]
pub struct TaskRepositoryImpl {
    /// Database connection
    db: Arc<DatabaseConnection>,
    /// Lock duration for task acquisition
    lock_duration: Duration,
}

impl TaskRepositoryImpl {
    /// Create new task repository instance
    pub fn new(db: Arc<DatabaseConnection>, lock_duration: Duration) -> Self {
        Self { db, lock_duration }
    }

    /// Get database connection reference
    pub fn db(&self) -> &Arc<DatabaseConnection> {
        &self.db
    }
}

#[async_trait]
impl TaskRepository for TaskRepositoryImpl {
    async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
        let entity = TaskMapper::to_entity(task);
        let active_model = task_entity::ActiveModel::from(entity);

        active_model
            .insert(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(task.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
        let entity = task_entity::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(entity.map(TaskMapper::to_domain))
    }

    async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
        let entity = TaskMapper::to_entity(task);
        let active_model = task_entity::ActiveModel::from(entity);

        active_model
            .update(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(task.clone())
    }

    async fn acquire_next(&self, worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
        // Find next queued task with expired lock or no lock
        let entity = task_entity::Entity::find()
            .filter(task_entity::Column::Status.eq(TaskStatus::Queued.to_string()))
            .order_by_asc(task_entity::Column::Priority)
            .order_by_asc(task_entity::Column::CreatedAt)
            .limit(1)
            .one(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if let Some(entity) = entity {
            let mut domain = TaskMapper::to_domain(entity);
            domain.start();
            domain.acquire_lock(worker_id, self.lock_duration);

            let updated_entity = TaskMapper::to_entity(&domain);
            let active_model = task_entity::ActiveModel::from(updated_entity);

            let updated = active_model
                .update(self.db.as_ref())
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;

            return Ok(Some(TaskMapper::to_domain(updated)));
        }

        Ok(None)
    }

    async fn mark_completed(&self, id: Uuid) -> Result<(), RepositoryError> {
        if let Some(entity) = task_entity::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = TaskMapper::to_domain(entity);
            domain.complete();

            let updated_entity = TaskMapper::to_entity(&domain);
            let active_model = task_entity::ActiveModel::from(updated_entity);

            active_model
                .update(self.db.as_ref())
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn mark_failed(&self, id: Uuid) -> Result<(), RepositoryError> {
        if let Some(entity) = task_entity::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = TaskMapper::to_domain(entity);
            domain.fail();

            let updated_entity = TaskMapper::to_entity(&domain);
            let active_model = task_entity::ActiveModel::from(updated_entity);

            active_model
                .update(self.db.as_ref())
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn mark_cancelled(&self, id: Uuid) -> Result<(), RepositoryError> {
        if let Some(entity) = task_entity::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = TaskMapper::to_domain(entity);
            domain.cancel();

            let updated_entity = TaskMapper::to_entity(&domain);
            let active_model = task_entity::ActiveModel::from(updated_entity);

            active_model
                .update(self.db.as_ref())
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn exists_by_url(&self, url: &str) -> Result<bool, RepositoryError> {
        let count = task_entity::Entity::find()
            .filter(task_entity::Column::Url.eq(url))
            .count(self.db.as_ref())
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

        let existing_tasks = task_entity::Entity::find()
            .filter(task_entity::Column::Url.is_in(urls.to_vec()))
            .all(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let existing: HashSet<String> = existing_tasks.into_iter().map(|task| task.url).collect();

        Ok(existing)
    }

    async fn reset_stuck_tasks(&self, timeout: Duration) -> Result<u64, RepositoryError> {
        let cutoff = Utc::now() - timeout;

        let stuck_tasks = task_entity::Entity::find()
            .filter(task_entity::Column::Status.eq(TaskStatus::Active.to_string()))
            .filter(task_entity::Column::StartedAt.lt(cutoff))
            .all(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let mut count = 0u64;

        for entity in stuck_tasks {
            let mut domain = TaskMapper::to_domain(entity);
            domain.status = TaskStatus::Queued;
            domain.started_at = None;
            domain.release_lock();

            let updated_entity = TaskMapper::to_entity(&domain);
            let active_model = task_entity::ActiveModel::from(updated_entity);

            active_model
                .update(self.db.as_ref())
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;

            count += 1;
        }

        Ok(count)
    }

    async fn cancel_tasks_by_crawl_id(&self, crawl_id: Uuid) -> Result<u64, RepositoryError> {
        let tasks = task_entity::Entity::find()
            .filter(task_entity::Column::CrawlId.eq(crawl_id))
            .filter(task_entity::Column::Status.is_in(vec![
                TaskStatus::Queued.to_string(),
                TaskStatus::Active.to_string(),
            ]))
            .all(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let mut count = 0u64;

        for entity in tasks {
            let mut domain = TaskMapper::to_domain(entity);
            domain.cancel();

            let updated_entity = TaskMapper::to_entity(&domain);
            let active_model = task_entity::ActiveModel::from(updated_entity);

            active_model
                .update(self.db.as_ref())
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;

            count += 1;
        }

        Ok(count)
    }

    async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
        let now = Utc::now();

        let tasks = task_entity::Entity::find()
            .filter(task_entity::Column::Status.eq(TaskStatus::Queued.to_string()))
            .filter(task_entity::Column::ExpiresAt.lt(now))
            .all(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let mut count = 0u64;

        for entity in tasks {
            let mut domain = TaskMapper::to_domain(entity);
            domain.fail();

            let updated_entity = TaskMapper::to_entity(&domain);
            let active_model = task_entity::ActiveModel::from(updated_entity);

            active_model
                .update(self.db.as_ref())
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;

            count += 1;
        }

        Ok(count)
    }

    async fn find_by_crawl_id(&self, crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
        let entities = task_entity::Entity::find()
            .filter(task_entity::Column::CrawlId.eq(crawl_id))
            .all(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(TaskMapper::to_domain_list(entities))
    }

    async fn query_tasks(
        &self,
        params: TaskQueryParams,
    ) -> Result<(Vec<Task>, u64), RepositoryError> {
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
            .count(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let entities = query
            .order_by_desc(task_entity::Column::CreatedAt)
            .limit(params.limit as u64)
            .offset(params.offset as u64)
            .all(self.db.as_ref())
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
        let mut cancelled = Vec::new();
        let mut errors = Vec::new();

        for id in task_ids {
            if let Some(entity) = task_entity::Entity::find_by_id(id)
                .one(self.db.as_ref())
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?
            {
                if entity.team_id == team_id {
                    let mut domain = TaskMapper::to_domain(entity);
                    domain.cancel();

                    let updated_entity = TaskMapper::to_entity(&domain);
                    let active_model = task_entity::ActiveModel::from(updated_entity);

                    match active_model.update(self.db.as_ref()).await {
                        Ok(_) => cancelled.push(id),
                        Err(e) => errors.push((id, e.to_string())),
                    }
                } else {
                    errors.push((id, "Team ID mismatch".to_string()));
                }
            } else {
                errors.push((id, "Task not found".to_string()));
            }
        }

        Ok((cancelled, errors))
    }
}
