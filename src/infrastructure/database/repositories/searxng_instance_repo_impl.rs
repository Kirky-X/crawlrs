// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SearXNG 实例登记簿仓储实现（Sea-ORM / PostgreSQL）

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Utc};
use dbnexus::sea_orm::sea_query::{Expr, ExprTrait, NullOrdering};
use dbnexus::sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, Order,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set, Statement, TransactionTrait, Value,
};
use dbnexus::DbPool;
use std::sync::Arc;

use crate::domain::id::Id;
use crate::domain::repositories::searxng_instance_repository::{
    AdmittedInstance, CollectLockGuard, SearxngInstance, SearxngInstanceRepository,
    SearxngInstanceState, UpsertOutcome,
};
use crate::domain::repositories::task_repository::RepositoryError;
use crate::infrastructure::database::entities::searxng_instance;

/// `last_error` 落库截断长度（避免超长错误串放大存储）
const LAST_ERROR_MAX_CHARS: usize = 512;

/// PG 事务级 advisory lock 守卫：持有一个打开的事务
///
/// 为什么不是会话级锁：dbnexus 的 `Session.connection()` 返回的是 SeaORM
/// `DatabaseConnection`（内部为连接池），同一 Session 的两条语句可能落在不同
/// 物理连接上——`pg_advisory_lock` 与 `pg_advisory_unlock` 命中不同后端会话时
/// 释放无效（旧实现 `pg_advisory_unlock_all()` 同样不可靠）。改用
/// `pg_try_advisory_xact_lock`：锁的生命周期绑定事务，而事务固定占用一条连接；
/// `release()` 通过回滚事务确定性释放。
///
/// 环境前提：采集轮期间事务处于 idle-in-transaction（不执行语句），因此部署侧须
/// 保持 `idle_in_transaction_session_timeout = 0`（PG 默认），且不要经
/// transaction-pooling 代理（如 PgBouncer transaction 模式）——否则后端连接可能
/// 被回收，互斥权在轮次中途静默丢失。
///
/// 计量说明：`begin()` 直接从底层 sqlx 池取连接，绕过 dbnexus 的 Session 计数，
/// 采集轮期间 dbnexus 池指标会比实际少 1 条（连接本身会在 release/Drop 时归还）。
/// 对齐该指标需要 dbnexus 暴露外部连接记账 API（第三方库能力边界）。
struct PgCollectLockGuard {
    /// `Some` = 锁仍持有；`release()` take 后置 None（重复释放幂等）
    tx: parking_lot::Mutex<Option<dbnexus::sea_orm::DatabaseTransaction>>,
}

#[async_trait]
impl CollectLockGuard for PgCollectLockGuard {
    async fn release(&self) -> Result<(), RepositoryError> {
        // 同步取出事务（parking_lot 守卫不跨 await 持有）
        let tx = self.tx.lock().take();
        let Some(tx) = tx else {
            return Ok(());
        };
        // 回滚即释放事务级 advisory lock（幂等：已释放时 take 返回 None）
        tx.rollback()
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        Ok(())
    }
}

/// 实例登记簿仓储实现
#[derive(Clone)]
pub struct SearxngInstanceRepositoryImpl {
    pool: Arc<DbPool>,
}

impl SearxngInstanceRepositoryImpl {
    /// 构造实现
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }

    /// 获取 admin 会话的 Sea-ORM 连接
    async fn admin_conn(&self) -> Result<dbnexus::sea_orm::DatabaseConnection, RepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        session
            .connection()
            .cloned()
            .map_err(|e| RepositoryError::Database(e.into()))
    }

    /// 按 url 读取实体
    async fn find_model(
        &self,
        url: &str,
    ) -> Result<Option<searxng_instance::Model>, RepositoryError> {
        let conn = self.admin_conn().await?;
        searxng_instance::Entity::find()
            .filter(searxng_instance::Column::Url.eq(url))
            .one(&conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))
    }
}

/// 当前时间（落库口径：带时区）
fn now_tz() -> DateTime<FixedOffset> {
    Utc::now().fixed_offset()
}

