// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::workers::worker::{ProcessResult, WorkerProcess};
use async_trait::async_trait;
use log::info;
use std::sync::Arc;

/// 任务过期清理工作器
///
/// 负责定期扫描并清理过期的任务，以及超过保留期的抓取结果
pub struct ExpirationWorker {
    repository: Arc<dyn TaskRepository>,
    result_repository: Arc<dyn ScrapeResultRepository>,
    /// scrape_results 保留天数；<= 0 表示禁用结果清理
    result_retention_days: i64,
}

impl ExpirationWorker {
    pub fn new(
        repository: Arc<dyn TaskRepository>,
        result_repository: Arc<dyn ScrapeResultRepository>,
        result_retention_days: i64,
    ) -> Self {
        Self {
            repository,
            result_repository,
            result_retention_days,
        }
    }

    async fn cleanup_expired_tasks(&self) -> Result<u64, String> {
        self.repository
            .expire_tasks()
            .await
            .map_err(|e| e.to_string())
    }

    /// 清理超过保留期的抓取结果
    ///
    /// `result_retention_days <= 0` 时为禁用状态，直接返回 0 且不触碰仓库
    async fn cleanup_expired_results(&self) -> Result<u64, String> {
        if self.result_retention_days <= 0 {
            return Ok(0);
        }
        self.result_repository
            .cleanup_expired(self.result_retention_days)
            .await
            .map_err(|e| format!("Failed to cleanup expired scrape results: {}", e))
    }
}

#[async_trait]
impl WorkerProcess for ExpirationWorker {
    fn name(&self) -> &str {
        "expiration-worker"
    }

    async fn process(&self) -> ProcessResult {
        match self.cleanup_expired_tasks().await {
            Ok(count) => {
                if count > 0 {
                    info!("Cleaned up {} expired tasks", count);
                }
                match self.cleanup_expired_results().await {
                    Ok(cleaned) => {
                        if cleaned > 0 {
                            info!(
                                "Cleaned up {} expired scrape results (retention={} days)",
                                cleaned, self.result_retention_days
                            );
                        }
                        ProcessResult::Completed
                    }
                    Err(e) => ProcessResult::Error(e),
                }
            }
            Err(e) => ProcessResult::Error(format!("Failed to cleanup expired tasks: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{ScrapeResult, Task};
    use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
    use crate::domain::repositories::task_repository::{RepositoryError, TaskQueryParams};
    use async_trait::async_trait;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use uuid::Uuid;

    /// Mock TaskRepository that returns configurable results for expire_tasks
    struct MockTaskRepository {
        /// Number of tasks to report as expired (Ok case)
        expired_count: AtomicU64,
        /// Optional error to return instead of Ok
        error: Mutex<Option<String>>,
        /// Number of times expire_tasks was called
        expire_call_count: AtomicU64,
    }

    impl MockTaskRepository {
        fn new_with_expired_count(count: u64) -> Self {
            Self {
                expired_count: AtomicU64::new(count),
                error: Mutex::new(None),
                expire_call_count: AtomicU64::new(0),
            }
        }

        fn new_with_error(msg: &str) -> Self {
            Self {
                expired_count: AtomicU64::new(0),
                error: Mutex::new(Some(msg.to_string())),
                expire_call_count: AtomicU64::new(0),
            }
        }

        fn expire_calls(&self) -> u64 {
            self.expire_call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, _task: &Task) -> Result<Task, RepositoryError> {
            Ok(_task.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }

        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn mark_completed(
            &self,
            _id: Uuid,
            _lock_token: Option<Uuid>,
        ) -> Result<u64, RepositoryError> {
            Ok(1)
        }

        async fn mark_failed(
            &self,
            _id: Uuid,
            _lock_token: Option<Uuid>,
        ) -> Result<u64, RepositoryError> {
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
            self.expire_call_count.fetch_add(1, Ordering::SeqCst);
            if let Some(msg) = self.error.lock().unwrap().take() {
                return Err(RepositoryError::Database(anyhow::anyhow!(msg)));
            }
            Ok(self.expired_count.load(Ordering::SeqCst))
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

    /// Mock ScrapeResultRepository，记录 cleanup_expired 的调用与可配置行为
    struct MockResultRepository {
        cleaned_rows: u64,
        /// Optional error to return from cleanup_expired
        cleanup_error: Mutex<Option<String>>,
        cleanup_call_count: AtomicU64,
        /// Records the retention_days passed to cleanup_expired
        last_retention_days: Mutex<Option<i64>>,
    }

    impl MockResultRepository {
        fn new(cleaned_rows: u64) -> Self {
            Self {
                cleaned_rows,
                cleanup_error: Mutex::new(None),
                cleanup_call_count: AtomicU64::new(0),
                last_retention_days: Mutex::new(None),
            }
        }

        fn new_with_cleanup_error(msg: &str) -> Self {
            Self {
                cleaned_rows: 0,
                cleanup_error: Mutex::new(Some(msg.to_string())),
                cleanup_call_count: AtomicU64::new(0),
                last_retention_days: Mutex::new(None),
            }
        }

        fn cleanup_calls(&self) -> u64 {
            self.cleanup_call_count.load(Ordering::SeqCst)
        }

        fn last_retention(&self) -> Option<i64> {
            *self.last_retention_days.lock().unwrap()
        }
    }

    #[async_trait]
    impl ScrapeResultRepository for MockResultRepository {
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

        async fn cleanup_expired(&self, retention_days: i64) -> anyhow::Result<u64> {
            self.cleanup_call_count.fetch_add(1, Ordering::SeqCst);
            *self.last_retention_days.lock().unwrap() = Some(retention_days);
            if let Some(msg) = self.cleanup_error.lock().unwrap().take() {
                return Err(anyhow::anyhow!(msg));
            }
            Ok(self.cleaned_rows)
        }
    }

    /// 构造使用给定任务仓库、默认结果仓库（30 天保留）的 worker
    fn make_worker(task_repo: Arc<MockTaskRepository>) -> ExpirationWorker {
        ExpirationWorker::new(
            task_repo as Arc<dyn TaskRepository>,
            Arc::new(MockResultRepository::new(0)),
            30,
        )
    }

    #[test]
    fn test_worker_name() {
        let repo = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let worker = make_worker(repo);
        assert_eq!(worker.name(), "expiration-worker");
    }

    #[tokio::test]
    async fn test_process_completes_with_zero_expired() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let worker = make_worker(mock.clone());
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        assert_eq!(mock.expire_calls(), 1);
    }

    #[tokio::test]
    async fn test_process_completes_with_some_expired() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(5));
        let worker = make_worker(mock.clone());
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        assert_eq!(mock.expire_calls(), 1);
    }

    #[tokio::test]
    async fn test_process_returns_error_on_repo_failure() {
        let mock = Arc::new(MockTaskRepository::new_with_error("db connection lost"));
        let worker = make_worker(mock.clone());
        let result = worker.process().await;
        match result {
            ProcessResult::Error(msg) => {
                assert!(msg.contains("Failed to cleanup expired tasks"));
                assert!(msg.contains("db connection lost"));
            }
            _ => panic!("Expected ProcessResult::Error, got {:?}", result),
        }
        assert_eq!(mock.expire_calls(), 1);
    }

    #[tokio::test]
    async fn test_cleanup_expired_tasks_returns_count() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(42));
        let worker = make_worker(mock.clone());
        let result = worker.cleanup_expired_tasks().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(mock.expire_calls(), 1);
    }

    #[tokio::test]
    async fn test_cleanup_expired_tasks_returns_error_string() {
        let mock = Arc::new(MockTaskRepository::new_with_error("timeout"));
        let worker = make_worker(mock.clone());
        let result = worker.cleanup_expired_tasks().await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("timeout"));
    }

    #[tokio::test]
    async fn test_process_calls_expire_tasks_exactly_once() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let worker = make_worker(mock.clone());
        let _ = worker.process().await;
        assert_eq!(mock.expire_calls(), 1);
    }

