// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 数据保留期清理工作器（R-retention-005）
//!
//! 按 `RetentionSettings` 配置周期清理四类数据：
//!   - scrape_results（按 created_at）
//!   - geo_restriction_logs（按 created_at）
//!   - webhook_events 终态（delivered 按 delivered_at、dead 按 updated_at）
//!   - audit_logs（按 created_at，复用已有 `cleanup_old_logs`）
//!
//! 失败语义：单类清理失败不中断其余三类；全部完成后有失败则返回
//! `ProcessResult::Error` 并聚合失败类别（规则 12：失败必须显性化）。
//! 每类删除行数 > 0 时输出含类别名与行数的 info 日志。

use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::retention_policy::RetentionBatchPolicy;
use crate::domain::services::audit_service::AuditServiceTrait;
use crate::workers::worker::{ProcessResult, WorkerProcess};
use async_trait::async_trait;
use log::{debug, error, info};
use sea_orm::ConnectionTrait;
use std::sync::Arc;

/// retention 周期互斥锁的固定键（R-retention-008：多实例部署下仅一个实例执行清理）
pub const RETENTION_ADVISORY_LOCK_ID: i64 = 0x0052_4554_4E54_0001;

/// 多实例互斥锁缝（R-retention-008）。
///
/// `try_acquire` 返回 `false` 表示其他实例持有锁，本轮跳过；
/// `release` 必须在周期结束（含错误路径）调用。
#[async_trait]
pub trait RetentionLock: Send + Sync {
    async fn try_acquire(&self) -> anyhow::Result<bool>;
    async fn release(&self) -> anyhow::Result<()>;
}

/// PG 会话级 advisory lock 实现（R-retention-008）。
///
/// `try_acquire` 从池中租借一个**专有连接**（DbSession）并在其上执行
/// `pg_try_advisory_lock`；该 session 被持有期间连接不回池，会话级锁保持有效。
/// `release` 在同一连接上执行 `pg_advisory_unlock` 后归还连接。
/// 锁连接与批处理连接分离，互不占用。
pub struct PgRetentionLock {
    pool: Arc<dbnexus::DbPool>,
    held: tokio::sync::Mutex<Option<dbnexus::Session>>,
}

impl PgRetentionLock {
    pub fn new(pool: Arc<dbnexus::DbPool>) -> Self {
        Self {
            pool,
            held: tokio::sync::Mutex::new(None),
        }
    }
}

#[async_trait]
impl RetentionLock for PgRetentionLock {
    async fn try_acquire(&self) -> anyhow::Result<bool> {
        let mut held = self.held.lock().await;
        if held.is_some() {
            anyhow::bail!("retention lock already held by this worker");
        }
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| anyhow::anyhow!("retention lock session: {e}"))?;
        let conn = session
            .connection()
            .map_err(|e| anyhow::anyhow!("retention lock connection: {e}"))?;
        let stmt = sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT pg_try_advisory_lock($1) AS acquired",
            [RETENTION_ADVISORY_LOCK_ID.into()],
        );
        let row = conn
            .query_one_raw(stmt)
            .await
            .map_err(|e| anyhow::anyhow!("pg_try_advisory_lock: {e}"))?;
        let acquired = row
            .and_then(|r| r.try_get::<bool>("", "acquired").ok())
            .unwrap_or(false);
        if acquired {
            // 持有 session（即持有连接）直至 release，保证会话级锁有效
            *held = Some(session);
        }
        Ok(acquired)
    }

    async fn release(&self) -> anyhow::Result<()> {
        let mut held = self.held.lock().await;
        let session = held
            .take()
            .ok_or_else(|| anyhow::anyhow!("retention lock not held"))?;
        let conn = session
            .connection()
            .map_err(|e| anyhow::anyhow!("retention unlock connection: {e}"))?;
        let stmt = sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT pg_advisory_unlock($1) AS released",
            [RETENTION_ADVISORY_LOCK_ID.into()],
        );
        conn.query_one_raw(stmt)
            .await
            .map_err(|e| anyhow::anyhow!("pg_advisory_unlock: {e}"))?;
        // session drop → 连接回池 → 会话结束 → 锁自动释放（双保险）
        drop(session);
        Ok(())
    }
}