/// 数据集来源 upsert：单语句插入-或-更新，冲突时保留 `source` 并刷新数据集元数据。
///
/// `xmax = 0` 在 PG 上区分「本次插入」与「冲突更新」；`state` 的 CASE 保留
/// 「退役实例重新出现在数据集 → 回 candidate」的回归语义。
///
/// 注意：SearXNG 登记簿写路径要求 PostgreSQL 驱动（advisory lock 与 `xmax`
/// upsert 均为 PG 语义）；在 sqlite/mysql 构建上启用 `search.searxng` 会在运行期
/// 报 SQL 错误，而非静默降级。
const DATASET_UPSERT_SQL: &str = "\
INSERT INTO searxng_instances (\
    id, url, source, state, network_type, version, api_grade, http_grade, \
    tls_grade, cdn_grade, uptime_day, uptime_month, latency_ms, \
    consecutive_failures, last_success_at, last_failure_at, last_error, \
    dataset_miss_count, created_at, updated_at\
) VALUES (\
    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, \
    $11, $12, $13, $14, $15, $16, $17, $18, $19, $20\
) ON CONFLICT (url) DO UPDATE SET \
    network_type = EXCLUDED.network_type, \
    version = EXCLUDED.version, \
    api_grade = EXCLUDED.api_grade, \
    http_grade = EXCLUDED.http_grade, \
    tls_grade = EXCLUDED.tls_grade, \
    cdn_grade = EXCLUDED.cdn_grade, \
    uptime_day = EXCLUDED.uptime_day, \
    uptime_month = EXCLUDED.uptime_month, \
    dataset_miss_count = 0, \
    updated_at = EXCLUDED.updated_at, \
    state = CASE \
        WHEN searxng_instances.state = 'retired' THEN 'candidate' \
        ELSE searxng_instances.state \
    END \
RETURNING (searxng_instances.xmax = 0) AS inserted";

/// seed 来源 upsert：冲突时只刷来源与时间，其余列保持既有值。
const SEED_INSERT_SQL: &str = "\
INSERT INTO searxng_instances (\
    id, url, source, state, network_type, version, api_grade, http_grade, \
    tls_grade, cdn_grade, uptime_day, uptime_month, latency_ms, \
    consecutive_failures, last_success_at, last_failure_at, last_error, \
    dataset_miss_count, created_at, updated_at\
) VALUES (\
    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, \
    $11, $12, $13, $14, $15, $16, $17, $18, $19, $20\
) ON CONFLICT (url) DO UPDATE SET \
    source = EXCLUDED.source, \
    updated_at = EXCLUDED.updated_at \
RETURNING (searxng_instances.xmax = 0) AS inserted";

/// 数据集 upsert 的参数序列（列顺序与 [`DATASET_UPSERT_SQL`] 一致）
fn dataset_insert_values(instance: &AdmittedInstance, now: DateTime<FixedOffset>) -> Vec<Value> {
    vec![
        Value::from(Id::generate().to_uuid_v8()),
        Value::from(instance.url.clone()),
        Value::from("searx_space".to_string()),
        Value::from(SearxngInstanceState::Candidate.as_str().to_string()),
        Value::from(instance.network_type.clone()),
        Value::from(instance.version.clone()),
        Value::from(instance.api_grade.clone()),
        Value::from(instance.http_grade.clone()),
        Value::from(instance.tls_grade.clone()),
        Value::from(instance.cdn_grade.clone()),
        Value::from(instance.uptime_day),
        Value::from(instance.uptime_month),
        Value::from(None::<i64>),
        Value::from(0i32),
        Value::from(None::<DateTime<FixedOffset>>),
        Value::from(None::<DateTime<FixedOffset>>),
        Value::from(None::<String>),
        Value::from(0i32),
        Value::from(now),
        Value::from(now),
    ]
}

/// seed upsert 的参数序列（列顺序与 [`SEED_INSERT_SQL`] 一致）
fn seed_insert_values(url: &str, now: DateTime<FixedOffset>) -> Vec<Value> {
    vec![
        Value::from(Id::generate().to_uuid_v8()),
        Value::from(url.to_string()),
        Value::from("seed".to_string()),
        Value::from(SearxngInstanceState::Candidate.as_str().to_string()),
        Value::from(None::<String>),
        Value::from(None::<String>),
        Value::from(None::<String>),
        Value::from(None::<String>),
        Value::from(None::<String>),
        Value::from(None::<String>),
        Value::from(None::<f64>),
        Value::from(None::<f64>),
        Value::from(None::<i64>),
        Value::from(0i32),
        Value::from(None::<DateTime<FixedOffset>>),
        Value::from(None::<DateTime<FixedOffset>>),
        Value::from(None::<String>),
        Value::from(0i32),
        Value::from(now),
        Value::from(now),
    ]
}

