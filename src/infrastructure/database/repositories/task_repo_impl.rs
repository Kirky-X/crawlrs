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
use crate::infrastructure::database::entities::crawl as crawl_entity;
use crate::infrastructure::database::entities::task as task_entity;
use crate::infrastructure::database::repositories::terminal_status;
use crate::infrastructure::persistence::mappers::TaskMapper;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use dbnexus::sea_orm::{
    sea_query::Expr, ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseBackend,
    DbErr, EntityTrait, ExprTrait, FromQueryResult, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Statement, TransactionTrait,
};
use dbnexus::DbPool;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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
            DbError::RateLimited { retry_after_secs } => {
                RepositoryError::Database(anyhow::anyhow!(DbErr::Custom(format!(
                    "Rate limited (retry_after={retry_after_secs:?})"
                ))))
            }
            DbError::Unsupported(msg) => RepositoryError::Database(anyhow::anyhow!(DbErr::Custom(
                format!("Unsupported: {}", msg)
            ))),
            DbError::VersionConflict { table, id } => RepositoryError::Database(anyhow::anyhow!(
                DbErr::Custom(format!("Version conflict on table '{table}' for id {id}"))
            )),
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

/// Step 1：最高优先级 `queued` 任务的原子领取（`UPDATE ... RETURNING *`）。
async fn acquire_queued_task(
    conn: &dbnexus::sea_orm::DatabaseConnection,
    worker_id: crate::domain::id::Id,
    lock_seconds: i64,
) -> Result<Option<Task>, RepositoryError> {
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

    match row {
        Some(row) => Ok(Some(task_from_row(row)?)),
        None => Ok(None),
    }
}

/// Step 2：过期锁 `active` 任务的恢复路径 + 毒丸 dead-letter 兜底。
///
/// 毒丸熔断：恢复即 attempt_count + 1，且仅恢复
/// attempt_count < max_retries 的任务——每次僵尸恢复都消耗一次尝试额度，
/// 防止反复崩溃的任务被无限重跑。
async fn acquire_stale_task(
    conn: &dbnexus::sea_orm::DatabaseConnection,
    worker_id: crate::domain::id::Id,
    lock_seconds: i64,
) -> Result<Option<Task>, RepositoryError> {
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
        Some(row) => Ok(Some(task_from_row(row)?)),
        // 无可恢复任务：检查是否存在超出 max_retries 的毒丸任务并 dead-letter，
        // 避免其永久滞留 active 状态
        None => {
            dead_letter_poison_task(conn).await?;
            Ok(None)
        }
    }
}

/// 超出 max_retries 的毒丸任务 dead-letter（无行则静默）。
async fn dead_letter_poison_task(
    conn: &dbnexus::sea_orm::DatabaseConnection,
) -> Result<(), RepositoryError> {
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
        if let Ok(dead_id) = dead_row.try_get_by_index::<uuid::Uuid>(0) {
            log::warn!(
                "Dead-lettered poison task {} (attempt_count >= max_retries, \
                 lock expired; will not be retried again)",
                dead_id
            );
        }
    }
    Ok(())
}