/// 数据保留期清理工作器
pub struct RetentionWorker {
    scrape_repo: Arc<dyn ScrapeResultRepository>,
    // teams 门控修复（R-teams-004）：geo 仓库仅 teams-on 有 accessor；teams-off
    // 传 None，run 跳过 geo 类清理（geo_restriction_logs 仅 teams 语义存在）。
    geo_repo: Option<Arc<dyn GeoRestrictionRepository>>,
    webhook_repo: Arc<dyn WebhookEventRepository>,
    audit_service: Arc<dyn AuditServiceTrait>,
    scrape_results_days: i64,
    geo_logs_days: i64,
    webhook_events_days: i64,
    audit_logs_days: i64,
    /// 有界删除参数（retention-worker-hardening R-retention-002）
    policy: RetentionBatchPolicy,
    /// 逐类清理超时（秒，R-retention-009：单类慢清理不阻塞其余三类）
    category_timeout_seconds: u64,
    /// 多实例互斥锁（R-retention-008）
    lock: Arc<dyn RetentionLock>,
}

impl RetentionWorker {
    /// 构造清理工作器；天数参数由调用方从 `Settings.retention` 传入。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scrape_repo: Arc<dyn ScrapeResultRepository>,
        geo_repo: Option<Arc<dyn GeoRestrictionRepository>>,
        webhook_repo: Arc<dyn WebhookEventRepository>,
        audit_service: Arc<dyn AuditServiceTrait>,
        scrape_results_days: i64,
        geo_logs_days: i64,
        webhook_events_days: i64,
        audit_logs_days: i64,
        policy: RetentionBatchPolicy,
        category_timeout_seconds: u64,
        lock: Arc<dyn RetentionLock>,
    ) -> Self {
        Self {
            scrape_repo,
            geo_repo,
            webhook_repo,
            audit_service,
            scrape_results_days,
            geo_logs_days,
            webhook_events_days,
            audit_logs_days,
            policy,
            category_timeout_seconds,
            lock,
        }
    }

    /// 顺序执行四类清理；返回收集到的错误描述（类别名 + 错误信息）。
    /// 每类以 `category_timeout_seconds` 超时包裹：超时该类记为错误、其余类继续
    /// （R-retention-009）。
    async fn run_cleanup(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let category_timeout = std::time::Duration::from_secs(self.category_timeout_seconds);

        // scrape_results
        match tokio::time::timeout(
            category_timeout,
            self.scrape_repo
                .cleanup_expired(self.scrape_results_days, &self.policy),
        )
        .await
        {
            Ok(Ok(count)) => {
                if count > 0 {
                    info!("Retention: cleaned up {} expired scrape_results", count);
                }
            }
            Ok(Err(e)) => {
                error!("Retention: scrape_results cleanup failed: {}", e);
                errors.push(format!("scrape_results: {}", e));
            }
            Err(_) => {
                error!(
                    "Retention: scrape_results cleanup timed out after {}s",
                    self.category_timeout_seconds
                );
                errors.push(format!(
                    "scrape_results: timed out after {}s",
                    self.category_timeout_seconds
                ));
            }
        }

        // geo_restriction_logs（仅 teams-on 提供 geo 仓库时执行；teams-off 无 geo 语义）
        if let Some(geo_repo) = &self.geo_repo {
            match tokio::time::timeout(
                category_timeout,
                geo_repo.cleanup_expired(self.geo_logs_days, &self.policy),
            )
            .await
            {
                Ok(Ok(count)) => {
                    if count > 0 {
                        info!(
                            "Retention: cleaned up {} expired geo_restriction_logs",
                            count
                        );
                    }
                }
                Ok(Err(e)) => {
                    error!("Retention: geo_restriction_logs cleanup failed: {}", e);
                    errors.push(format!("geo_restriction_logs: {}", e));
                }
                Err(_) => {
                    error!(
                        "Retention: geo_restriction_logs cleanup timed out after {}s",
                        self.category_timeout_seconds
                    );
                    errors.push(format!(
                        "geo_restriction_logs: timed out after {}s",
                        self.category_timeout_seconds
                    ));
                }
            }
        }

        // webhook_events
        match tokio::time::timeout(
            category_timeout,
            self.webhook_repo
                .cleanup_terminal(self.webhook_events_days, &self.policy),
        )
        .await
        {
            Ok(Ok(count)) => {
                if count > 0 {
                    info!("Retention: cleaned up {} terminal webhook_events", count);
                }
            }
            Ok(Err(e)) => {
                error!("Retention: webhook_events cleanup failed: {}", e);
                errors.push(format!("webhook_events: {}", e));
            }
            Err(_) => {
                error!(
                    "Retention: webhook_events cleanup timed out after {}s",
                    self.category_timeout_seconds
                );
                errors.push(format!(
                    "webhook_events: timed out after {}s",
                    self.category_timeout_seconds
                ));
            }
        }

        // audit_logs
        match tokio::time::timeout(
            category_timeout,
            self.audit_service
                .cleanup_old_logs(self.audit_logs_days, &self.policy),
        )
        .await
        {
            Ok(Ok(count)) => {
                if count > 0 {
                    info!("Retention: cleaned up {} old audit_logs", count);
                }
            }
            Ok(Err(e)) => {
                error!("Retention: audit_logs cleanup failed: {}", e);
                errors.push(format!("audit_logs: {}", e));
            }
            Err(_) => {
                error!(
                    "Retention: audit_logs cleanup timed out after {}s",
                    self.category_timeout_seconds
                );
                errors.push(format!(
                    "audit_logs: timed out after {}s",
                    self.category_timeout_seconds
                ));
            }
        }

        errors
    }
}

