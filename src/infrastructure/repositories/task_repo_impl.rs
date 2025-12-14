// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::domain::models::task::{Task, TaskStatus};
use crate::domain::repositories::task_repository::{RepositoryError, TaskRepository};
use crate::infrastructure::database::entities::task as task_entity;
use async_trait::async_trait;
use chrono::{DateTime, Duration, FixedOffset, Utc};
use sea_orm::{
    sea_query::{Expr, LockBehavior, LockType},
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait,
};
use std::sync::Arc;
use uuid::Uuid;

/// 任务仓库实现
///
/// 基于SeaORM实现的任务数据访问层
#[derive(Clone)]
pub struct TaskRepositoryImpl {
    /// 数据库连接
    db: Arc<DatabaseConnection>,
}

impl TaskRepositoryImpl {
    /// 创建新的任务仓库实例
    ///
    /// # 参数
    ///
    /// * `db` - 数据库连接
    ///
    /// # 返回值
    ///
    /// 返回新的任务仓库实例
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }
}

impl From<task_entity::Model> for Task {
    fn from(model: task_entity::Model) -> Self {
        Self {
            id: model.id,
            task_type: model.task_type.parse().unwrap_or_default(),
            status: model.status.parse().unwrap_or_default(),
            priority: model.priority,
            team_id: model.team_id,
            url: model.url,
            payload: model.payload,
            attempt_count: model.attempt_count,
            max_retries: model.max_retries,
            scheduled_at: model.scheduled_at,
            created_at: model.created_at,
            started_at: model.started_at,
            completed_at: model.completed_at,
            crawl_id: model.crawl_id,
            updated_at: model.updated_at,
            lock_token: model.lock_token,
            lock_expires_at: model.lock_expires_at,
        }
    }
}

impl From<Task> for task_entity::ActiveModel {
    fn from(task: Task) -> Self {
        Self {
            id: Set(task.id),
            task_type: Set(task.task_type.to_string()),
            status: Set(task.status.to_string()),
            priority: Set(task.priority),
            team_id: Set(task.team_id),
            url: Set(task.url.clone()),
            payload: Set(task.payload.clone()),
            attempt_count: Set(task.attempt_count),
            max_retries: Set(task.max_retries),
            scheduled_at: Set(task.scheduled_at),
            created_at: Set(task.created_at),
            started_at: Set(task.started_at),
            completed_at: Set(task.completed_at),
            crawl_id: Set(task.crawl_id),
            updated_at: Set(task.updated_at),
            lock_token: Set(task.lock_token),
            lock_expires_at: Set(task.lock_expires_at),
            ..Default::default()
        }
    }
}

#[async_trait]
impl TaskRepository for TaskRepositoryImpl {
    async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
        let model: task_entity::ActiveModel = task.clone().into();

        model.insert(self.db.as_ref()).await?;
        Ok(task.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
        let model = task_entity::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await?;

        Ok(model.map(Into::into))
    }

    async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
        let mut model: task_entity::ActiveModel = task.clone().into();

        model.status = Set(task.status.to_string());
        model.attempt_count = Set(task.attempt_count);
        model.scheduled_at = Set(task.scheduled_at);
        model.started_at = Set(task.started_at);
        model.completed_at = Set(task.completed_at);

