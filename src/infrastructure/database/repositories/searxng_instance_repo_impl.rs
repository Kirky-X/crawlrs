// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SearXNG 实例登记簿仓储实现（Sea-ORM / PostgreSQL）

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Utc};
use dbnexus::sea_orm::sea_query::{Expr, ExprTrait, NullOrdering};
use dbnexus::sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, Order,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set, Statement,
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

/// PG 会话级 advisory lock 守卫：持有 Session（同一会话才能正确解锁）
struct PgCollectLockGuard {
    session: dbnexus::Session,
}

#[async_trait]
impl CollectLockGuard for PgCollectLockGuard {
    async fn release(&self) -> Result<(), RepositoryError> {
        let conn = self
            .session
            .connection()
            .map_err(|e| RepositoryError::Database(e.into()))?;
        conn.execute_unprepared("SELECT pg_advisory_unlock_all()")
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

/// 错误串截断
fn truncate_error(error: Option<&str>) -> Option<String> {
    error.map(|e| e.chars().take(LAST_ERROR_MAX_CHARS).collect())
}

#[async_trait]
impl SearxngInstanceRepository for SearxngInstanceRepositoryImpl {
    async fn try_acquire_collect_lock(
        &self,
        key: i64,
    ) -> Result<Option<Box<dyn CollectLockGuard>>, RepositoryError> {
        // 会话级 advisory lock：守卫必须持有同一 Session 才能正确解锁
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let acquired = {
            let conn = session
                .connection()
                .map_err(|e| RepositoryError::Database(e.into()))?;
            let row = conn
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT pg_try_advisory_lock($1) AS locked",
                    [key.into()],
                ))
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
            row.and_then(|r| r.try_get::<bool>("", "locked").ok())
                .unwrap_or(false)
        };

        if acquired {
            Ok(Some(Box::new(PgCollectLockGuard { session })))
        } else {
            Ok(None)
        }
    }

    async fn upsert_from_dataset(
        &self,
        instance: &AdmittedInstance,
    ) -> Result<UpsertOutcome, RepositoryError> {
        let conn = self.admin_conn().await?;
        let now = now_tz();

        let existing = searxng_instance::Entity::find()
            .filter(searxng_instance::Column::Url.eq(instance.url.as_str()))
            .one(&conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        match existing {
            Some(model) => {
                // 重新出现在数据集 → 视为回归：退役实例回到候选（重新走探活准入）
                let was_retired = model.state == SearxngInstanceState::Retired.as_str();
                let mut active: searxng_instance::ActiveModel = model.into();
                active.network_type = Set(instance.network_type.clone());
                active.version = Set(instance.version.clone());
                active.api_grade = Set(instance.api_grade.clone());
                active.http_grade = Set(instance.http_grade.clone());
                active.tls_grade = Set(instance.tls_grade.clone());
                active.cdn_grade = Set(instance.cdn_grade.clone());
                active.uptime_day = Set(instance.uptime_day);
                active.uptime_month = Set(instance.uptime_month);
                active.dataset_miss_count = Set(0);
                if was_retired {
                    active.state = Set(SearxngInstanceState::Candidate.as_str().to_string());
                }
                active.updated_at = Set(now);
                active
                    .update(&conn)
                    .await
                    .map_err(|e| RepositoryError::Database(e.into()))?;
                Ok(UpsertOutcome::Updated)
            }
            None => {
                let model = searxng_instance::ActiveModel {
                    id: Set(Id::generate().to_uuid_v8()),
                    url: Set(instance.url.clone()),
                    source: Set("searx_space".to_string()),
                    state: Set(SearxngInstanceState::Candidate.as_str().to_string()),
                    network_type: Set(instance.network_type.clone()),
                    version: Set(instance.version.clone()),
                    api_grade: Set(instance.api_grade.clone()),
                    http_grade: Set(instance.http_grade.clone()),
                    tls_grade: Set(instance.tls_grade.clone()),
                    cdn_grade: Set(instance.cdn_grade.clone()),
                    uptime_day: Set(instance.uptime_day),
                    uptime_month: Set(instance.uptime_month),
                    latency_ms: Set(None),
                    consecutive_failures: Set(0),
                    last_success_at: Set(None),
                    last_failure_at: Set(None),
                    last_error: Set(None),
                    dataset_miss_count: Set(0),
                    created_at: Set(now),
                    updated_at: Set(now),
                };
                model
                    .insert(&conn)
                    .await
                    .map_err(|e| RepositoryError::Database(e.into()))?;
                Ok(UpsertOutcome::Inserted)
            }
        }
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

        let existing = searxng_instance::Entity::find()
            .filter(searxng_instance::Column::Url.eq(url))
            .one(&conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        match existing {
            Some(model) => {
                let mut active: searxng_instance::ActiveModel = model.into();
                // seed 语义：来源改为 seed（此后采集器不会因数据集缺席退役它）
                active.source = Set("seed".to_string());
                active.updated_at = Set(now);
                active
                    .update(&conn)
                    .await
                    .map_err(|e| RepositoryError::Database(e.into()))?;
                Ok(UpsertOutcome::Updated)
            }
            None => {
                let model = searxng_instance::ActiveModel {
                    id: Set(Id::generate().to_uuid_v8()),
                    url: Set(url.to_string()),
                    source: Set("seed".to_string()),
                    state: Set(SearxngInstanceState::Candidate.as_str().to_string()),
                    network_type: Set(None),
                    version: Set(None),
                    api_grade: Set(None),
                    http_grade: Set(None),
                    tls_grade: Set(None),
                    cdn_grade: Set(None),
                    uptime_day: Set(None),
                    uptime_month: Set(None),
                    latency_ms: Set(None),
                    consecutive_failures: Set(0),
                    last_success_at: Set(None),
                    last_failure_at: Set(None),
                    last_error: Set(None),
                    dataset_miss_count: Set(0),
                    created_at: Set(now),
                    updated_at: Set(now),
                };
                model
                    .insert(&conn)
                    .await
                    .map_err(|e| RepositoryError::Database(e.into()))?;
                Ok(UpsertOutcome::Inserted)
            }
        }
    }

    async fn list_candidates(&self, limit: u64) -> Result<Vec<SearxngInstance>, RepositoryError> {
        let conn = self.admin_conn().await?;
        let models = searxng_instance::Entity::find()
            .filter(searxng_instance::Column::State.eq(SearxngInstanceState::Candidate.as_str()))
            .filter(searxng_instance::Column::State.ne(SearxngInstanceState::Retired.as_str()))
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
        let model = self
            .find_model(url)
            .await?
            .ok_or(RepositoryError::NotFound)?;
        // seed 来源永不因数据集缺席退役（也不累计缺席计数）
        if model.source != "searx_space" {
            return Ok(false);
        }

        let miss_count = model.dataset_miss_count.saturating_add(1);
        let current_state: SearxngInstanceState = model
            .state
            .parse()
            .map_err(|e| RepositoryError::Database(anyhow::anyhow!("{e}")))?;

        // 超阈值 → 退役；`active` 不允许直迁 `retired`（状态机约束），先落 cooldown，
        // 后续轮次由 cooldown→retired 完成退役。
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

        let conn = self.admin_conn().await?;
        let mut active: searxng_instance::ActiveModel = model.into();
        active.dataset_miss_count = Set(miss_count);
        active.state = Set(next_state.as_str().to_string());
        active.updated_at = Set(now_tz());
        active
            .update(&conn)
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
