// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 请求合并协调器
//!
//! 从 [`crate::workers::scrape_worker::ScrapeWorker`] 抽取的请求合并协调逻辑，
//! 遵循 SRP：ScrapeWorker 专注任务调度，coalesce 协调由本类型独立负责。
//!
//! # 职责
//!
//! 对同 URL 并发请求执行 single-flight 协调：
//! - **Proceed 路径**：首个 worker（leader）获得执行权，正常抓取后通过 guard Drop
//!   广播 [`CoalesceSignal::Completed`]
//! - **Wait 路径**：等待方监听广播信号（45s 超时）：
//!   - `Completed` → 按 leader task_id 从 `result_repo` 读取结果，命中则 mark_completed，
//!     未命中则延后 5s 重排（leader 仍在写入）
//!   - `Purged`（僵死条目被清理）/ recv `Closed`（leader 异常）/ 超时 →
//!     不 mark_failed leader 任务，走等待方自身重排（超上限后才 mark_failed 自身）
//!
//! # 信号语义（R-engines-004）
//!
//! 原代码 `rx.recv().await` 无超时且不区分信号来源，leader panic / 死锁会导致等待方
//! 永久挂起或误判完成。现等待方区分 leader 正常完成与异常消失：正常完成才按
//! leader task_id 查询结果；异常（Purged/Closed/超时）一律走自身重排，避免误杀
//! leader 任务或无条件 purge 全表。

use crate::domain::models::{Task, TaskStatus};
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::utils::coalesce::{CoalesceGuard, CoalesceResult, CoalesceSignal, RequestCoalescer};
use anyhow::Result;
use chrono::Utc;
use log::{debug, error, info, warn};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

/// 请求合并协调器
///
/// 封装同 URL 并发请求的 single-flight 协调逻辑，从 ScrapeWorker 抽离。
///
/// # 字段
///
/// - `repository`：任务仓储（mark_completed / mark_failed / update）
/// - `result_repository`：抓取结果仓储（等待方从 result_repo 读取首个 worker 结果）
/// - `request_coalescer`：请求合并器（try_start / purge_stale）
///
/// # Clone 语义
///
/// 所有字段均为 `Arc`，`#[derive(Clone)]` 仅增加引用计数（O(1) 原子操作），
/// 不复制底层数据。测试中可在 spawn 的 task 中使用 clone。
#[derive(Clone)]
pub struct CoalesceCoordinator {
    /// 任务仓储
    repository: Arc<dyn TaskRepository>,
    /// 抓取结果仓储
    result_repository: Arc<dyn ScrapeResultRepository>,
    /// 请求合并器
    request_coalescer: Arc<RequestCoalescer>,
}

impl CoalesceCoordinator {
    /// 创建新的协调器
    ///
    /// # 参数
    ///
    /// - `repository`：任务仓储（实现 [`TaskRepository`] trait）
    /// - `result_repository`：抓取结果仓储（实现 [`ScrapeResultRepository`] trait）
    /// - `request_coalescer`：请求合并器（由 `WorkerManager` 从
    ///   `ServicesComponents.request_coalescer` 注入，所有 worker 共享同一实例）
    #[must_use]
    pub fn new(
        repository: Arc<dyn TaskRepository>,
        result_repository: Arc<dyn ScrapeResultRepository>,
        request_coalescer: Arc<RequestCoalescer>,
    ) -> Self {
        Self {
            repository,
            result_repository,
            request_coalescer,
        }
    }

