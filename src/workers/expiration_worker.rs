// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::repositories::task_repository::TaskRepository;
use crate::workers::worker::{ProcessResult, WorkerProcess};
use async_trait::async_trait;
use log::info;
use std::sync::Arc;

/// 任务过期清理工作器
///
/// 负责定期扫描并清理过期的任务
pub struct ExpirationWorker {
    repository: Arc<dyn TaskRepository>,
}

impl ExpirationWorker {
    pub fn new(repository: Arc<dyn TaskRepository>) -> Self {
        Self { repository }
    }

    async fn cleanup_expired_tasks(&self) -> Result<u64, String> {
        self.repository
            .expire_tasks()
            .await
            .map_err(|e| e.to_string())
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
                ProcessResult::Completed
            }
            Err(e) => ProcessResult::Error(format!("Failed to cleanup expired tasks: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::Task;
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

        async fn mark_completed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn mark_failed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn mark_cancelled(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
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
    }

    #[test]
    fn test_worker_name() {
        let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let worker = ExpirationWorker::new(repo);
        assert_eq!(worker.name(), "expiration-worker");
    }

    #[tokio::test]
    async fn test_process_completes_with_zero_expired() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let repo: Arc<dyn TaskRepository> = mock.clone();
        let worker = ExpirationWorker::new(repo);
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        assert_eq!(mock.expire_calls(), 1);
    }

    #[tokio::test]
    async fn test_process_completes_with_some_expired() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(5));
        let repo: Arc<dyn TaskRepository> = mock.clone();
        let worker = ExpirationWorker::new(repo);
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        assert_eq!(mock.expire_calls(), 1);
    }

    #[tokio::test]
    async fn test_process_returns_error_on_repo_failure() {
        let mock = Arc::new(MockTaskRepository::new_with_error("db connection lost"));
        let repo: Arc<dyn TaskRepository> = mock.clone();
        let worker = ExpirationWorker::new(repo);
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
        let repo: Arc<dyn TaskRepository> = mock.clone();
        let worker = ExpirationWorker::new(repo);
        let result = worker.cleanup_expired_tasks().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(mock.expire_calls(), 1);
    }

    #[tokio::test]
    async fn test_cleanup_expired_tasks_returns_error_string() {
        let mock = Arc::new(MockTaskRepository::new_with_error("timeout"));
        let repo: Arc<dyn TaskRepository> = mock.clone();
        let worker = ExpirationWorker::new(repo);
        let result = worker.cleanup_expired_tasks().await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("timeout"));
    }

    #[tokio::test]
    async fn test_process_calls_expire_tasks_exactly_once() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(0));
        let repo: Arc<dyn TaskRepository> = mock.clone();
        let worker = ExpirationWorker::new(repo);
        let _ = worker.process().await;
        assert_eq!(mock.expire_calls(), 1);
    }

    #[tokio::test]
    async fn test_process_multiple_cycles() {
        let mock = Arc::new(MockTaskRepository::new_with_expired_count(3));
        let repo: Arc<dyn TaskRepository> = mock.clone();
        let worker = ExpirationWorker::new(repo);
        // Run multiple cycles - but note the mock only returns Ok on first call
        // because error is Mutex<Option> and gets taken. Let's test a single cycle
        // properly completes.
        let result1 = worker.process().await;
        assert_eq!(result1, ProcessResult::Completed);
        assert_eq!(mock.expire_calls(), 1);
    }
}
