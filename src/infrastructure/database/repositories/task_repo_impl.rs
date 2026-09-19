// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

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
use dbnexus::sea_orm::{
    sea_query::Expr, ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseBackend,
    DbErr, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    Statement,
};
use dbnexus::DbPool;
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

impl From<DbErr> for RepositoryError {
    fn from(err: DbErr) -> Self {
        RepositoryError::Database(anyhow::anyhow!(err))
    }
}

impl From<dbnexus::DbError> for RepositoryError {
    fn from(err: dbnexus::DbError) -> Self {
        use dbnexus::DbError;
        match err {
            DbError::Connection(db_err) => RepositoryError::Database(anyhow::anyhow!(db_err)),
            DbError::Config(msg) => RepositoryError::Database(anyhow::anyhow!(DbErr::Custom(
                format!("Config: {}", msg)
            ))),
            DbError::Permission(msg) => RepositoryError::Database(anyhow::anyhow!(DbErr::Custom(
                format!("Permission: {}", msg)
            ))),
            DbError::Transaction(msg) => RepositoryError::Database(anyhow::anyhow!(DbErr::Custom(
                format!("Transaction: {}", msg)
            ))),
            DbError::Migration(msg) => RepositoryError::Database(anyhow::anyhow!(DbErr::Custom(
                format!("Migration: {}", msg)
            ))),
            DbError::Cache(msg) | DbError::Query(msg) => {
                RepositoryError::Database(anyhow::anyhow!(DbErr::Custom(msg)))
            }
        }
    }
}

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

    /// Acquire the next available task for a worker.
    ///
    /// Raw SQL is required because sea-orm doesn't support `FOR UPDATE SKIP LOCKED`.
    /// This is the only method in the trait that bypasses sea-orm ActiveModel —
    /// all other methods use the standard `Entity::find`/`ActiveModel::update` API.
    ///
    /// # Architecture
    ///
    /// Two-step query to keep each step as an Index Scan with LIMIT 1:
    ///   1. Normal path: fetch the highest-priority `queued` task.
    ///   2. Recovery path: if no queued task, fetch an `active` task whose
    ///      `lock_expires_at` has passed (previous worker crashed or lock
    ///      timed out). Without this, such tasks would be stuck forever.
    ///
    /// Each step is an atomic `UPDATE ... WHERE id = (SELECT ... FOR UPDATE
    /// SKIP LOCKED LIMIT 1) RETURNING *`, eliminating the race condition in
    /// the original non-atomic `SELECT + UPDATE` implementation (production
    /// observed 1 task picked up by 18 workers).
    ///
    /// # Mirrored Domain Logic
    ///
    /// The `SET` clause mirrors `Task::start()` + `Task::acquire_lock()` in
    /// `task_model.rs`. Changes to either side must be synchronized — see
    /// `test_acquire_next_set_clause_mirrors_domain_methods` for the
    /// regression guard.
    async fn acquire_next(&self, worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let lock_seconds = self.lock_duration.num_seconds();

        // Step 1 — Normal path: highest-priority queued task.
        // Uses partial index idx_tasks_acquire_queued for Index Scan with LIMIT 1.
        let stmt_queued = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE tasks
               SET status = 'active',
                   started_at = NOW(),
                   lock_token = $1,
                   lock_expires_at = NOW() + ($2 * INTERVAL '1 second'),
                   updated_at = NOW()
               WHERE id = (
                   SELECT id FROM tasks
                   WHERE status = 'queued'
                     AND (scheduled_at IS NULL OR scheduled_at <= NOW())
                   ORDER BY priority ASC, created_at ASC
                   FOR UPDATE SKIP LOCKED
                   LIMIT 1
               )
               RETURNING *"#,
            [worker_id.into(), lock_seconds.into()],
        );

        let row: Option<dbnexus::sea_orm::QueryResult> = conn
            .query_one_raw(stmt_queued)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if let Some(row) = row {
            let entity = task_entity::Model::from_query_result(&row, "")
                .map_err(|e| RepositoryError::Database(e.into()))?;
            return Ok(Some(TaskMapper::to_domain(entity)));
        }

        // Step 2 — Recovery path: expired-lock active task.
        // Uses partial index idx_tasks_acquire_stale for Index Scan with LIMIT 1.
        // Reaches here only when step 1 returned no row, so queued tasks always
        // take priority over recovery (no starvation).
        //
        // 毒丸熔断：恢复即 attempt_count + 1，且仅恢复
        // attempt_count < max_retries 的任务——每次僵尸恢复都消耗一次尝试额度，
        // 防止反复崩溃的任务被无限重跑。
        let stmt_stale = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE tasks
               SET status = 'active',
                   started_at = NOW(),
                   lock_token = $1,
                   lock_expires_at = NOW() + ($2 * INTERVAL '1 second'),
                   updated_at = NOW(),
                   attempt_count = attempt_count + 1
               WHERE id = (
                   SELECT id FROM tasks
                   WHERE status = 'active' AND lock_expires_at < NOW()
                     AND (scheduled_at IS NULL OR scheduled_at <= NOW())
                     AND attempt_count < max_retries
                   ORDER BY priority ASC, created_at ASC
                   FOR UPDATE SKIP LOCKED
                   LIMIT 1
               )
               RETURNING *"#,
            [worker_id.into(), lock_seconds.into()],
        );

        let row: Option<dbnexus::sea_orm::QueryResult> = conn
            .query_one_raw(stmt_stale)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        match row {
            Some(row) => {
                let entity = task_entity::Model::from_query_result(&row, "")
                    .map_err(|e| RepositoryError::Database(e.into()))?;
                return Ok(Some(TaskMapper::to_domain(entity)));
            }
            // 无可恢复任务：检查是否存在超出 max_retries 的毒丸任务并 dead-letter，
            // 避免其永久滞留 active 状态
            None => {
                let stmt_dead_letter = Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"UPDATE tasks
                       SET status = 'failed',
                           completed_at = NOW(),
                           lock_token = NULL,
                           lock_expires_at = NULL,
                           updated_at = NOW()
                       WHERE id = (
                           SELECT id FROM tasks
                           WHERE status = 'active' AND lock_expires_at < NOW()
                             AND attempt_count >= max_retries
                           ORDER BY priority ASC, created_at ASC
                           FOR UPDATE SKIP LOCKED
                           LIMIT 1
                       )
                       RETURNING id"#,
                    [],
                );
                let dead: Option<dbnexus::sea_orm::QueryResult> = conn
                    .query_one_raw(stmt_dead_letter)
                    .await
                    .map_err(|e| RepositoryError::Database(e.into()))?;
                if let Some(dead_row) = dead {
                    if let Ok(dead_id) = dead_row.try_get_by_index::<Uuid>(0) {
                        log::warn!(
                            "Dead-lettered poison task {} (attempt_count >= max_retries, \
                             lock expired; will not be retried again)",
                            dead_id
                        );
                    }
                }
                Ok(None)
            }
        }
    }

    async fn mark_completed(
        &self,
        id: Uuid,
        lock_token: Option<Uuid>,
    ) -> Result<u64, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let result = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Completed.to_string()),
            )
            .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
            .col_expr(task_entity::Column::CompletedAt, Expr::value(Utc::now()))
            .col_expr(task_entity::Column::LockToken, Expr::value(None::<Uuid>))
            .col_expr(
                task_entity::Column::LockExpiresAt,
                Expr::value(None::<chrono::DateTime<Utc>>),
            )
            .filter(task_entity::Column::Id.eq(id))
            .filter(task_entity::Column::Status.is_in([
                TaskStatus::Queued.to_string(),
                TaskStatus::Active.to_string(),
            ]))
            // 锁守卫：无锁任务（queued）或锁仍归属调用者的任务才可终结
            .filter(match lock_token {
                Some(w) => task_entity::Column::LockToken.eq(w),
                None => task_entity::Column::LockToken.is_null(),
            })
            .exec(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(result.rows_affected)
    }

    async fn mark_failed(
        &self,
        id: Uuid,
        lock_token: Option<Uuid>,
    ) -> Result<u64, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let result = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Failed.to_string()),
            )
            .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
            .col_expr(task_entity::Column::CompletedAt, Expr::value(Utc::now()))
            .col_expr(task_entity::Column::LockToken, Expr::value(None::<Uuid>))
            .col_expr(
                task_entity::Column::LockExpiresAt,
                Expr::value(None::<chrono::DateTime<Utc>>),
            )
            .filter(task_entity::Column::Id.eq(id))
            .filter(task_entity::Column::Status.is_in([
                TaskStatus::Queued.to_string(),
                TaskStatus::Active.to_string(),
            ]))
            // 锁守卫：无锁任务（queued）或锁仍归属调用者的任务才可终结
            .filter(match lock_token {
                Some(w) => task_entity::Column::LockToken.eq(w),
                None => task_entity::Column::LockToken.is_null(),
            })
            .exec(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(result.rows_affected)
    }

    /// 条件回队
    ///
    /// 仅当任务仍处于 queued/active 且锁仍归属调用者（或无锁）时置回 queued，
    /// 清空认领信息并应用重排时间；防止覆盖并发发生的取消/终结等状态迁移。
    async fn requeue_task(
        &self,
        id: Uuid,
        lock_token: Option<Uuid>,
        scheduled_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<bool, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let result = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Queued.to_string()),
            )
            .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
            .col_expr(task_entity::Column::LockToken, Expr::value(None::<Uuid>))
            .col_expr(
                task_entity::Column::LockExpiresAt,
                Expr::value(None::<chrono::DateTime<Utc>>),
            )
            .col_expr(
                task_entity::Column::StartedAt,
                Expr::value(None::<chrono::DateTime<Utc>>),
            )
            .col_expr(task_entity::Column::ScheduledAt, Expr::value(scheduled_at))
            .filter(task_entity::Column::Id.eq(id))
            .filter(task_entity::Column::Status.is_in([
                TaskStatus::Queued.to_string(),
                TaskStatus::Active.to_string(),
            ]))
            // 锁守卫：无锁任务（queued）或锁仍归属调用者的任务才可回队
            .filter(match lock_token {
                Some(w) => task_entity::Column::LockToken.eq(w),
                None => task_entity::Column::LockToken.is_null(),
            })
            .exec(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if result.rows_affected == 0 {
            log::warn!(
                "requeue_task: task {} not requeued (already migrated by another worker)",
                id
            );
        }
        Ok(result.rows_affected > 0)
    }

    /// 守卫式生命周期更新
    ///
    /// 按内存快照写入生命周期字段，但仅当 DB 行仍为 queued/active 且
    /// lock_token 与快照一致时生效；防止覆盖并发发生的取消/终结。
    async fn update_task_guarded(
        &self,
        task: &crate::domain::models::task_model::Task,
    ) -> Result<u64, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let result = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(task.status.to_string()),
            )
            .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
            .col_expr(
                task_entity::Column::ScheduledAt,
                Expr::value(task.scheduled_at),
            )
            .col_expr(task_entity::Column::StartedAt, Expr::value(task.started_at))
            .col_expr(
                task_entity::Column::CompletedAt,
                Expr::value(task.completed_at),
            )
            .col_expr(
                task_entity::Column::AttemptCount,
                Expr::value(task.attempt_count),
            )
            .col_expr(
                task_entity::Column::RetryCount,
                Expr::value(task.retry_count),
            )
            .col_expr(
                task_entity::Column::Payload,
                Expr::value(task.payload.clone()),
            )
            .col_expr(task_entity::Column::LockToken, Expr::value(task.lock_token))
            .col_expr(
                task_entity::Column::LockExpiresAt,
                Expr::value(task.lock_expires_at),
            )
            .filter(task_entity::Column::Id.eq(task.id))
            .filter(task_entity::Column::Status.is_in([
                TaskStatus::Queued.to_string(),
                TaskStatus::Active.to_string(),
            ]))
            // 锁守卫：调用者快照中的锁身份必须与 DB 当前值一致
            .filter(match task.lock_token {
                Some(w) => task_entity::Column::LockToken.eq(w),
                None => task_entity::Column::LockToken.is_null(),
            })
            .exec(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if result.rows_affected == 0 {
            log::warn!(
                "update_task_guarded: task {} not updated (guard failed: migrated by another worker)",
                task.id
            );
        }
        Ok(result.rows_affected)
    }

    async fn mark_cancelled(&self, id: Uuid) -> Result<u64, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Cancelled.to_string()),
            )
            .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
            .col_expr(task_entity::Column::CompletedAt, Expr::value(Utc::now()))
            .col_expr(task_entity::Column::LockToken, Expr::value(None::<Uuid>))
            .col_expr(
                task_entity::Column::LockExpiresAt,
                Expr::value(None::<chrono::DateTime<Utc>>),
            )
            .filter(task_entity::Column::Id.eq(id))
            .filter(task_entity::Column::Status.is_in([
                TaskStatus::Queued.to_string(),
                TaskStatus::Active.to_string(),
            ]))
            .exec(conn)
            .await
            .map(|res| res.rows_affected)
            .map_err(|e| RepositoryError::Database(e.into()))
    }

    async fn renew_lock(
        &self,
        task_id: Uuid,
        worker_id: Uuid,
        extend_seconds: i64,
    ) -> Result<bool, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        // 锁续期心跳：仅 active 且锁仍归属该 worker 的任务可续期
        let new_expiry = Utc::now() + chrono::Duration::seconds(extend_seconds);
        let result = task_entity::Entity::update_many()
            .col_expr(task_entity::Column::LockExpiresAt, Expr::value(new_expiry))
            .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(task_entity::Column::Id.eq(task_id))
            .filter(task_entity::Column::LockToken.eq(worker_id))
            .filter(task_entity::Column::Status.eq(TaskStatus::Active.to_string()))
            .exec(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(result.rows_affected > 0)
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

    /// 按本副本 worker 身份回滚
    ///
    /// 在 [`Self::reset_stuck_tasks`] 基础上增加 `lock_token ∈ worker_ids` 过滤，
    /// 停机时只回滚本进程 worker 认领的任务，不触碰其他副本在跑任务。
    async fn reset_stuck_tasks_for_workers(
        &self,
        timeout: Duration,
        worker_ids: &[Uuid],
    ) -> Result<u64, RepositoryError> {
        if worker_ids.is_empty() {
            return Ok(0);
        }
        let cutoff = Utc::now() - timeout;

        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

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
            // 身份过滤：仅本副本 worker 认领的任务（lock_token 即认领时的 worker_id）
            .filter(task_entity::Column::LockToken.is_in(worker_ids.to_vec()))
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

        let update_result = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Cancelled.to_string()),
            )
            .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(task_entity::Column::CrawlId.eq(crawl_id))
            .filter(task_entity::Column::Status.is_in(vec![
                TaskStatus::Queued.to_string(),
                TaskStatus::Active.to_string(),
            ]))
            .exec(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(update_result.rows_affected)
    }

    async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
        let now = Utc::now();
        let stale_threshold = now - Duration::hours(24);

        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let stale_condition = Condition::any()
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
            );

        // 持锁守卫：仅过期锁已释放（lock_expires_at IS NULL）
        // 或锁租约已过期（lock_expires_at < now）的任务。仍在心跳续约（持有效锁）的
        // 长任务即使 started_at 早于 stale_threshold 也不会被误杀。
        let lock_lease_free = Condition::any()
            .add(task_entity::Column::LockExpiresAt.is_null())
            .add(task_entity::Column::LockExpiresAt.lt(now));

        let update_result = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Failed.to_string()),
            )
            .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(stale_condition)
            .filter(lock_lease_free)
            .exec(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(update_result.rows_affected)
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

        if let Some(task_ids) = &params.task_ids {
            if task_ids.is_empty() {
                return Ok((Vec::new(), 0));
            }
            // 精确按 task_ids 过滤：sync_wait 轮询与查询 API 依赖此条件
            // 圈定目标任务集合，缺失会导致用"团队最新 N 条"算错完成率
            query = query.filter(task_entity::Column::Id.is_in(task_ids.clone()));
        }

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
#[path = "tests/task_repo_test.rs"]
mod tests;