    /// 尝试获取同 URL 的执行权
    ///
    /// # 返回值
    ///
    /// - `Ok(Some(guard))`：获得执行权，调用方应继续抓取，guard Drop 时广播
    /// - `Ok(None)`：已被其他 worker 处理（等待方从 result_repo 读到结果，或被延后重排），
    ///   调用方应直接返回 Ok(())
    /// - `Err(e)`：协调失败（超时 / 仓储错误），调用方应失败处理
    pub async fn try_coalesce(
        &self,
        url: &str,
        task: &Task,
        worker_id: Uuid,
    ) -> Result<Option<CoalesceGuard>> {
        match self.request_coalescer.try_start(url, task.id) {
            CoalesceResult::Proceed(g) => Ok(Some(g)),
            CoalesceResult::Wait(mut rx, leader_task_id) => {
                info!(
                    "URL {} already in-flight (leader task {}), task {} waiting for coalesce",
                    url, leader_task_id, task.id
                );
                // 等待首个 worker（leader）完成，加 timeout 防止活锁。
                //
                // 45s 选择依据：
                // - 大于浏览器引擎 MRT（30s）+ 缓冲，留足 leader 完成时间
                // - 短于 RequestCoalescer::STALE_TIMEOUT（120s），超时后由 purge_stale
                //   兜底清理 in-flight 条目并广播 Purged
                //
                // 信号语义（R-engines-004）：
                // - Completed → 按 leader task_id 查询落库结果
                // - Purged → leader 僵死被清理，不视为完成，走自身重排
                // - recv Closed / Lagged → leader 异常，不 mark_failed，走自身重排
                // - timeout → leader 仍可能在跑，不无条件 purge 全表，走自身重排
                const COALESCE_RECV_TIMEOUT_SECS: u64 = 45;

                let signal = match tokio::time::timeout(
                    std::time::Duration::from_secs(COALESCE_RECV_TIMEOUT_SECS),
                    rx.recv(),
                )
                .await
                {
                    Err(_) => {
                        // 超时：leader 未在窗口内完成，可能仍在跑或已 panic/死锁。
                        // R-engines-004：超时路径不得无条件 purge 全表，也不 mark_failed
                        // leader 的任务；走等待方自身重排（超上限后才 mark_failed 自身）。
                        error!(
                            "Coalesce wait for task {} timed out after {}s (leader task {}); \
                             rescheduling self instead of failing leader",
                            task.id, COALESCE_RECV_TIMEOUT_SECS, leader_task_id
                        );
                        return self
                            .reschedule_or_fail(task, worker_id, "coalesce wait timeout")
                            .await;
                    }
                    Ok(Ok(sig)) => sig,
                    Ok(Err(broadcast::error::RecvError::Closed)) => {
                        // recv Closed：leader 的 sender 全部 drop 却未广播完成
                        // （panic / 异常终止）。视为 leader 异常，不 mark_failed，走自身重排。
                        warn!(
                            "Coalesce channel closed for task {} (leader task {} likely panicked); \
                             rescheduling self",
                            task.id, leader_task_id
                        );
                        return self
                            .reschedule_or_fail(task, worker_id, "coalesce channel closed")
                            .await;
                    }
                    Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                        // Lagged：等待方消费过慢丢失信号（容量 1 理论不应发生）。保守重排。
                        warn!(
                            "Coalesce receiver for task {} lagged by {} messages; rescheduling self",
                            task.id, n
                        );
                        return self
                            .reschedule_or_fail(task, worker_id, "coalesce receiver lagged")
                            .await;
                    }
                };

                match signal {
                    CoalesceSignal::Purged => {
                        // purge_stale 清理僵死条目：不视为完成，不查询结果，走自身重排。
                        warn!(
                            "Coalesce entry for task {} purged as stale (leader task {}); \
                             rescheduling self without result lookup",
                            task.id, leader_task_id
                        );
                        self.reschedule_or_fail(task, worker_id, "coalesce entry purged")
                            .await
                    }
                    CoalesceSignal::Completed => {
                        // leader 正常完成：按 leader task_id 查询落库结果
                        // （结果落在 leader 名下，非等待方自身 task_id）。
                        match self.result_repository.find_by_task_id(leader_task_id).await {
                            Ok(Some(_)) => {
                                debug!(
                                    "Coalesced task {} resolved from leader {} result, marking completed",
                                    task.id, leader_task_id
                                );
                                self.repository
                                    .mark_completed(task.id, Some(worker_id))
                                    .await?;
                                Ok(None)
                            }
                            Ok(None) => {
                                // leader 已完成但结果尚未落库/不可见——延后重排
                                self.reschedule_or_fail(
                                    task,
                                    worker_id,
                                    "leader result not yet available",
                                )
                                .await
                            }
                            Err(e) => {
                                Err(anyhow::anyhow!("coalesce find_by_task_id failed: {}", e))
                            }
                        }
                    }
                }
            }
        }
    }

    /// 等待方自身重排（不 mark_failed leader 的任务）；超过最大重排次数后 mark_failed 自身。
    ///
    /// 触发场景：recv 超时 / Closed / Lagged、Purged 信号、Completed 但结果尚未落库。
    /// 统一走带锁守卫的 `update_task_guarded`（重排）或 `mark_failed`（超上限），
    /// 避免裸写 status/payload 覆盖并发状态迁移。
    async fn reschedule_or_fail(
        &self,
        task: &Task,
        worker_id: Uuid,
        reason: &str,
    ) -> Result<Option<CoalesceGuard>> {
        // 追踪 reschedule 次数，超过上限后 mark_failed 防止无限循环
        const MAX_COALESCE_RESCHEDULE: u32 = 3;
        let reschedule_count = task
            .payload
            .get("coalesce_reschedule_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        if reschedule_count >= MAX_COALESCE_RESCHEDULE {
            error!(
                "Coalesced task {} exceeded max reschedule count ({}) after [{}], marking failed",
                task.id, MAX_COALESCE_RESCHEDULE, reason
            );
            // 状态迁移统一走带锁守卫的 mark_failed；失败原因已在本条 error! 日志留存
            self.repository
                .mark_failed(task.id, Some(worker_id))
                .await?;
            return Ok(None);
        }

        warn!(
            "Coalesced task {} rescheduling after [{}] (attempt {}/{})",
            task.id,
            reason,
            reschedule_count + 1,
            MAX_COALESCE_RESCHEDULE
        );
        let mut updated = task.clone();
        updated.scheduled_at = Some(Utc::now() + chrono::Duration::seconds(5));
        updated.status = TaskStatus::Queued;
        if !updated.payload.is_object() {
            updated.payload = json!({});
        }
        if let Some(obj) = updated.payload.as_object_mut() {
            obj.insert(
                "coalesce_reschedule_count".to_string(),
                json!(reschedule_count + 1),
            );
        }
        // 守卫式写入：锁被抢占/任务被终结时放弃重排
        self.repository.update_task_guarded(&updated).await?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{Task, TaskType};
    use crate::domain::repositories::task_repository::{RepositoryError, TaskQueryParams};
    use crate::utils::coalesce::RequestCoalescer;
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;
    use uuid::Uuid;

    // === Mock Repositories ===

    struct CountingTaskRepo {
        update_calls: AtomicU32,
        mark_completed_calls: AtomicU32,
        mark_failed_calls: AtomicU32,
        captured_updates: Mutex<Vec<Task>>,
    }

    impl CountingTaskRepo {
        fn new() -> Self {
            Self {
                update_calls: AtomicU32::new(0),
                mark_completed_calls: AtomicU32::new(0),
                mark_failed_calls: AtomicU32::new(0),
                captured_updates: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl TaskRepository for CountingTaskRepo {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            self.update_calls.fetch_add(1, Ordering::SeqCst);
            self.captured_updates.lock().unwrap().push(task.clone());
            Ok(task.clone())
        }
        // 守卫式更新与 update 走同一计数/捕获（生产实现中两者互斥使用）
        async fn update_task_guarded(&self, task: &Task) -> Result<u64, RepositoryError> {
            self.update_calls.fetch_add(1, Ordering::SeqCst);
            self.captured_updates.lock().unwrap().push(task.clone());
            Ok(1)
        }
        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn mark_completed(
            &self,
            _id: Uuid,
            _lock_token: Option<Uuid>,
        ) -> Result<u64, RepositoryError> {
            self.mark_completed_calls.fetch_add(1, Ordering::SeqCst);
            Ok(1)
        }
        async fn mark_failed(
            &self,
            _id: Uuid,
            _lock_token: Option<Uuid>,
        ) -> Result<u64, RepositoryError> {
            self.mark_failed_calls.fetch_add(1, Ordering::SeqCst);
            Ok(1)
        }
        async fn mark_cancelled(&self, _id: Uuid) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }
        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            Ok(HashSet::new())
        }
        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(vec![])
        }
        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }
        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }

        async fn renew_lock(
            &self,
            _task_id: Uuid,
            _worker_id: Uuid,
            _extend_seconds: i64,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    struct ConfigurableResultRepo {
        find_calls: AtomicU32,
        return_some: Mutex<bool>,
        /// 记录最近一次 find_by_task_id 查询的 task_id，
        /// 用于断言等待方按 leader task_id（而非自身）查询。
        last_query_task_id: Mutex<Option<Uuid>>,
    }

    impl ConfigurableResultRepo {
        fn new(return_some: bool) -> Self {
            Self {
                find_calls: AtomicU32::new(0),
                return_some: Mutex::new(return_some),
                last_query_task_id: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl ScrapeResultRepository for ConfigurableResultRepo {
        async fn save(&self, _result: crate::domain::models::ScrapeResult) -> Result<()> {
            Ok(())
        }
        async fn find_by_task_id(
            &self,
            task_id: Uuid,
        ) -> Result<Option<crate::domain::models::ScrapeResult>> {
            self.find_calls.fetch_add(1, Ordering::SeqCst);
            *self.last_query_task_id.lock().unwrap() = Some(task_id);
            if *self.return_some.lock().unwrap() {
                Ok(Some(crate::domain::models::ScrapeResult {
                    id: uuid::Uuid::new_v4(),
                    task_id: uuid::Uuid::new_v4(),
                    url: String::new(),
                    status_code: 200,
                    content: String::new(),
                    content_type: "text/html".to_string(),
                    headers: serde_json::json!({}),
                    meta_data: serde_json::json!({}),
                    screenshot: None,
                    response_time_ms: 0,
                    created_at: Utc::now(),
                }))
            } else {
                Ok(None)
            }
        }
        async fn find_by_task_ids(
            &self,
            _task_ids: &[Uuid],
        ) -> Result<Vec<crate::domain::models::ScrapeResult>> {
            Ok(vec![])
        }
        async fn get_team_avg_response_time(&self, _team_id: Uuid) -> Result<f64> {
            Ok(0.0)
        }

        async fn cleanup_expired(&self, _retention_days: i64) -> anyhow::Result<u64> {
            Ok(0)
        }
    }

    fn make_task() -> Task {
        Task::new(
            uuid::Uuid::new_v4(),
            TaskType::Scrape,
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            "https://example.com".to_string(),
            json!({}),
        )
    }

    /// Proceed 路径：首个 worker 获得执行权，返回 Some(guard)
    #[tokio::test]
    async fn try_coalesce_proceed_returns_some_guard() {
        let worker_id = Uuid::new_v4();
        let task_repo = Arc::new(CountingTaskRepo::new());
        let result_repo = Arc::new(ConfigurableResultRepo::new(false));
        let coalescer = Arc::new(RequestCoalescer::new());
        let coord =
            CoalesceCoordinator::new(task_repo.clone(), result_repo.clone(), coalescer.clone());

        let task = make_task();
        let result = coord.try_coalesce(&task.url, &task, worker_id).await;
        assert!(result.is_ok(), "proceed path should succeed");
        let guard = result.unwrap();
        assert!(guard.is_some(), "first call should return Some(guard)");
        // 不应触发仓储调用
        assert_eq!(task_repo.update_calls.load(Ordering::SeqCst), 0);
        assert_eq!(task_repo.mark_completed_calls.load(Ordering::SeqCst), 0);
        assert_eq!(result_repo.find_calls.load(Ordering::SeqCst), 0);
    }

    /// Wait 路径收到广播但 result_repo 未命中：延后 5s 重排
    #[tokio::test]
    async fn try_coalesce_wait_reschedules_when_result_missing() {
        let worker_id = Uuid::new_v4();
        let task_repo = Arc::new(CountingTaskRepo::new());
        let result_repo = Arc::new(ConfigurableResultRepo::new(false)); // 未命中
        let coalescer = Arc::new(RequestCoalescer::new());
        let coord =
            CoalesceCoordinator::new(task_repo.clone(), result_repo.clone(), coalescer.clone());

        let task = make_task();
        let url = task.url.clone();

        // 占住 slot
        let first_guard = coalescer.try_start(&url, Uuid::new_v4());
        // 等待 guard 进入 Wait 路径
        let task_clone = task.clone();
        let coord_clone = coord.clone();
        let url_clone = url.clone();
        let handle = tokio::spawn(async move {
            coord_clone
                .try_coalesce(&url_clone, &task_clone, worker_id)
                .await
        });

        // 给一点时间让等待方进入 rx.recv()
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // 释放 guard 触发广播
        drop(first_guard);

        let result = handle.await.expect("task panicked");
        assert!(result.is_ok(), "should not error");
        let guard = result.unwrap();
        assert!(guard.is_none(), "should return None (rescheduled)");

        // 验证 update 被调用（reschedule 路径），状态为 Queued
        assert_eq!(
            task_repo.update_calls.load(Ordering::SeqCst),
            1,
            "update should be called once for reschedule"
        );
        let captured = task_repo.captured_updates.lock().unwrap();
        let updated = captured.last().expect("captured update");
        assert_eq!(updated.status, TaskStatus::Queued);
        assert!(
            updated.scheduled_at.is_some(),
            "scheduled_at should be set for reschedule"
        );

        // 不应触发 mark_completed
        assert_eq!(
            task_repo.mark_completed_calls.load(Ordering::SeqCst),
            0,
            "mark_completed should not be called when result missing"
        );
    }

    /// Wait 路径收到广播且 result_repo 命中：mark_completed 并返回 None
    #[tokio::test]
    async fn try_coalesce_wait_completes_when_result_found() {
        let worker_id = Uuid::new_v4();
        let task_repo = Arc::new(CountingTaskRepo::new());
        let result_repo = Arc::new(ConfigurableResultRepo::new(true)); // 命中
        let coalescer = Arc::new(RequestCoalescer::new());
        let coord =
            CoalesceCoordinator::new(task_repo.clone(), result_repo.clone(), coalescer.clone());

        let task = make_task();
        let url = task.url.clone();

        // 占住 slot
        let first_guard = coalescer.try_start(&url, Uuid::new_v4());
        let task_clone = task.clone();
        let coord_clone = coord.clone();
        let url_clone = url.clone();
        let handle = tokio::spawn(async move {
            coord_clone
                .try_coalesce(&url_clone, &task_clone, worker_id)
                .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drop(first_guard);

        let result = handle.await.expect("task panicked");
        assert!(result.is_ok(), "should not error");
        let guard = result.unwrap();
        assert!(guard.is_none(), "should return None (completed)");

        // mark_completed 应被调用
        assert_eq!(
            task_repo.mark_completed_calls.load(Ordering::SeqCst),
            1,
            "mark_completed should be called when result found"
        );
        // 不应调用 update（无 reschedule）
        assert_eq!(
            task_repo.update_calls.load(Ordering::SeqCst),
            0,
            "update should not be called when result found"
        );
    }

    /// Wait 路径未触发广播时不应调用 find_by_task_id（仍在 rx.recv() 等待中）
    ///
    /// 这覆盖超时路径的前置条件：在信号到达前，等待方不会读 result_repo。
    /// 完整 45s 超时测试因耗时不实际，超时后的自身重排语义由 Closed/Purged 用例覆盖。
    #[tokio::test]
    async fn try_coalesce_wait_does_not_call_find_while_waiting() {
        let worker_id = Uuid::new_v4();
        let task_repo = Arc::new(CountingTaskRepo::new());
        let result_repo = Arc::new(ConfigurableResultRepo::new(false));
        let coalescer = Arc::new(RequestCoalescer::new());
        let coord =
            CoalesceCoordinator::new(task_repo.clone(), result_repo.clone(), coalescer.clone());

        let task = make_task();
        let url = task.url.clone();

        // 占住 slot 但不释放（模拟首个 worker 长时间处理）
        let _first_guard = coalescer.try_start(&url, Uuid::new_v4());

        let task_clone = task.clone();
        let coord_clone = coord.clone();
        let url_clone = url.clone();
        let handle = tokio::spawn(async move {
            coord_clone
                .try_coalesce(&url_clone, &task_clone, worker_id)
                .await
        });

        // 短暂等待，验证仍在等待中
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        // find_by_task_id 不应被调用（仍在 recv 等待中）
        assert_eq!(
            result_repo.find_calls.load(Ordering::SeqCst),
            0,
            "find_by_task_id should not be called while waiting for broadcast"
        );
        // 取消避免 45s 阻塞
        handle.abort();
    }

    /// 最大重排次数路径：coalesce_reschedule_count >= 3 时 mark_failed
    #[tokio::test]
    async fn try_coalesce_max_reschedule_exceeded_marks_failed() {
        let worker_id = Uuid::new_v4();
        let task_repo = Arc::new(CountingTaskRepo::new());
        let result_repo = Arc::new(ConfigurableResultRepo::new(false));
        let coalescer = Arc::new(RequestCoalescer::new());
        let coord =
            CoalesceCoordinator::new(task_repo.clone(), result_repo.clone(), coalescer.clone());

        // 构造已达最大重排次数的任务
        let mut task = make_task();
        task.payload = json!({"coalesce_reschedule_count": 3});
        let url = task.url.clone();

        // 占住 slot
        let first_guard = coalescer.try_start(&url, Uuid::new_v4());
        let task_clone = task.clone();
        let coord_clone = coord.clone();
        let url_clone = url.clone();
        let handle = tokio::spawn(async move {
            coord_clone
                .try_coalesce(&url_clone, &task_clone, worker_id)
                .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drop(first_guard);

        let result = handle.await.expect("task panicked");
        assert!(result.is_ok());
        let guard = result.unwrap();
        assert!(
            guard.is_none(),
            "max reschedule exceeded should return None"
        );

        // 超上限路径应走带锁守卫的 mark_failed（不再先裸写 status/payload）
        assert_eq!(
            task_repo.mark_failed_calls.load(Ordering::SeqCst),
            1,
            "mark_failed should be called when max reschedule exceeded"
        );
        assert_eq!(
            task_repo.update_calls.load(Ordering::SeqCst),
            0,
            "bare update should not be called on the max-reschedule path"
        );
    }

    /// find_by_task_id 返回 Err 时应传播错误
    #[tokio::test]
    async fn try_coalesce_wait_find_by_task_id_error_propagates() {
        let worker_id = Uuid::new_v4();
        let task_repo = Arc::new(CountingTaskRepo::new());
        let coalescer = Arc::new(RequestCoalescer::new());

        // ErrorResultRepo: find_by_task_id returns Err
        struct ErrorResultRepo;
        #[async_trait]
        impl ScrapeResultRepository for ErrorResultRepo {
            async fn save(&self, _result: crate::domain::models::ScrapeResult) -> Result<()> {
                Ok(())
            }
            async fn find_by_task_id(
                &self,
                _task_id: Uuid,
            ) -> Result<Option<crate::domain::models::ScrapeResult>> {
                Err(anyhow::anyhow!("db connection lost"))
            }
            async fn find_by_task_ids(
                &self,
                _task_ids: &[Uuid],
            ) -> Result<Vec<crate::domain::models::ScrapeResult>> {
                Ok(vec![])
            }
            async fn get_team_avg_response_time(&self, _team_id: Uuid) -> Result<f64> {
                Ok(0.0)
            }

            async fn cleanup_expired(&self, _retention_days: i64) -> anyhow::Result<u64> {
                Ok(0)
            }
        }

        let result_repo = Arc::new(ErrorResultRepo);
        let coord =
            CoalesceCoordinator::new(task_repo.clone(), result_repo.clone(), coalescer.clone());

        let task = make_task();
        let url = task.url.clone();

        let first_guard = coalescer.try_start(&url, Uuid::new_v4());
        let task_clone = task.clone();
        let coord_clone = coord.clone();
        let url_clone = url.clone();
        let handle = tokio::spawn(async move {
            coord_clone
                .try_coalesce(&url_clone, &task_clone, worker_id)
                .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drop(first_guard);

        let result = handle.await.expect("task panicked");
        assert!(result.is_err(), "find_by_task_id error should propagate");
    }

    /// new() 构造器验证字段正确初始化
    #[test]
    fn test_coalesce_coordinator_new() {
        let task_repo = Arc::new(CountingTaskRepo::new());
        let result_repo = Arc::new(ConfigurableResultRepo::new(false));
        let coalescer = Arc::new(RequestCoalescer::new());
        let coord =
            CoalesceCoordinator::new(task_repo.clone(), result_repo.clone(), coalescer.clone());
        // Clone should work (Arc-based)
        let _cloned = coord.clone();
    }

    /// R-engines-004：recv Closed（leader 异常消失）→ 等待方不进入 Failed，走自身重排
    #[tokio::test]
    async fn try_coalesce_closed_channel_reschedules_not_failed() {
        let worker_id = Uuid::new_v4();
        let task_repo = Arc::new(CountingTaskRepo::new());
        // return_some=true：若等待方误走完成路径查询会命中——反证 Closed 分支不查结果
        let result_repo = Arc::new(ConfigurableResultRepo::new(true));
        let coalescer = Arc::new(RequestCoalescer::new());
        let coord =
            CoalesceCoordinator::new(task_repo.clone(), result_repo.clone(), coalescer.clone());

        let task = make_task();
        let url = task.url.clone();

        // leader 占位；_leader_guard 保活到作用域末，避免其提前 Drop 广播 Completed
        let _leader_guard = coalescer.try_start(&url, Uuid::new_v4());

        let task_clone = task.clone();
        let coord_clone = coord.clone();
        let url_clone = url.clone();
        let handle = tokio::spawn(async move {
            coord_clone
                .try_coalesce(&url_clone, &task_clone, worker_id)
                .await
        });

        // 等待方进入 recv 后，静默移除条目 → sender drop → recv 返回 Closed
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        coalescer.drop_entry_silently(&url);

        let result = handle.await.expect("task panicked");
        assert!(result.is_ok(), "closed channel should not error");
        assert!(
            result.unwrap().is_none(),
            "closed channel should reschedule self (None)"
        );

        // 关键断言：leader 异常不 mark_failed 等待方任务，而是走自身重排
        assert_eq!(
            task_repo.mark_failed_calls.load(Ordering::SeqCst),
            0,
            "closed channel must NOT mark waiter task failed"
        );
        assert_eq!(
            task_repo.update_calls.load(Ordering::SeqCst),
            1,
            "closed channel should reschedule self via guarded update"
        );
        assert_eq!(
            result_repo.find_calls.load(Ordering::SeqCst),
            0,
            "closed channel must not query result_repo"
        );
    }

    /// R-engines-004：Purged 信号不触发结果查询/完成路径，走自身重排
    #[tokio::test]
    async fn try_coalesce_purged_signal_skips_result_lookup() {
        let worker_id = Uuid::new_v4();
        let task_repo = Arc::new(CountingTaskRepo::new());
        // return_some=true：若等待方误按完成路径查询会命中并 mark_completed——
        // 用以反证 Purged 分支确实跳过了结果查询与完成路径。
        let result_repo = Arc::new(ConfigurableResultRepo::new(true));
        let coalescer = Arc::new(RequestCoalescer::new());
        let coord =
            CoalesceCoordinator::new(task_repo.clone(), result_repo.clone(), coalescer.clone());

        let task = make_task();
        let url = task.url.clone();

        let _leader_guard = coalescer.try_start(&url, Uuid::new_v4());

        let task_clone = task.clone();
        let coord_clone = coord.clone();
        let url_clone = url.clone();
        let handle = tokio::spawn(async move {
            coord_clone
                .try_coalesce(&url_clone, &task_clone, worker_id)
                .await
        });

        // 等待方进入 recv 后，强制 purge 该条目 → 广播 Purged
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        coalescer.purge_url(&url);

        let result = handle.await.expect("task panicked");
        assert!(result.is_ok());
        assert!(
            result.unwrap().is_none(),
            "purged signal should reschedule self (None)"
        );

        // Purged 分支不视为完成：不查询结果、不 mark_completed、不 mark_failed
        assert_eq!(
            result_repo.find_calls.load(Ordering::SeqCst),
            0,
            "Purged must NOT trigger result lookup"
        );
        assert_eq!(
            task_repo.mark_completed_calls.load(Ordering::SeqCst),
            0,
            "Purged must NOT mark completed"
        );
        assert_eq!(
            task_repo.mark_failed_calls.load(Ordering::SeqCst),
            0,
            "Purged must NOT mark waiter failed"
        );
        assert_eq!(
            task_repo.update_calls.load(Ordering::SeqCst),
            1,
            "Purged should reschedule self via guarded update"
        );
    }

    /// R-engines-004：Completed 后等待方按 leader task_id 查询结果（非自身 task_id）
    #[tokio::test]
    async fn try_coalesce_queries_result_by_leader_task_id() {
        let worker_id = Uuid::new_v4();
        let task_repo = Arc::new(CountingTaskRepo::new());
        let result_repo = Arc::new(ConfigurableResultRepo::new(true));
        let coalescer = Arc::new(RequestCoalescer::new());
        let coord =
            CoalesceCoordinator::new(task_repo.clone(), result_repo.clone(), coalescer.clone());

        // leader 用已知 task_id 占位；等待方 task.id 与 leader_id 不同
        let leader_id = Uuid::new_v4();
        let waiter_task = make_task();
        assert_ne!(
            leader_id, waiter_task.id,
            "test precondition: leader and waiter task ids differ"
        );
        let url = waiter_task.url.clone();

        let leader_guard = coalescer.try_start(&url, leader_id);

        let task_clone = waiter_task.clone();
        let coord_clone = coord.clone();
        let url_clone = url.clone();
        let handle = tokio::spawn(async move {
            coord_clone
                .try_coalesce(&url_clone, &task_clone, worker_id)
                .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // leader 正常完成 → Drop CoalesceResult::Proceed(guard) → 广播 Completed
        drop(leader_guard);

        let result = handle.await.expect("task panicked");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        // 关键断言：结果查询使用 leader task_id，而非等待方自身 task.id
        let queried = *result_repo.last_query_task_id.lock().unwrap();
        assert_eq!(
            queried,
            Some(leader_id),
            "waiter must query result by leader task_id"
        );
        assert_ne!(queried, Some(waiter_task.id));
        // 命中后 mark_completed 等待方自身任务
        assert_eq!(task_repo.mark_completed_calls.load(Ordering::SeqCst), 1);
    }
}
