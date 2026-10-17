// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SearXNG 实例采集周期 worker
//!
//! 每个周期调用 [`SearxngInstanceService::collect`]：拉取 searx.space 数据集、
//! 准入、幂等 upsert、缺席退役。api 与 worker 双进程都会拉起本 worker，
//! 采集轮内的 PG advisory lock 保证同一时刻只执行一个采集体。

use std::sync::Arc;

use async_trait::async_trait;
use log::{debug, error, info};

use crate::domain::services::searxng::SearxngInstanceService;
use crate::workers::worker::{ProcessResult, WorkerProcess};

/// 采集 worker
pub struct SearxngCollectorWorker {
    service: Arc<dyn SearxngInstanceService>,
}

impl SearxngCollectorWorker {
    /// 构造采集 worker
    pub fn new(service: Arc<dyn SearxngInstanceService>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl WorkerProcess for SearxngCollectorWorker {
    fn name(&self) -> &str {
        "searxng_collector"
    }

    async fn process(&self) -> ProcessResult {
        match self.service.collect().await {
            Ok(report) if report.skipped => {
                debug!(
                    "searxng collector skipped this round (advisory lock held elsewhere), pool={:?}",
                    self.service.health()
                );
                ProcessResult::Empty
            }
            Ok(report) => {
                info!(
                    "searxng collector round finished fetched={} seen={} admitted_new={} updated={} retired={} errors={}",
                    report.fetched,
                    report.seen,
                    report.admitted_new,
                    report.updated,
                    report.retired,
                    report.errors
                );
                ProcessResult::Completed
            }
            Err(e) => {
                error!("searxng collector round failed: {e}");
                ProcessResult::Error(e.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repositories::searxng_instance_repository::SearxngInstanceRepository;
    use crate::domain::services::searxng::test_support::{
        shared_repo, test_settings, InMemorySearxngRepo, Scripted, ScriptedTransport,
    };
    use crate::domain::services::searxng::SearxngInstanceServiceImpl;

    fn worker(
        repo: Arc<InMemorySearxngRepo>,
        transport: Arc<ScriptedTransport>,
    ) -> SearxngCollectorWorker {
        SearxngCollectorWorker::new(Arc::new(SearxngInstanceServiceImpl::new(
            repo,
            transport,
            test_settings(),
        )))
    }

    #[tokio::test]
    async fn test_process_returns_completed_on_successful_collect() {
        let transport = Arc::new(ScriptedTransport::new(vec![Scripted::Json {
            status: 200,
            body: serde_json::json!({
                "instances": {
                    "https://searx.ok.example": { "network_type": "normal", "api": true, "uptime": { "day": 99.0 } }
                }
            }),
        }]));
        let worker = worker(shared_repo(), transport);
        assert_eq!(worker.name(), "searxng_collector");
        assert_eq!(worker.process().await, ProcessResult::Completed);
    }

    #[tokio::test]
    async fn test_process_returns_error_with_reason_on_failure() {
        let transport = Arc::new(ScriptedTransport::new(vec![Scripted::Network(
            "connection refused".to_string(),
        )]));
        let worker = worker(shared_repo(), transport);
        match worker.process().await {
            ProcessResult::Error(reason) => {
                assert!(reason.contains("connection refused"), "got: {reason}")
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_process_returns_empty_when_round_is_skipped() {
        let repo = shared_repo();
        let guard = repo
            .try_acquire_collect_lock(0)
            .await
            .expect("lock attempt")
            .expect("lock is free");
        let worker = worker(repo.clone(), Arc::new(ScriptedTransport::new(vec![])));
        assert_eq!(worker.process().await, ProcessResult::Empty);
        guard.release().await.expect("release");
    }
}