/// `RETURNING *` 行 → 领域 `Task`。
fn task_from_row(row: dbnexus::sea_orm::QueryResult) -> Result<Task, RepositoryError> {
    let entity = task_entity::Model::from_query_result(&row, "")
        .map_err(|e| RepositoryError::Database(e.into()))?;
    Ok(TaskMapper::to_domain(entity))
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
            .map_err(|e| {
                let msg = e.to_string().to_lowercase();
                if msg.contains("duplicate key")
                    || msg.contains("23505")
                    || msg.contains("unique constraint")
                    || msg.contains("duplicate entry")
                {
                    RepositoryError::Conflict {
                        detail: "tasks unique constraint conflict".to_string(),
                    }
                } else {
                    RepositoryError::Database(e.into())
                }
            })?;

        Ok(task.clone())
    }

    async fn find_by_id(&self, id: crate::domain::id::Id) -> Result<Option<Task>, RepositoryError> {
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
    async fn acquire_next(
        &self,
        worker_id: crate::domain::id::Id,
    ) -> Result<Option<Task>, RepositoryError> {
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
        if let Some(task) = acquire_queued_task(conn, worker_id, lock_seconds).await? {
            return Ok(Some(task));
        }

        // Step 2 — Recovery path: expired-lock active task.
        // Uses partial index idx_tasks_acquire_stale for Index Scan with LIMIT 1.
        // Reaches here only when step 1 returned no row, so queued tasks always
        // take priority over recovery (no starvation).
        acquire_stale_task(conn, worker_id, lock_seconds).await
    }

    async fn mark_completed(
        &self,
        id: crate::domain::id::Id,
        lock_token: Option<crate::domain::id::Id>,
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
            .col_expr(
                task_entity::Column::LockToken,
                Expr::value(None::<uuid::Uuid>),
            )
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
        id: crate::domain::id::Id,
        lock_token: Option<crate::domain::id::Id>,
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
            .col_expr(
                task_entity::Column::LockToken,
                Expr::value(None::<uuid::Uuid>),
            )
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
        id: crate::domain::id::Id,
        lock_token: Option<crate::domain::id::Id>,
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
            .col_expr(
                task_entity::Column::LockToken,
                Expr::value(None::<uuid::Uuid>),
            )
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

        // 终态统一清锁：守卫过滤仍使用快照中的锁身份（必须与 DB 当前值
        // 一致才允许写入），写回时 Completed/Failed/Cancelled 一律置 NULL，
        // 与 mark_failed/mark_completed 的终态行形状一致（审查 F10）。
        let is_terminal = matches!(
            task.status,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        );
        let lock_token_to_write = if is_terminal {
            None
        } else {
            task.lock_token.map(uuid::Uuid::from)
        };
        let lock_expires_to_write = if is_terminal {
            None
        } else {
            task.lock_expires_at
        };

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
            .col_expr(
                task_entity::Column::LockToken,
                Expr::value(lock_token_to_write),
            )
            .col_expr(
                task_entity::Column::LockExpiresAt,
                Expr::value(lock_expires_to_write),
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

    async fn mark_cancelled(&self, id: crate::domain::id::Id) -> Result<u64, RepositoryError> {
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
            .col_expr(
                task_entity::Column::LockToken,
                Expr::value(None::<uuid::Uuid>),
            )
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
        task_id: crate::domain::id::Id,
        worker_id: crate::domain::id::Id,
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

    async fn list_distinct_urls(
        &self,
        limit: u32,
        after_url: Option<String>,
    ) -> Result<Vec<String>, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let conn = session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;

        // keyset 分页（url > after_url）：每页 O(limit)，配合 url 升序稳定
        // 翻页。用于去重状态启动预热（预热幂等，页间轻微漂移无害）。
        let mut query = task_entity::Entity::find()
            .select_only()
            .column(task_entity::Column::Url)
            .distinct()
            .order_by_asc(task_entity::Column::Url)
            .limit(u64::from(limit));
        if let Some(after) = after_url {
            query = query.filter(task_entity::Column::Url.gt(after));
        }
        let urls = query
            .into_tuple::<String>()
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(urls)
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
            .col_expr(
                task_entity::Column::LockToken,
                Expr::value(None::<uuid::Uuid>),
            )
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
        worker_ids: &[crate::domain::id::Id],
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
            .col_expr(
                task_entity::Column::LockToken,
                Expr::value(None::<uuid::Uuid>),
            )
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

    async fn cancel_tasks_by_crawl_id(
        &self,
        crawl_id: crate::domain::id::Id,
    ) -> Result<u64, RepositoryError> {
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
            .col_expr(task_entity::Column::CompletedAt, Expr::value(Utc::now()))
            .col_expr(
                task_entity::Column::LockToken,
                Expr::value(None::<uuid::Uuid>),
            )
            .col_expr(
                task_entity::Column::LockExpiresAt,
                Expr::value(None::<chrono::DateTime<Utc>>),
            )
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

        // 先取候选 crawl，再按 crawl 逐组事务处理：
        // 每个 crawl 的“实际更新行数 → 计数增量 → 终态判定”在同一事务内，
        // 且增量来自 `rows_affected`（不是预统计），并发认领/双 sweep 不会多计。
        let affected_crawl_ids: Vec<Option<uuid::Uuid>> = task_entity::Entity::find()
            .select_only()
            .column(task_entity::Column::CrawlId)
            .filter(stale_condition.clone())
            .filter(lock_lease_free.clone())
            .distinct()
            .into_tuple()
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let mut total_affected = 0u64;
        let mut first_error: Option<String> = None;
        for crawl_id in affected_crawl_ids.into_iter().flatten() {
            let group_result: Result<u64, RepositoryError> = async {
                let txn = conn
                    .begin()
                    .await
                    .map_err(|e| RepositoryError::Database(e.into()))?;
                let step: Result<u64, RepositoryError> = async {
                    let updated = task_entity::Entity::update_many()
                        .col_expr(
                            task_entity::Column::Status,
                            Expr::value(TaskStatus::Failed.to_string()),
                        )
                        .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
                        .col_expr(task_entity::Column::CompletedAt, Expr::value(Utc::now()))
                        .col_expr(
                            task_entity::Column::LockToken,
                            Expr::value(None::<uuid::Uuid>),
                        )
                        .col_expr(
                            task_entity::Column::LockExpiresAt,
                            Expr::value(None::<chrono::DateTime<Utc>>),
                        )
                        .filter(stale_condition.clone())
                        .filter(lock_lease_free.clone())
                        .filter(task_entity::Column::CrawlId.eq(crawl_id))
                        .exec(&txn)
                        .await
                        .map_err(|e| RepositoryError::Database(e.into()))?
                        .rows_affected;
                    if updated > 0 {
                        reconcile_crawl_after_expiry(&txn, crawl_id, updated as i64).await?;
                    }
                    Ok(updated)
                }
                .await;
                match step {
                    Ok(updated) => txn
                        .commit()
                        .await
                        .map(|()| updated)
                        .map_err(|e| RepositoryError::Database(e.into())),
                    Err(e) => {
                        // 显式回滚：SeaORM 的 Drop 回滚失败会 panic（可能终止 sweep 循环）。
                        if let Err(rollback_err) = txn.rollback().await {
                            log::error!(
                                "Rollback failed for crawl {} expire transaction: {}",
                                crawl_id,
                                rollback_err
                            );
                        }
                        Err(e)
                    }
                }
            }
            .await;
            match group_result {
                Ok(n) => total_affected += n,
                Err(e) => {
                    log::error!(
                        "Failed to expire tasks for crawl {} (transaction rolled back): {}",
                        crawl_id,
                        e
                    );
                    if first_error.is_none() {
                        first_error = Some(e.to_string());
                    }
                }
            }
        }

        // 无 crawl 归属的普通任务（crawl_id IS NULL）没有对账对象，
        // 仍要在同一批次内标记失败（否则普通过期任务永不清理）。
        let orphan_updated = task_entity::Entity::update_many()
            .col_expr(
                task_entity::Column::Status,
                Expr::value(TaskStatus::Failed.to_string()),
            )
            .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
            .col_expr(task_entity::Column::CompletedAt, Expr::value(Utc::now()))
            .col_expr(
                task_entity::Column::LockToken,
                Expr::value(None::<uuid::Uuid>),
            )
            .col_expr(
                task_entity::Column::LockExpiresAt,
                Expr::value(None::<chrono::DateTime<Utc>>),
            )
            .filter(stale_condition.clone())
            .filter(lock_lease_free.clone())
            .filter(task_entity::Column::CrawlId.is_null())
            .exec(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
            .rows_affected;
        total_affected += orphan_updated;

        // 周期对账（level-triggered）：任何已满足终态谓词但仍是非终态的
        // crawl（此前 guarded 写遇瞬态错误而漏掉）在本轮 sweep 收口。
        let finalized = crawl_entity::Entity::update_many()
            .col_expr(
                crawl_entity::Column::Status,
                terminal_status::terminal_status_case(),
            )
            .col_expr(
                crawl_entity::Column::CompletedAt,
                Expr::value(Some(Utc::now().naive_utc())),
            )
            .col_expr(
                crawl_entity::Column::UpdatedAt,
                Expr::value(Utc::now().naive_utc()),
            )
            .filter(terminal_status::terminal_eligible_condition())
            .exec(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
            .rows_affected;
        if finalized > 0 {
            log::info!(
                "expire_tasks finalized {} terminal-eligible crawl(s) during periodic reconciliation",
                finalized
            );
        }

        // 崩溃恢复对账：worker 可能在“任务行已 Completed、crawl 计数未自增”
        // 之间被硬终止，completed+failed 永久差 1，终态谓词永不成立。
        // 对静默超过 10 分钟、已无 queued/active 任务的非终态 crawl，
        // 用 tasks 实际计数重写 total/completed/failed 并收口。
        let cutoff = Utc::now() - Duration::minutes(10);
        let candidates = crawl_entity::Entity::find()
            .filter(terminal_status::non_terminal_condition())
            .filter(crawl_entity::Column::TotalTasks.gt(0))
            .filter(
                Expr::col(crawl_entity::Column::CompletedTasks)
                    .add(Expr::col(crawl_entity::Column::FailedTasks))
                    .lt(Expr::col(crawl_entity::Column::TotalTasks)),
            )
            .filter(crawl_entity::Column::UpdatedAt.lt(cutoff))
            .limit(100)
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let mut recovered = 0u64;
        for crawl in candidates {
            // 有排队/在跑任务，或最近（cutoff 内）有任务变更 → 不是静默
            // 僵尸：跳过，避免在“任务刚完成、子链接尚未入队”的窗口内
            // 提前终结活跃 crawl。
            let busy = task_entity::Entity::find()
                .filter(task_entity::Column::CrawlId.eq(crawl.id))
                .filter(
                    Condition::any()
                        .add(task_entity::Column::Status.is_in([
                            TaskStatus::Queued.to_string(),
                            TaskStatus::Active.to_string(),
                        ]))
                        .add(task_entity::Column::UpdatedAt.gte(cutoff)),
                )
                .count(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
            if busy > 0 {
                continue;
            }
            let completed = task_entity::Entity::find()
                .filter(task_entity::Column::CrawlId.eq(crawl.id))
                .filter(task_entity::Column::Status.eq(TaskStatus::Completed.to_string()))
                .count(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?
                as i32;
            let failed = task_entity::Entity::find()
                .filter(task_entity::Column::CrawlId.eq(crawl.id))
                .filter(task_entity::Column::Status.eq(TaskStatus::Failed.to_string()))
                .count(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))? as i32;
            let old_total = crawl.total_tasks;
            let (total, completed, failed, status) = if completed + failed == 0 {
                // crawl 声明了 total 但一条任务行都没有（首任务插入即崩溃）：
                // 按“全部未创建任务失败”收口，避免永久 queued。
                (
                    old_total,
                    0,
                    old_total,
                    crate::domain::models::CrawlStatus::Failed,
                )
            } else {
                let total = completed + failed;
                // 任务行少于声明 total：计划中的任务从未创建（入队中断），
                // 不能当作“自动缩小范围后成功”，按失败收口。
                let status = if completed == 0 || total < old_total {
                    crate::domain::models::CrawlStatus::Failed
                } else {
                    crate::domain::models::CrawlStatus::Completed
                };
                (total, completed, failed, status)
            };
            let now_dt = Utc::now().naive_utc();
            recovered += crawl_entity::Entity::update_many()
                .col_expr(
                    crawl_entity::Column::Status,
                    Expr::value(status.to_string()),
                )
                .col_expr(crawl_entity::Column::TotalTasks, Expr::value(total))
                .col_expr(crawl_entity::Column::CompletedTasks, Expr::value(completed))
                .col_expr(crawl_entity::Column::FailedTasks, Expr::value(failed))
                .col_expr(crawl_entity::Column::CompletedAt, Expr::value(Some(now_dt)))
                .col_expr(crawl_entity::Column::UpdatedAt, Expr::value(now_dt))
                .filter(crawl_entity::Column::Id.eq(crawl.id))
                .filter(terminal_status::non_terminal_condition())
                .exec(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?
                .rows_affected;
        }
        if recovered > 0 {
            log::info!(
                "expire_tasks recovered {} stuck crawl(s) by recomputing counters from tasks",
                recovered
            );
        }

        if let Some(e) = first_error {
            return Err(RepositoryError::Database(anyhow::anyhow!(
                "expire_tasks partially failed: {e}"
            )));
        }

        Ok(total_affected)
    }

    async fn find_by_crawl_id(
        &self,
        crawl_id: crate::domain::id::Id,
    ) -> Result<Vec<Task>, RepositoryError> {
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
        task_ids: Vec<crate::domain::id::Id>,
        team_id: crate::domain::id::Id,
        _force: bool,
    ) -> Result<
        (
            Vec<crate::domain::id::Id>,
            Vec<(crate::domain::id::Id, String)>,
        ),
        RepositoryError,
    > {
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
        let entities = load_cancel_candidates(conn, &task_ids).await?;

        let mut cancelled = Vec::new();
        let mut errors = Vec::new();

        // 按团队所有权分组
        let (owned_ids, not_found_errors) = classify_task_ownership(&task_ids, &team_id, &entities);
        errors.extend(not_found_errors);

        // 批量更新所有归属当前团队的任务
        if !owned_ids.is_empty() {
            cancel_owned_tasks(conn, &owned_ids).await?;
            // 所有更新的任务都成功取消
            cancelled.extend(owned_ids);
        }

        Ok((cancelled, errors))
    }
}

/// 批量载入待取消任务实体（一次查询，避免 N+1）
async fn load_cancel_candidates(
    conn: &dbnexus::sea_orm::DatabaseConnection,
    task_ids: &[crate::domain::id::Id],
) -> Result<Vec<task_entity::Model>, RepositoryError> {
    task_entity::Entity::find()
        .filter(task_entity::Column::Id.is_in(task_ids.iter().copied()))
        .all(conn)
        .await
        .map_err(|e| RepositoryError::Database(e.into()))
}

/// 按团队所有权分组任务 ID：归属当前团队 → 待取消（owned），他队 → 错误，
/// 不存在 → 错误。预建索引把 O(task_ids × entities) 的线性扫描降为 O(1) 查找。
fn classify_task_ownership(
    task_ids: &[crate::domain::id::Id],
    team_id: &crate::domain::id::Id,
    entities: &[task_entity::Model],
) -> (
    Vec<crate::domain::id::Id>,
    Vec<(crate::domain::id::Id, String)>,
) {
    let mut owned_ids: Vec<crate::domain::id::Id> = Vec::new();
    let mut errors = Vec::new();
    let mut not_found_ids = Vec::new();

    // 预建索引：把 O(task_ids × entities) 的线性扫描降为 O(1) 查找
    let entities_by_id: HashMap<uuid::Uuid, &task_entity::Model> =
        entities.iter().map(|e| (e.id, e)).collect();

    for id in task_ids {
        if let Some(entity) = entities_by_id.get(&id.to_uuid_v8()) {
            if entity.team_id == team_id.to_uuid_v8() {
                owned_ids.push(entity.id.into());
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

    (owned_ids, errors)
}

/// 批量取消归属当前团队的任务（状态置 Cancelled、刷新 UpdatedAt）
async fn cancel_owned_tasks(
    conn: &dbnexus::sea_orm::DatabaseConnection,
    owned_ids: &[crate::domain::id::Id],
) -> Result<(), RepositoryError> {
    let update_count = task_entity::Entity::update_many()
        .col_expr(
            task_entity::Column::Status,
            Expr::value(TaskStatus::Cancelled.to_string()),
        )
        .col_expr(task_entity::Column::UpdatedAt, Expr::value(Utc::now()))
        .col_expr(task_entity::Column::CompletedAt, Expr::value(Utc::now()))
        .col_expr(
            task_entity::Column::LockToken,
            Expr::value(None::<uuid::Uuid>),
        )
        .col_expr(
            task_entity::Column::LockExpiresAt,
            Expr::value(None::<chrono::DateTime<Utc>>),
        )
        .filter(task_entity::Column::Id.is_in(owned_ids.iter().copied()))
        // 只取消活跃行，避免把已 completed/failed 覆盖为 cancelled
        .filter(task_entity::Column::Status.is_in([
            TaskStatus::Queued.to_string(),
            TaskStatus::Active.to_string(),
        ]))
        .exec(conn)
        .await
        .map_err(|e| RepositoryError::Database(e.into()))?;

    let _ = update_count;
    Ok(())
}

/// 过期批量清理后的 crawl 对账：对失败计数做**相对增量**，
/// 再用单条条件 UPDATE 原子判定终态。
///
/// 不读 crawl 计数快照后绝对回写：并发的完成/入队走原子自增，
/// 读-改-写窗口会覆盖这些增量（lost update），让 crawl 永久停在非终态。
async fn reconcile_crawl_after_expiry<C: ConnectionTrait>(
    conn: &C,
    crawl_id: uuid::Uuid,
    expired_count: i64,
) -> Result<(), RepositoryError> {
    if expired_count <= 0 {
        return Ok(());
    }
    let now = Utc::now().naive_utc();

    // 1) failed_tasks = failed_tasks + n（与正常失败路径同一增量语义）；
    //    已终态的 crawl 不再改计数（取消/完成不被过期任务污染）。
    crawl_entity::Entity::update_many()
        .col_expr(
            crawl_entity::Column::FailedTasks,
            Expr::col(crawl_entity::Column::FailedTasks).add(expired_count as i32),
        )
        .col_expr(crawl_entity::Column::UpdatedAt, Expr::value(now))
        .filter(crawl_entity::Column::Id.eq(crawl_id))
        .filter(terminal_status::non_terminal_condition())
        .exec(conn)
        .await
        .map_err(|e| RepositoryError::Database(e.into()))?;

    // 2) 终态判定放进 UPDATE 的 WHERE：谓词与行锁在同一语句内求值，
    //    与并发的 completed/failed 自增之间不存在读后写窗口。
    //    口径与 update_crawl_completion_status 一致：total>0 且
    //    completed+failed==total；completed==0 → Failed（全失败不报成功）。
    crawl_entity::Entity::update_many()
        .col_expr(
            crawl_entity::Column::Status,
            terminal_status::terminal_status_case(),
        )
        .col_expr(crawl_entity::Column::CompletedAt, Expr::value(Some(now)))
        .col_expr(crawl_entity::Column::UpdatedAt, Expr::value(now))
        .filter(crawl_entity::Column::Id.eq(crawl_id))
        .filter(terminal_status::terminal_eligible_condition())
        .exec(conn)
        .await
        .map_err(|e| RepositoryError::Database(e.into()))?;
    Ok(())
}

#[cfg(test)]
#[path = "tests/task_repo_test.rs"]
mod tests;