/// 读取 upsert 的 `inserted` 标志。
///
/// `INSERT ... RETURNING` 必然返回一行；行缺失或列解码失败都是异常（不是「更新」），
/// 必须显性报错，禁止用默认值兜底（失败显性化）。
fn inserted_flag(row: Option<dbnexus::sea_orm::QueryResult>) -> Result<bool, RepositoryError> {
    let row = row.ok_or_else(|| {
        RepositoryError::Database(anyhow::anyhow!("searxng upsert returned no row"))
    })?;
    row.try_get::<bool>("", "inserted")
        .map_err(|e| RepositoryError::Database(e.into()))
}

/// 错误串截断
fn truncate_error(error: Option<&str>) -> Option<String> {
    error.map(|e| e.chars().take(LAST_ERROR_MAX_CHARS).collect())
}

/// 行锁读取实例：串行化「读计数 → 阈值判定 → 状态迁移 → 写回」的读侧。
///
/// 并发两次 miss 各自递增（不再同读同值互相覆盖），Active→Cooldown 迁移不丢失。
async fn find_locked_instance(
    tx: &dbnexus::sea_orm::DatabaseTransaction,
    url: &str,
) -> Result<searxng_instance::Model, RepositoryError> {
    searxng_instance::Entity::find()
        .filter(searxng_instance::Column::Url.eq(url))
        .lock_exclusive()
        .one(tx)
        .await
        .map_err(|e| RepositoryError::Database(e.into()))?
        .ok_or(RepositoryError::NotFound)
}

/// 计算超阈值后的下一状态与退役标志。
///
/// 超阈值 → 退役；`active` 不允许直迁 `retired`（状态机约束），先落 cooldown，
/// 后续轮次由 cooldown→retired 完成退役。
fn next_miss_state(
    current_state: SearxngInstanceState,
    miss_count: i32,
    retire_after: i32,
) -> (SearxngInstanceState, bool) {
    let mut retired = false;
    let mut next_state = current_state;
    if miss_count > retire_after {
        if current_state.can_transition_to(SearxngInstanceState::Retired) {
            next_state = SearxngInstanceState::Retired;
            retired = true;
        } else if current_state == SearxngInstanceState::Active {
            next_state = SearxngInstanceState::Cooldown;
        }
    }
    (next_state, retired)
}

/// 写回 miss 计数与下一状态。
async fn persist_dataset_miss(
    tx: &dbnexus::sea_orm::DatabaseTransaction,
    model: searxng_instance::Model,
    miss_count: i32,
    next_state: SearxngInstanceState,
) -> Result<(), RepositoryError> {
    let mut active: searxng_instance::ActiveModel = model.into();
    active.dataset_miss_count = Set(miss_count);
    active.state = Set(next_state.as_str().to_string());
    active.updated_at = Set(now_tz());
    active
        .update(tx)
        .await
        .map_err(|e| RepositoryError::Database(e.into()))?;
    Ok(())
}