#[async_trait]
impl WorkerProcess for RetentionWorker {
    fn name(&self) -> &str {
        "retention-worker"
    }

    async fn process(&self) -> ProcessResult {
        // R-retention-008：多实例互斥——未抢到锁的实例跳过本轮
        match self.lock.try_acquire().await {
            Ok(true) => {}
            Ok(false) => {
                debug!("Retention: another instance holds the lock, skip this cycle");
                return ProcessResult::Completed;
            }
            Err(e) => {
                error!("Retention: lock try_acquire failed: {}", e);
                return ProcessResult::Error(format!("retention lock acquire: {}", e));
            }
        }

        let mut errors = self.run_cleanup().await;

        // 锁必须在周期结束（含错误路径）释放
        if let Err(e) = self.lock.release().await {
            error!("Retention: lock release failed: {}", e);
            errors.push(format!("lock_release: {}", e));
        }

        if errors.is_empty() {
            ProcessResult::Completed
        } else {
            ProcessResult::Error(format!("Retention cleanup failed: {}", errors.join("; ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::auth::AuditLogEntry;
    use crate::domain::models::{ScrapeResult, WebhookEvent};
    use crate::domain::services::audit_service::{AuditServiceError, AuditServiceTrait};
    use crate::domain::services::team_service::TeamGeoRestrictions;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use uuid::Uuid;

    // ========== Mock 仓库 ==========

    struct MockScrapeRepo {
        cleanup_calls: AtomicU64,
        result: Mutex<Option<anyhow::Result<u64>>>,
        /// cleanup 时的延迟（R-retention-009 逐类超时用例）
        delay: Option<tokio::time::Duration>,
    }

    impl MockScrapeRepo {
        fn succeeding() -> Self {
            Self {
                cleanup_calls: AtomicU64::new(0),
                result: Mutex::new(None),
                delay: None,
            }
        }
        fn failing() -> Self {
            Self {
                cleanup_calls: AtomicU64::new(0),
                result: Mutex::new(Some(Err(anyhow::anyhow!("db down")))),
                delay: None,
            }
        }
        fn slow(delay: tokio::time::Duration) -> Self {
            Self {
                cleanup_calls: AtomicU64::new(0),
                result: Mutex::new(None),
                delay: Some(delay),
            }
        }
    }

    #[async_trait]
    impl ScrapeResultRepository for MockScrapeRepo {
        async fn save(&self, _result: ScrapeResult) -> anyhow::Result<()> {
            Ok(())
        }
        async fn find_by_task_id(&self, _task_id: Uuid) -> anyhow::Result<Option<ScrapeResult>> {
            Ok(None)
        }
        async fn find_by_task_ids(&self, _task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
            Ok(vec![])
        }
        async fn get_team_avg_response_time(&self, _team_id: Uuid) -> anyhow::Result<f64> {
            Ok(0.0)
        }
        async fn cleanup_expired(
            &self,
            _retention_days: i64,
            _policy: &RetentionBatchPolicy,
        ) -> anyhow::Result<u64> {
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            self.cleanup_calls.fetch_add(1, Ordering::SeqCst);
            match self.result.lock().unwrap().take() {
                Some(r) => r,
                None => Ok(5),
            }
        }
    }

    struct MockGeoRepo {
        cleanup_calls: AtomicU64,
        fail: bool,
    }

    impl MockGeoRepo {
        fn new(fail: bool) -> Self {
            Self {
                cleanup_calls: AtomicU64::new(0),
                fail,
            }
        }
    }

    #[async_trait]
    impl GeoRestrictionRepository for MockGeoRepo {
        async fn get_team_restrictions(
            &self,
            _team_id: Uuid,
        ) -> Result<
            TeamGeoRestrictions,
            crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
        > {
            Ok(TeamGeoRestrictions::default())
        }
        async fn update_team_restrictions(
            &self,
            _team_id: Uuid,
            _restrictions: &TeamGeoRestrictions,
        ) -> Result<
            (),
            crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
        > {
            Ok(())
        }
        async fn log_geo_restriction_action(
            &self,
            _team_id: Uuid,
            _ip_address: &str,
            _country_code: &str,
            _action: &str,
            _reason: &str,
        ) -> Result<
            (),
            crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
        > {
            Ok(())
        }
        async fn cleanup_expired(
            &self,
            _retention_days: i64,
            _policy: &RetentionBatchPolicy,
        ) -> Result<
            u64,
            crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
        > {
            self.cleanup_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err(crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError::Database("db down".to_string()))
            } else {
                Ok(2)
            }
        }
    }

    struct MockWebhookRepo {
        cleanup_calls: AtomicU64,
    }

    #[async_trait]
    impl WebhookEventRepository for MockWebhookRepo {
        async fn create(
            &self,
            event: &WebhookEvent,
        ) -> Result<WebhookEvent, crate::domain::repositories::task_repository::RepositoryError>
        {
            Ok(event.clone())
        }
        async fn find_by_id(
            &self,
            _id: Uuid,
        ) -> Result<
            Option<WebhookEvent>,
            crate::domain::repositories::task_repository::RepositoryError,
        > {
            Ok(None)
        }
        async fn find_pending(
            &self,
            _limit: u64,
        ) -> Result<Vec<WebhookEvent>, crate::domain::repositories::task_repository::RepositoryError>
        {
            Ok(vec![])
        }
        async fn find_by_team_id_paginated(
            &self,
            _team_id: Uuid,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<WebhookEvent>, crate::domain::repositories::task_repository::RepositoryError>
        {
            Ok(vec![])
        }
        async fn count_by_team_id(
            &self,
            _team_id: Uuid,
        ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
            Ok(0)
        }
        async fn update(
            &self,
            event: &WebhookEvent,
        ) -> Result<WebhookEvent, crate::domain::repositories::task_repository::RepositoryError>
        {
            Ok(event.clone())
        }
        async fn cleanup_terminal(
            &self,
            _retention_days: i64,
            _policy: &RetentionBatchPolicy,
        ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
            self.cleanup_calls.fetch_add(1, Ordering::SeqCst);
            Ok(3)
        }
    }

    struct MockAuditService {
        cleanup_calls: AtomicU64,
    }

    #[async_trait]
    impl AuditServiceTrait for MockAuditService {
        async fn log(&self, _entry: AuditLogEntry) -> Result<(), AuditServiceError> {
            Ok(())
        }
        async fn log_allow(
            &self,
            _action: String,
            _api_key_id: Uuid,
            _team_id: Uuid,
            _scope: crate::domain::auth::ApiKeyScope,
        ) -> Result<(), AuditServiceError> {
            Ok(())
        }
        async fn log_deny(
            &self,
            _action: String,
            _api_key_id: Option<Uuid>,
            _team_id: Option<Uuid>,
            _reason: String,
            _scope: Option<crate::domain::auth::ApiKeyScope>,
        ) -> Result<(), AuditServiceError> {
            Ok(())
        }
        async fn get_logs_for_key(
            &self,
            _api_key_id: Uuid,
            _limit: u64,
            _offset: u64,
        ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
            Ok(vec![])
        }
        async fn get_logs_for_team(
            &self,
            _team_id: Uuid,
            _limit: u64,
            _offset: u64,
        ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
            Ok(vec![])
        }
        async fn get_denied_requests(
            &self,
            _api_key_id: Uuid,
            _limit: u64,
        ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
            Ok(vec![])
        }
        async fn cleanup_old_logs(
            &self,
            _retention_days: i64,
            _policy: &crate::domain::retention_policy::RetentionBatchPolicy,
        ) -> Result<u64, AuditServiceError> {
            self.cleanup_calls.fetch_add(1, Ordering::SeqCst);
            Ok(0)
        }
    }

    // ========== 用例 ==========

    fn make_worker(
        scrape: Arc<dyn ScrapeResultRepository>,
        geo: Arc<dyn GeoRestrictionRepository>,
        webhook: Arc<dyn WebhookEventRepository>,
        audit: Arc<dyn AuditServiceTrait>,
    ) -> RetentionWorker {
        RetentionWorker::new(
            scrape,
            Some(geo),
            webhook,
            audit,
            30,
            90,
            30,
            90,
            RetentionBatchPolicy::default(),
            300,
            Arc::new(MockRetentionLock::acquires()),
        )
    }

    // ========== Mock 锁（R-retention-008） ==========

    struct MockRetentionLock {
        available: std::sync::Mutex<bool>,
        acquire_calls: AtomicU64,
        release_calls: AtomicU64,
    }

    impl MockRetentionLock {
        /// 每次都获取成功
        fn acquires() -> Self {
            Self {
                available: std::sync::Mutex::new(true),
                acquire_calls: AtomicU64::new(0),
                release_calls: AtomicU64::new(0),
            }
        }
        /// 永远获取失败（模拟其他实例持锁）
        fn never_acquires() -> Self {
            Self {
                available: std::sync::Mutex::new(false),
                acquire_calls: AtomicU64::new(0),
                release_calls: AtomicU64::new(0),
            }
        }
    }

    #[async_trait]
    impl RetentionLock for MockRetentionLock {
        async fn try_acquire(&self) -> anyhow::Result<bool> {
            self.acquire_calls.fetch_add(1, Ordering::SeqCst);
            let mut available = self.available.lock().unwrap();
            if *available {
                *available = false;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        async fn release(&self) -> anyhow::Result<()> {
            self.release_calls.fetch_add(1, Ordering::SeqCst);
            *self.available.lock().unwrap() = true;
            Ok(())
        }
    }

    /// 成功路径：四类清理各执行一次，process 返回 Completed。
    #[tokio::test]
    async fn test_process_success_calls_all_four_repos() {
        let scrape = Arc::new(MockScrapeRepo::succeeding());
        let geo = Arc::new(MockGeoRepo::new(false));
        let webhook = Arc::new(MockWebhookRepo {
            cleanup_calls: AtomicU64::new(0),
        });
        let audit = Arc::new(MockAuditService {
            cleanup_calls: AtomicU64::new(0),
        });
        let worker = make_worker(scrape.clone(), geo.clone(), webhook.clone(), audit.clone());

        let result = worker.process().await;
        assert!(
            matches!(result, ProcessResult::Completed),
            "expected Completed, got {:?}",
            result
        );
        assert_eq!(scrape.cleanup_calls.load(Ordering::SeqCst), 1);
        assert_eq!(geo.cleanup_calls.load(Ordering::SeqCst), 1);
        assert_eq!(webhook.cleanup_calls.load(Ordering::SeqCst), 1);
        assert_eq!(audit.cleanup_calls.load(Ordering::SeqCst), 1);
    }

    /// 失败语义：scrape 失败时其余三类仍被调用，process 返回 Error 且错误串含类别名。
    #[tokio::test]
    async fn test_process_scrape_failure_continues_others() {
        let scrape = Arc::new(MockScrapeRepo::failing());
        let geo = Arc::new(MockGeoRepo::new(false));
        let webhook = Arc::new(MockWebhookRepo {
            cleanup_calls: AtomicU64::new(0),
        });
        let audit = Arc::new(MockAuditService {
            cleanup_calls: AtomicU64::new(0),
        });
        let worker = make_worker(scrape.clone(), geo.clone(), webhook.clone(), audit.clone());

        let result = worker.process().await;
        match result {
            ProcessResult::Error(msg) => {
                assert!(
                    msg.contains("scrape_results"),
                    "error should name category: {msg}"
                );
            }
            other => panic!("expected Error, got {:?}", other),
        }
        assert_eq!(
            geo.cleanup_calls.load(Ordering::SeqCst),
            1,
            "geo must still run"
        );
        assert_eq!(
            webhook.cleanup_calls.load(Ordering::SeqCst),
            1,
            "webhook must still run"
        );
        assert_eq!(
            audit.cleanup_calls.load(Ordering::SeqCst),
            1,
            "audit must still run"
        );
    }

    /// 失败语义：多处失败时错误串聚合全部失败类别。
    #[tokio::test]
    async fn test_process_multiple_failures_aggregated() {
        let scrape = Arc::new(MockScrapeRepo::failing());
        let geo = Arc::new(MockGeoRepo::new(true));
        let webhook = Arc::new(MockWebhookRepo {
            cleanup_calls: AtomicU64::new(0),
        });
        let audit = Arc::new(MockAuditService {
            cleanup_calls: AtomicU64::new(0),
        });
        let worker = make_worker(scrape.clone(), geo.clone(), webhook.clone(), audit.clone());

        let result = worker.process().await;
        match result {
            ProcessResult::Error(msg) => {
                assert!(msg.contains("scrape_results"));
                assert!(msg.contains("geo_restriction_logs"));
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    /// R-retention-009：scrape 清理耗时 2s 而逐类超时 1s → scrape 记超时错误，
    /// 其余三类仍被调用，process 返回 Error 且错误串含超时类别名。
    #[tokio::test]
    async fn test_process_slow_scrape_times_out() {
        let scrape = Arc::new(MockScrapeRepo::slow(tokio::time::Duration::from_secs(2)));
        let geo = Arc::new(MockGeoRepo::new(false));
        let webhook = Arc::new(MockWebhookRepo {
            cleanup_calls: AtomicU64::new(0),
        });
        let audit = Arc::new(MockAuditService {
            cleanup_calls: AtomicU64::new(0),
        });
        let worker = RetentionWorker::new(
            scrape.clone(),
            Some(geo.clone()),
            webhook.clone(),
            audit.clone(),
            30,
            90,
            30,
            90,
            RetentionBatchPolicy::default(),
            1, // 1s 超时 < 2s 清理耗时
            Arc::new(MockRetentionLock::acquires()),
        );

        let result = worker.process().await;
        match result {
            ProcessResult::Error(msg) => {
                assert!(
                    msg.contains("scrape_results") && msg.contains("timed out"),
                    "error should name timed-out category: {msg}"
                );
            }
            other => panic!("expected Error, got {:?}", other),
        }
        assert_eq!(
            geo.cleanup_calls.load(Ordering::SeqCst),
            1,
            "geo must still run after scrape timeout"
        );
        assert_eq!(
            webhook.cleanup_calls.load(Ordering::SeqCst),
            1,
            "webhook must still run after scrape timeout"
        );
        assert_eq!(
            audit.cleanup_calls.load(Ordering::SeqCst),
            1,
            "audit must still run after scrape timeout"
        );
    }

    /// name() 返回固定标识。
    #[test]
    fn test_name_is_retention_worker() {
        let worker = make_worker(
            Arc::new(MockScrapeRepo::succeeding()),
            Arc::new(MockGeoRepo::new(false)),
            Arc::new(MockWebhookRepo {
                cleanup_calls: AtomicU64::new(0),
            }),
            Arc::new(MockAuditService {
                cleanup_calls: AtomicU64::new(0),
            }),
        );
        assert_eq!(worker.name(), "retention-worker");
    }

    /// R-retention-008：未获取锁时四类清理零调用，process 返回 Completed（跳过本轮）。
    #[tokio::test]
    async fn test_process_skips_cleanup_when_lock_not_acquired() {
        let scrape = Arc::new(MockScrapeRepo::succeeding());
        let geo = Arc::new(MockGeoRepo::new(false));
        let webhook = Arc::new(MockWebhookRepo {
            cleanup_calls: AtomicU64::new(0),
        });
        let audit = Arc::new(MockAuditService {
            cleanup_calls: AtomicU64::new(0),
        });
        let lock = Arc::new(MockRetentionLock::never_acquires());
        let worker = RetentionWorker::new(
            scrape.clone(),
            Some(geo.clone()),
            webhook.clone(),
            audit.clone(),
            30,
            90,
            30,
            90,
            RetentionBatchPolicy::default(),
            300,
            lock.clone(),
        );

        let result = worker.process().await;
        assert!(
            matches!(result, ProcessResult::Completed),
            "skip cycle must report Completed, got {:?}",
            result
        );
        assert_eq!(scrape.cleanup_calls.load(Ordering::SeqCst), 0);
        assert_eq!(geo.cleanup_calls.load(Ordering::SeqCst), 0);
        assert_eq!(webhook.cleanup_calls.load(Ordering::SeqCst), 0);
        assert_eq!(audit.cleanup_calls.load(Ordering::SeqCst), 0);
        assert_eq!(lock.release_calls.load(Ordering::SeqCst), 0);
    }

    /// R-retention-008：获取锁成功时四类各执行一次，release 恰好一次。
    #[tokio::test]
    async fn test_process_releases_lock_exactly_once() {
        let scrape = Arc::new(MockScrapeRepo::succeeding());
        let geo = Arc::new(MockGeoRepo::new(false));
        let webhook = Arc::new(MockWebhookRepo {
            cleanup_calls: AtomicU64::new(0),
        });
        let audit = Arc::new(MockAuditService {
            cleanup_calls: AtomicU64::new(0),
        });
        let lock = Arc::new(MockRetentionLock::acquires());
        let worker = RetentionWorker::new(
            scrape.clone(),
            Some(geo.clone()),
            webhook.clone(),
            audit.clone(),
            30,
            90,
            30,
            90,
            RetentionBatchPolicy::default(),
            300,
            lock.clone(),
        );

        let result = worker.process().await;
        assert!(
            matches!(result, ProcessResult::Completed),
            "got {:?}",
            result
        );
        assert_eq!(scrape.cleanup_calls.load(Ordering::SeqCst), 1);
        assert_eq!(geo.cleanup_calls.load(Ordering::SeqCst), 1);
        assert_eq!(webhook.cleanup_calls.load(Ordering::SeqCst), 1);
        assert_eq!(audit.cleanup_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            lock.release_calls.load(Ordering::SeqCst),
            1,
            "lock must be released exactly once"
        );
    }

    /// R-retention-008：失败路径同样释放锁（release 恰一次），错误聚合含清理类别。
    #[tokio::test]
    async fn test_process_releases_lock_on_error_path() {
        let lock = Arc::new(MockRetentionLock::acquires());
        let worker = RetentionWorker::new(
            Arc::new(MockScrapeRepo::failing()),
            Some(Arc::new(MockGeoRepo::new(false))),
            Arc::new(MockWebhookRepo {
                cleanup_calls: AtomicU64::new(0),
            }),
            Arc::new(MockAuditService {
                cleanup_calls: AtomicU64::new(0),
            }),
            30,
            90,
            30,
            90,
            RetentionBatchPolicy::default(),
            300,
            lock.clone(),
        );

        let result = worker.process().await;
        assert!(
            matches!(result, ProcessResult::Error(_)),
            "got {:?}",
            result
        );
        assert_eq!(
            lock.release_calls.load(Ordering::SeqCst),
            1,
            "lock must be released on the error path too"
        );
    }

    /// R-retention-008（PG 集成）：两个并发 `try_acquire` 恰一个成功。
    #[tokio::test]
    async fn test_pg_retention_lock_is_exclusive() {
        use crate::common::test_helpers::create_test_db_pool;

        let lock_a = PgRetentionLock::new(create_test_db_pool());
        let lock_b = PgRetentionLock::new(create_test_db_pool());

        // 串行语义下先 A 后 B；A 持锁期间 B 必须拿不到
        let acquired_a = lock_a.try_acquire().await.expect("lock a acquire");
        assert!(acquired_a, "first acquirer must get the lock");

        let acquired_b = lock_b.try_acquire().await.expect("lock b acquire");
        assert!(!acquired_b, "second acquirer must be refused while a holds");

        lock_a.release().await.expect("lock a release");

        let acquired_b2 = lock_b.try_acquire().await.expect("lock b re-acquire");
        assert!(acquired_b2, "b must get the lock after a releases");
        lock_b.release().await.expect("lock b release");
    }
}
