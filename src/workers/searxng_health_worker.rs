// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SearXNG 实例健康探活周期 worker
//!
//! 每个周期调用 [`SearxngInstanceService::health_cycle`]：热池全体 + 冷却期满的
//! cooldown + 前 `candidate_batch` 个候选逐个探活，按状态机迁移并补齐热池。
//! 探活幂等——api 与 worker 双进程各自维护内存热池，DB 是共享真相。

use std::sync::Arc;

use async_trait::async_trait;
use log::{error, info};

use crate::domain::services::searxng::SearxngInstanceService;
use crate::workers::worker::{ProcessResult, WorkerProcess};

/// 健康探活 worker
pub struct SearxngHealthWorker {
    service: Arc<dyn SearxngInstanceService>,
}

impl SearxngHealthWorker {
    /// 构造探活 worker
    pub fn new(service: Arc<dyn SearxngInstanceService>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl WorkerProcess for SearxngHealthWorker {
    fn name(&self) -> &str {
        "searxng_health"
    }

    async fn process(&self) -> ProcessResult {
        let report = self.service.health_cycle().await;
        info!(
            "searxng health round finished probed={} promoted={} demoted={} retired={} errors={} health={:?}",
            report.probed,
            report.promoted,
            report.demoted,
            report.retired,
            report.errors,
            self.service.health()
        );
        if report.errors > 0 {
            // 逐实例失败已在服务内显性记录（last_error + 计数报告），整轮仍算完成
            error!(
                "searxng health round completed with {} probe failures",
                report.errors
            );
        }
        ProcessResult::Completed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repositories::searxng_instance_repository::SearxngInstanceState;
    use crate::domain::services::searxng::test_support::{
        shared_repo, test_settings, InMemorySearxngRepo, Scripted, ScriptedTransport,
    };
    use crate::domain::services::searxng::SearxngInstanceServiceImpl;

    #[tokio::test]
    async fn test_process_completes_and_promotes_candidates() {
        let repo = shared_repo();
        repo.insert_raw(InMemorySearxngRepo::dataset_instance(
            "https://searx.ok.example",
            SearxngInstanceState::Candidate,
        ));
        let transport = Arc::new(ScriptedTransport::repeat(
            Scripted::Json {
                status: 200,
                body: serde_json::json!({ "results": [] }),
            },
            5,
        ));
        let service = Arc::new(SearxngInstanceServiceImpl::new(
            repo.clone(),
            transport,
            test_settings(),
        ));
        let worker = SearxngHealthWorker::new(service.clone());
        assert_eq!(worker.name(), "searxng_health");

        assert_eq!(worker.process().await, ProcessResult::Completed);
        assert_eq!(
            repo.get("https://searx.ok.example").expect("row").state,
            SearxngInstanceState::Active
        );
        assert_eq!(service.pool_len(), 1);
    }

    #[tokio::test]
    async fn test_process_completes_even_when_all_probes_fail() {
        let repo = shared_repo();
        repo.insert_raw(InMemorySearxngRepo::dataset_instance(
            "https://searx.bad.example",
            SearxngInstanceState::Candidate,
        ));
        let transport = Arc::new(ScriptedTransport::repeat(
            Scripted::Json {
                status: 429,
                body: serde_json::Value::Null,
            },
            5,
        ));
        let service = Arc::new(SearxngInstanceServiceImpl::new(
            repo.clone(),
            transport,
            test_settings(),
        ));
        let worker = SearxngHealthWorker::new(service);

        assert_eq!(worker.process().await, ProcessResult::Completed);
        assert_eq!(
            repo.get("https://searx.bad.example").expect("row").state,
            SearxngInstanceState::Cooldown
        );
    }
}
