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
use log::{error, info};
use std::sync::Arc;

/// 数据保留期清理工作器
pub struct RetentionWorker {
    scrape_repo: Arc<dyn ScrapeResultRepository>,
    geo_repo: Arc<dyn GeoRestrictionRepository>,
    webhook_repo: Arc<dyn WebhookEventRepository>,
    audit_service: Arc<dyn AuditServiceTrait>,
    scrape_results_days: i64,
    geo_logs_days: i64,
    webhook_events_days: i64,
    audit_logs_days: i64,
    /// 有界删除参数（retention-worker-hardening R-retention-002）
    policy: RetentionBatchPolicy,
}

impl RetentionWorker {
    /// 构造清理工作器；天数参数由调用方从 `Settings.retention` 传入。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scrape_repo: Arc<dyn ScrapeResultRepository>,
        geo_repo: Arc<dyn GeoRestrictionRepository>,
        webhook_repo: Arc<dyn WebhookEventRepository>,
        audit_service: Arc<dyn AuditServiceTrait>,
        scrape_results_days: i64,
        geo_logs_days: i64,
        webhook_events_days: i64,
        audit_logs_days: i64,
        policy: RetentionBatchPolicy,
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
        }
    }

    /// 顺序执行四类清理；返回收集到的错误描述（类别名 + 错误信息）。
    async fn run_cleanup(&self) -> Vec<String> {
        let mut errors = Vec::new();

        match self
            .scrape_repo
            .cleanup_expired(self.scrape_results_days, &self.policy)
            .await
        {
            Ok(count) => {
                if count > 0 {
                    info!("Retention: cleaned up {} expired scrape_results", count);
                }
            }
            Err(e) => {
                error!("Retention: scrape_results cleanup failed: {}", e);
                errors.push(format!("scrape_results: {}", e));
            }
        }

        match self.geo_repo.cleanup_expired(self.geo_logs_days).await {
            Ok(count) => {
                if count > 0 {
                    info!(
                        "Retention: cleaned up {} expired geo_restriction_logs",
                        count
                    );
                }
            }
            Err(e) => {
                error!("Retention: geo_restriction_logs cleanup failed: {}", e);
                errors.push(format!("geo_restriction_logs: {}", e));
            }
        }

        match self
            .webhook_repo
            .cleanup_terminal(self.webhook_events_days)
            .await
        {
            Ok(count) => {
                if count > 0 {
                    info!("Retention: cleaned up {} terminal webhook_events", count);
                }
            }
            Err(e) => {
                error!("Retention: webhook_events cleanup failed: {}", e);
                errors.push(format!("webhook_events: {}", e));
            }
        }

        match self
            .audit_service
            .cleanup_old_logs(self.audit_logs_days)
            .await
        {
            Ok(count) => {
                if count > 0 {
                    info!("Retention: cleaned up {} old audit_logs", count);
                }
            }
            Err(e) => {
                error!("Retention: audit_logs cleanup failed: {}", e);
                errors.push(format!("audit_logs: {}", e));
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
        let errors = self.run_cleanup().await;
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
    }

    impl MockScrapeRepo {
        fn succeeding() -> Self {
            Self {
                cleanup_calls: AtomicU64::new(0),
                result: Mutex::new(None),
            }
        }
        fn failing() -> Self {
            Self {
                cleanup_calls: AtomicU64::new(0),
                result: Mutex::new(Some(Err(anyhow::anyhow!("db down")))),
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
        async fn cleanup_old_logs(&self, _retention_days: i64) -> Result<u64, AuditServiceError> {
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
            geo,
            webhook,
            audit,
            30,
            90,
            30,
            90,
            RetentionBatchPolicy::default(),
        )
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
}