        let updated_model = model.update(self.db.as_ref()).await?;
        Ok(updated_model.into())
    }

    async fn acquire_next(&self, worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
        let txn = self.db.begin().await?;

        let task = task_entity::Entity::find()
            .filter(task_entity::Column::Status.eq(TaskStatus::Queued.to_string()))
            .filter(
                Condition::any()
                    .add(task_entity::Column::ScheduledAt.is_null())
                    .add(task_entity::Column::ScheduledAt.lte(Utc::now())),
            )
            .order_by_desc(task_entity::Column::Priority)
            .order_by_asc(task_entity::Column::CreatedAt)
            .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
            .one(&txn)
            .await?;

        if let Some(task) = task {
            let mut active: task_entity::ActiveModel = task.into();
            active.lock_token = Set(Some(worker_id));
            active.lock_expires_at = Set(Some((Utc::now() + Duration::minutes(5)).into()));
            active.status = Set(TaskStatus::Active.to_string());
            active.started_at = Set(Some(Utc::now().into()));
            let current_attempt = *active.attempt_count.as_ref();
            active.attempt_count = Set(current_attempt + 1);

            let updated = active.update(&txn).await?;

            txn.commit().await?;

            return Ok(Some(updated.into()));
        } else {
            txn.commit().await?;
        }

        Ok(None)
    }

    async fn mark_completed(&self, id: Uuid) -> Result<(), RepositoryError> {
        let task = self
            .find_by_id(id)
            .await?
            .ok_or(RepositoryError::NotFound)?;
        let mut updated_task = task.clone();
        updated_task.status = TaskStatus::Completed;
        updated_task.completed_at = Some(Utc::now().into());
        self.update(&updated_task).await?;
        Ok(())
    }

    async fn mark_failed(&self, id: Uuid) -> Result<(), RepositoryError> {
        let task = self
            .find_by_id(id)
            .await?
            .ok_or(RepositoryError::NotFound)?;
        let mut updated_task = task.clone();
        updated_task.status = TaskStatus::Failed;
        updated_task.completed_at = Some(Utc::now().into());
        self.update(&updated_task).await?;
        Ok(())
    }

    async fn mark_cancelled(&self, id: Uuid) -> Result<(), RepositoryError> {
        let task = self
            .find_by_id(id)
            .await?
            .ok_or(RepositoryError::NotFound)?;
        let mut updated_task = task.clone();
        updated_task.status = TaskStatus::Cancelled;
        updated_task.completed_at = Some(Utc::now().into());
        self.update(&updated_task).await?;
        Ok(())
    }

    async fn exists_by_url(&self, url: &str) -> Result<bool, RepositoryError> {
        let count = task_entity::Entity::find()
            .filter(task_entity::Column::Url.eq(url))
            .count(self.db.as_ref())
            .await?;
        Ok(count > 0)
    }

    async fn reset_stuck_tasks(&self, timeout: chrono::Duration) -> Result<u64, RepositoryError> {
        let threshold = Utc::now() - timeout;

        // Find tasks that are Active but lock_expires_at is past or started_at is too old
        // For simplicity, we use lock_expires_at if available, or a timeout based on started_at

        let result = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Queued.to_string()),
            )
            .col_expr(
                task_entity::Column::LockToken,
                Expr::value(Option::<Uuid>::None),
            )
            .col_expr(
                task_entity::Column::LockExpiresAt,
                Expr::value(Option::<DateTime<FixedOffset>>::None),
            )
            .filter(task_entity::Column::Status.eq(TaskStatus::Active.to_string()))
            .filter(
                Condition::any()
                    .add(task_entity::Column::LockExpiresAt.lte(Utc::now()))
                    .add(
                        Condition::all()
                            .add(task_entity::Column::LockExpiresAt.is_null())
                            .add(task_entity::Column::StartedAt.lte(threshold)),
                    ),
            )
            .exec(self.db.as_ref())
            .await?;

        Ok(result.rows_affected)
    }

    async fn cancel_tasks_by_crawl_id(&self, crawl_id: Uuid) -> Result<u64, RepositoryError> {
        // 使用 Payload 中的 crawl_id 字段进行筛选
        // 由于 Payload 是 JSONB 类型，我们需要使用 JSON 操作符
        // 注意：sea-orm 对 JSONB 查询的支持取决于后端数据库
        // 这里假设是 Postgres，使用 Expr::cust 进行原生 SQL 片段构建可能更灵活，
        // 但为了保持 safe，尝试使用 sea-orm 的 filter

        // 假设 payload 结构为 { "crawl_id": "uuid-string", ... }
        // Postgres JSONB 查询: payload->>'crawl_id' = 'uuid-string'

        use sea_orm::sea_query::Expr;

        let result = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Cancelled.to_string()),
            )
            .col_expr(
                task_entity::Column::CompletedAt,
                Expr::value::<Option<DateTime<FixedOffset>>>(Some(Utc::now().into())),
            )
            .filter(
                // 仅取消未完成的任务 (Queued 或 Active)
                task_entity::Column::Status.is_in(vec![
                    TaskStatus::Queued.to_string(),
                    TaskStatus::Active.to_string(),
                ]),
            )
            .filter(
                // 使用 JSON 包含操作符 @>
                // payload @> '{"crawl_id": "crawl_id"}'
                Expr::cust_with_values(
                    "payload->>'crawl_id' = ?",
                    vec![crawl_id.to_string()],
                ),
            )
            .exec(self.db.as_ref())
            .await?;

        Ok(result.rows_affected)
    }

    async fn find_by_crawl_id(&self, crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
        let models = task_entity::Entity::find()
            .filter(
                task_entity::Column::CrawlId.eq(crawl_id)
            )
            .all(self.db.as_ref())
            .await?;

        Ok(models.into_iter().map(Task::from).collect())
    }
}