    #[tokio::test]
    async fn test_process_multiple_cycles() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(3));
        let worker = make_worker(mock.clone());
        // Run multiple cycles - but note the mock only returns Ok on first call
        // because error is Mutex<Option> and gets taken. Let's test a single cycle
        // properly completes.
        let result1 = worker.process().await;
        assert_eq!(result1, ProcessResult::Completed);
        assert_eq!(mock.expire_calls(), 1);
    }

    // ========== process() with count = 1 (minimum non-zero) ==========

    #[tokio::test]
    async fn test_process_with_count_one() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(1));
        let worker = make_worker(mock.clone());
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        assert_eq!(mock.expire_calls(), 1);
    }

    // ========== process() with large count ==========

    #[tokio::test]
    async fn test_process_with_large_count() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(u64::MAX));
        let worker = make_worker(mock.clone());
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        assert_eq!(mock.expire_calls(), 1);
    }

    // ========== process() error then success on second call ==========

    #[tokio::test]
    async fn test_process_error_then_success_on_retry() {
        let mock = Arc::new(MockTaskRepository::new_with_error("first call fails"));
        let worker = make_worker(mock.clone());
        // First call: error is taken from the mock, returns Error
        let result1 = worker.process().await;
        match result1 {
            ProcessResult::Error(msg) => {
                assert!(msg.contains("first call fails"));
            }
            _ => panic!("Expected Error on first call, got {:?}", result1),
        }
        assert_eq!(mock.expire_calls(), 1);
        // Second call: error has been consumed, returns Ok(0)
        let result2 = worker.process().await;
        assert_eq!(result2, ProcessResult::Completed);
        assert_eq!(mock.expire_calls(), 2);
    }

    // ========== multiple process cycles all complete ==========

    #[tokio::test]
    async fn test_process_multiple_cycles_all_complete() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(5));
        let worker = make_worker(mock.clone());
        // The mock returns Ok(5) on every call (error is None)
        for i in 1..=3 {
            let result = worker.process().await;
            assert_eq!(result, ProcessResult::Completed, "cycle {} failed", i);
        }
        assert_eq!(mock.expire_calls(), 3);
    }

    // ========== cleanup_expired_tasks with zero count ==========

    #[tokio::test]
    async fn test_cleanup_expired_tasks_zero_count() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let worker = make_worker(mock.clone());
        let result = worker.cleanup_expired_tasks().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
        assert_eq!(mock.expire_calls(), 1);
    }

    // ========== new() returns worker with correct name ==========

    #[test]
    fn test_new_returns_worker_with_correct_name() {
        let repo = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let worker = make_worker(repo);
        // Verify the worker was constructed correctly by checking its name
        assert_eq!(worker.name(), "expiration-worker");
    }

    // ========== process() preserves repository across calls ==========

    #[tokio::test]
    async fn test_process_preserves_repository_across_calls() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(2));
        let worker = make_worker(mock.clone());
        // First call
        let r1 = worker.process().await;
        assert_eq!(r1, ProcessResult::Completed);
        // Second call uses the same repository
        let r2 = worker.process().await;
        assert_eq!(r2, ProcessResult::Completed);
        // Both calls should have invoked expire_tasks
        assert_eq!(mock.expire_calls(), 2);
    }

    // ========== scrape result retention ==========

    #[tokio::test]
    async fn test_process_cleans_expired_results_and_completes() {
        let task_mock = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let result_mock = Arc::new(MockResultRepository::new(7));
        let worker = ExpirationWorker::new(
            task_mock.clone() as Arc<dyn TaskRepository>,
            result_mock.clone() as Arc<dyn ScrapeResultRepository>,
            30,
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        assert_eq!(task_mock.expire_calls(), 1);
        assert_eq!(result_mock.cleanup_calls(), 1);
        assert_eq!(result_mock.last_retention(), Some(30));
    }

    #[tokio::test]
    async fn test_retention_disabled_skips_result_cleanup() {
        let task_mock = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let result_mock = Arc::new(MockResultRepository::new(0));
        let worker = ExpirationWorker::new(
            task_mock.clone() as Arc<dyn TaskRepository>,
            result_mock.clone() as Arc<dyn ScrapeResultRepository>,
            0,
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        // 禁用状态下不应触碰结果仓库
        assert_eq!(result_mock.cleanup_calls(), 0);
    }

    #[tokio::test]
    async fn test_negative_retention_skips_result_cleanup() {
        let task_mock = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let result_mock = Arc::new(MockResultRepository::new(0));
        let worker = ExpirationWorker::new(
            task_mock.clone() as Arc<dyn TaskRepository>,
            result_mock.clone() as Arc<dyn ScrapeResultRepository>,
            -1,
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        assert_eq!(result_mock.cleanup_calls(), 0);
    }

    #[tokio::test]
    async fn test_process_returns_error_on_result_cleanup_failure() {
        let task_mock = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let result_mock = Arc::new(MockResultRepository::new_with_cleanup_error("deadlock"));
        let worker = ExpirationWorker::new(
            task_mock.clone() as Arc<dyn TaskRepository>,
            result_mock.clone() as Arc<dyn ScrapeResultRepository>,
            30,
        );
        let result = worker.process().await;
        match result {
            ProcessResult::Error(msg) => {
                assert!(msg.contains("Failed to cleanup expired scrape results"));
                assert!(msg.contains("deadlock"));
            }
            _ => panic!("Expected ProcessResult::Error, got {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_task_failure_short_circuits_result_cleanup() {
        let task_mock = Arc::new(MockTaskRepository::new_with_error("db down"));
        let result_mock = Arc::new(MockResultRepository::new(0));
        let worker = ExpirationWorker::new(
            task_mock.clone() as Arc<dyn TaskRepository>,
            result_mock.clone() as Arc<dyn ScrapeResultRepository>,
            30,
        );
        let result = worker.process().await;
        assert!(matches!(result, ProcessResult::Error(_)));
        // 任务清理失败时不应继续清理结果
        assert_eq!(result_mock.cleanup_calls(), 0);
    }

    #[tokio::test]
    async fn test_cleanup_expired_results_returns_count() {
        let task_mock = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let result_mock = Arc::new(MockResultRepository::new(42));
        let worker = ExpirationWorker::new(
            task_mock.clone() as Arc<dyn TaskRepository>,
            result_mock.clone() as Arc<dyn ScrapeResultRepository>,
            14,
        );
        let result = worker.cleanup_expired_results().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(result_mock.cleanup_calls(), 1);
        assert_eq!(result_mock.last_retention(), Some(14));
    }

    #[tokio::test]
    async fn test_cleanup_expired_results_disabled_returns_zero() {
        let task_mock = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let result_mock = Arc::new(MockResultRepository::new(99));
        let worker = ExpirationWorker::new(
            task_mock.clone() as Arc<dyn TaskRepository>,
            result_mock.clone() as Arc<dyn ScrapeResultRepository>,
            0,
        );
        let result = worker.cleanup_expired_results().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
        assert_eq!(result_mock.cleanup_calls(), 0);
    }
}