#[async_trait]
impl SearxngInstanceRepository for SearxngInstanceRepositoryImpl {
    async fn try_acquire_collect_lock(
        &self,
        key: i64,
    ) -> Result<Option<Box<dyn CollectLockGuard>>, RepositoryError> {
        // 事务级 advisory lock：锁绑定在下面这个打开的事务上，事务固定占用一条
        // 物理连接，release 回滚事务即确定性释放（不受连接池换连接影响）。
        let conn = self.admin_conn().await?;
        let tx = conn
            .begin()
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let row = tx
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT pg_try_advisory_xact_lock($1) AS locked",
                [key.into()],
            ))
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        // SELECT 必返回一行；行缺失/列解码失败是异常，不能当成「锁被占用」
        let row = row.ok_or_else(|| {
            RepositoryError::Database(anyhow::anyhow!("pg_try_advisory_xact_lock returned no row"))
        })?;
        let acquired = row
            .try_get::<bool>("", "locked")
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if acquired {
            Ok(Some(Box::new(PgCollectLockGuard {
                tx: parking_lot::Mutex::new(Some(tx)),
            })))
        } else {
            // 未获锁：立即回滚事务，归还连接
            tx.rollback()
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
            Ok(None)
        }
    }

    async fn upsert_from_dataset(
        &self,
        instance: &AdmittedInstance,
    ) -> Result<UpsertOutcome, RepositoryError> {
        let conn = self.admin_conn().await?;
        let now = now_tz();
        let values = dataset_insert_values(instance, now);

        // 单语句 upsert：消除「find → insert」竞态窗口。并发同 url 插入由唯一键 +
        // ON CONFLICT 串行化；`xmax = 0` 区分本次是插入还是冲突更新。
        // 重新出现在数据集 → 视为回归：退役实例在冲突更新里回 candidate。
        let row = conn
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                DATASET_UPSERT_SQL,
                values,
            ))
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        let inserted = inserted_flag(row)?;
        Ok(if inserted {
            UpsertOutcome::Inserted
        } else {
            UpsertOutcome::Updated
        })
    }

    async fn upsert_seed(&self, url: &str) -> Result<UpsertOutcome, RepositoryError> {
        // 契约：调用方已用 `normalize_instance_url` 规范化；此处只做最低限度的形态校验，
        // 避免把畸形值写进唯一键。
        if !url.starts_with("https://") || url.len() <= "https://".len() {
            return Err(RepositoryError::Database(anyhow::anyhow!(
                "upsert_seed requires a normalized https url, got: {url}"
            )));
        }

        let conn = self.admin_conn().await?;
        let now = now_tz();
        // 单语句 upsert：冲突时刷 source='seed' 与 updated_at；同时用 xmax 区分结果。
        let row = conn
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                SEED_INSERT_SQL,
                seed_insert_values(url, now),
            ))
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        let inserted = inserted_flag(row)?;
        Ok(if inserted {
            UpsertOutcome::Inserted
        } else {
            UpsertOutcome::Updated
        })
    }

    async fn list_candidates(&self, limit: u64) -> Result<Vec<SearxngInstance>, RepositoryError> {
        let conn = self.admin_conn().await?;
        let models = searxng_instance::Entity::find()
            // candidate 已蕴含非 retired，无需再加 State.ne(Retired)
            .filter(searxng_instance::Column::State.eq(SearxngInstanceState::Candidate.as_str()))
            .order_by_with_nulls(
                searxng_instance::Column::UptimeDay,
                Order::Desc,
                NullOrdering::Last,
            )
            .order_by_with_nulls(
                searxng_instance::Column::LatencyMs,
                Order::Asc,
                NullOrdering::Last,
            )
            .limit(limit)
            .all(&conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        Ok(models.into_iter().map(SearxngInstance::from).collect())
    }

    async fn list_by_state(
        &self,
        state: SearxngInstanceState,
        limit: u64,
    ) -> Result<Vec<SearxngInstance>, RepositoryError> {
        let conn = self.admin_conn().await?;
        let models = searxng_instance::Entity::find()
            .filter(searxng_instance::Column::State.eq(state.as_str()))
            .order_by_with_nulls(
                searxng_instance::Column::UptimeDay,
                Order::Desc,
                NullOrdering::Last,
            )
            .limit(limit)
            .all(&conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        Ok(models.into_iter().map(SearxngInstance::from).collect())
    }

    async fn list_paginated(
        &self,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<SearxngInstance>, RepositoryError> {
        let conn = self.admin_conn().await?;
        // state 字典序恰好是期望的优先级：active < candidate < cooldown < retired
        let models = searxng_instance::Entity::find()
            .order_by_asc(searxng_instance::Column::State)
            .order_by_with_nulls(
                searxng_instance::Column::UptimeDay,
                Order::Desc,
                NullOrdering::Last,
            )
            .offset(offset)
            .limit(limit)
            .all(&conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        Ok(models.into_iter().map(SearxngInstance::from).collect())
    }

    async fn record_probe_result(
        &self,
        url: &str,
        ok: bool,
        latency_ms: Option<i64>,
        error: Option<&str>,
    ) -> Result<(), RepositoryError> {
        let conn = self.admin_conn().await?;
        let now = now_tz();

        let update =
            searxng_instance::Entity::update_many().filter(searxng_instance::Column::Url.eq(url));
        let update = if ok {
            update
                .col_expr(
                    searxng_instance::Column::ConsecutiveFailures,
                    Expr::value(0),
                )
                .col_expr(searxng_instance::Column::LatencyMs, Expr::value(latency_ms))
                .col_expr(
                    searxng_instance::Column::LastSuccessAt,
                    Expr::value(Some(now)),
                )
                .col_expr(
                    searxng_instance::Column::LastError,
                    Expr::value(None::<String>),
                )
        } else {
            update
                .col_expr(
                    searxng_instance::Column::ConsecutiveFailures,
                    Expr::col(searxng_instance::Column::ConsecutiveFailures).add(1),
                )
                .col_expr(
                    searxng_instance::Column::LastFailureAt,
                    Expr::value(Some(now)),
                )
                .col_expr(
                    searxng_instance::Column::LastError,
                    Expr::value(truncate_error(error)),
                )
        };
        let result = update
            .col_expr(searxng_instance::Column::UpdatedAt, Expr::value(now))
            .exec(&conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        if result.rows_affected == 0 {
            return Err(RepositoryError::NotFound);
        }
        Ok(())
    }

    async fn transition_state(
        &self,
        url: &str,
        to: SearxngInstanceState,
    ) -> Result<(), RepositoryError> {
        let model = self
            .find_model(url)
            .await?
            .ok_or(RepositoryError::NotFound)?;
        let from: SearxngInstanceState = model
            .state
            .parse()
            .map_err(|e| RepositoryError::Database(anyhow::anyhow!("{e}")))?;
        if !from.can_transition_to(to) {
            return Err(RepositoryError::Database(anyhow::anyhow!(
                "illegal searxng instance state transition for {url}: {from} -> {to}"
            )));
        }
        if from == to {
            return Ok(());
        }

        let conn = self.admin_conn().await?;
        let mut active: searxng_instance::ActiveModel = model.into();
        active.state = Set(to.as_str().to_string());
        active.updated_at = Set(now_tz());
        active
            .update(&conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        Ok(())
    }

    async fn record_dataset_miss(
        &self,
        url: &str,
        retire_after: i32,
    ) -> Result<bool, RepositoryError> {
        let conn = self.admin_conn().await?;
        let tx = conn
            .begin()
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        // 行锁串行化「读计数 → 阈值判定 → 状态迁移 → 写回」：
        // 并发两次 miss 各自递增（不再同读同值互相覆盖），Active→Cooldown 迁移不丢失。
        let model = find_locked_instance(&tx, url).await?;

        // seed 来源永不因数据集缺席退役（也不累计缺席计数）
        if model.source != "searx_space" {
            tx.commit()
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
            return Ok(false);
        }

        let miss_count = model.dataset_miss_count.saturating_add(1);
        let current_state: SearxngInstanceState = model
            .state
            .parse()
            .map_err(|e| RepositoryError::Database(anyhow::anyhow!("{e}")))?;

        let (next_state, retired) = next_miss_state(current_state, miss_count, retire_after);

        persist_dataset_miss(&tx, model, miss_count, next_state).await?;
        tx.commit()
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        Ok(retired)
    }

    async fn get_by_url(&self, url: &str) -> Result<Option<SearxngInstance>, RepositoryError> {
        Ok(self.find_model(url).await?.map(SearxngInstance::from))
    }

    async fn count_by_state(&self, state: SearxngInstanceState) -> Result<u64, RepositoryError> {
        let conn = self.admin_conn().await?;
        searxng_instance::Entity::find()
            .filter(searxng_instance::Column::State.eq(state.as_str()))
            .count(&conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_error_caps_length() {
        let long = "x".repeat(LAST_ERROR_MAX_CHARS + 100);
        let truncated = truncate_error(Some(&long)).expect("some");
        assert_eq!(truncated.chars().count(), LAST_ERROR_MAX_CHARS);
    }

    #[test]
    fn test_truncate_error_preserves_none_and_short() {
        assert!(truncate_error(None).is_none());
        assert_eq!(truncate_error(Some("boom")), Some("boom".to_string()));
    }

    #[test]
    fn test_now_tz_is_utc() {
        assert_eq!(now_tz().offset().local_minus_utc(), 0);
    }
}
