// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use chrono::Utc;
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait,
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

pub struct TasksBacklogRepositoryImpl {
    db: Arc<DatabaseConnection>,
}

impl TasksBacklogRepositoryImpl {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl TasksBacklogRepository for TasksBacklogRepositoryImpl {
    async fn create(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
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

        let result = active_model.insert(self.db.as_ref()).await?;
        Ok(TasksBacklog::from(result))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<TasksBacklog>, RepositoryError> {
        let result = TasksBacklogEntity::find_by_id(id)
            .one(self.db.as_ref())
            .await?;
        Ok(result.map(TasksBacklog::from))
    }

    async fn find_by_task_id(
        &self,
        task_id: Uuid,
    ) -> Result<Option<TasksBacklog>, RepositoryError> {
        let result = TasksBacklogEntity::find()
            .filter(tasks_backlog::Column::TaskId.eq(task_id))
            .one(self.db.as_ref())
            .await?;
        Ok(result.map(TasksBacklog::from))
    }

    async fn update(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
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

        let result = active_model.update(self.db.as_ref()).await?;
        Ok(TasksBacklog::from(result))
    }

    async fn delete(&self, id: Uuid) -> Result<(), RepositoryError> {
        TasksBacklogEntity::delete_by_id(id)
            .exec(self.db.as_ref())
            .await?;
        Ok(())
    }

    async fn get_pending_tasks(
        &self,
        team_id: Option<Uuid>,
        limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError> {
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

        let results = query.all(self.db.as_ref()).await?;
        Ok(results.into_iter().map(TasksBacklog::from).collect())
    }

    async fn get_expired_tasks(
        &self,
        limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError> {
        let now = Utc::now();
        let mut query = TasksBacklogEntity::find()
            .filter(tasks_backlog::Column::ExpiresAt.lt(now))
            .filter(tasks_backlog::Column::Status.ne(TasksBacklogStatus::Expired.to_string()))
            .order_by_asc(tasks_backlog::Column::ExpiresAt);

        if let Some(limit) = limit {
            query = query.limit(limit);
        }

        let results = query.all(self.db.as_ref()).await?;
        Ok(results.into_iter().map(TasksBacklog::from).collect())
    }

    async fn count_by_status(
        &self,
        team_id: Option<Uuid>,
        status: TasksBacklogStatus,
    ) -> Result<i64, RepositoryError> {
        let mut query =
            TasksBacklogEntity::find().filter(tasks_backlog::Column::Status.eq(status.to_string()));

        if let Some(team_id) = team_id {
            query = query.filter(tasks_backlog::Column::TeamId.eq(team_id));
        }

        let count = query.count(self.db.as_ref()).await?;
        Ok(count as i64)
    }

    async fn update_status_batch(
        &self,
        ids: &[Uuid],
        status: TasksBacklogStatus,
    ) -> Result<u64, RepositoryError> {
        let result = TasksBacklogEntity::update_many()
            .col_expr(
                tasks_backlog::Column::Status,
                Expr::value(status.to_string()),
            )
            .col_expr(tasks_backlog::Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(tasks_backlog::Column::Id.is_in(ids.to_vec()))
            .exec(self.db.as_ref())
            .await?;

        Ok(result.rows_affected)
    }
}
