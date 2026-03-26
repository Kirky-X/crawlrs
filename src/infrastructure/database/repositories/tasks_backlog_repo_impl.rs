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
    sea_query::Expr, ActiveModelTrait, ColumnTrait, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
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
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
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
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
        let result = TasksBacklogEntity::find_by_id(id)
            .one(conn)
            .await?;
        Ok(result.map(TasksBacklog::from))
    }

    async fn find_by_task_id(
        &self,
        task_id: Uuid,
    ) -> Result<Option<TasksBacklog>, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
        let result = TasksBacklogEntity::find()
            .filter(tasks_backlog::Column::TaskId.eq(task_id))
            .one(conn)
            .await?;
        Ok(result.map(TasksBacklog::from))
    }

    async fn update(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
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
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
        TasksBacklogEntity::delete_by_id(id)
            .exec(conn)
            .await?;
        Ok(())
    }

    async fn get_pending_tasks(
        &self,
        team_id: Option<Uuid>,
        limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
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
        
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
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
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
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
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
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
