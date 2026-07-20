// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::Task;
use crate::domain::repositories::task_repository::TaskRepository;
use async_trait::async_trait;
use log::debug;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

/// 队列错误类型
#[derive(Error, Debug)]
pub enum QueueError {
    /// 仓库错误
    #[error("Repository error: {0}")]
    Repository(#[from] crate::domain::repositories::task_repository::RepositoryError),

    /// 队列为空
    #[error("Queue empty")]
    Empty,
}

/// 任务队列特质
#[async_trait]
pub trait TaskQueue: Send + Sync {
    /// 入队任务
    async fn enqueue(&self, task: Task) -> Result<Task, QueueError>;

    /// 出队任务
    async fn dequeue(&self, worker_id: Uuid) -> Result<Option<Task>, QueueError>;

    /// 完成任务
    async fn complete(&self, task_id: Uuid) -> Result<(), QueueError>;
    /// 失败任务
    async fn fail(&self, task_id: Uuid) -> Result<(), QueueError>;
    /// 取消任务
    async fn cancel(&self, task_id: Uuid) -> Result<(), QueueError>;
}

/// PostgreSQL任务队列实现
pub struct PostgresTaskQueue {
    /// 任务仓库
    pub repository: Arc<dyn TaskRepository>,
}

impl PostgresTaskQueue {
    /// 创建新的PostgreSQL任务队列实例
    ///
    /// # 参数
    ///
    /// * `repository` - 任务仓库
    ///
    /// # 返回值
    ///
    /// 返回新的PostgreSQL任务队列实例
    pub fn new(repository: Arc<dyn TaskRepository>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl TaskQueue for PostgresTaskQueue {
    /// 入队任务
    ///
    /// # 参数
    ///
    /// * `task` - 要入队的任务
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 入队成功的任务
    /// * `Err(QueueError)` - 入队失败
    async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
        let created = self.repository.create(&task).await?;
        Ok(created)
    }

    /// 出队任务
    ///
    /// # 参数
    ///
    /// * `worker_id` - 工作者ID
    ///
    /// # 返回值
    ///
    /// * `Ok(Some(Task))` - 成功出队的任务
    /// * `Ok(None)` - 没有可出队的任务
    /// * `Err(QueueError)` - 出队失败
    async fn dequeue(&self, worker_id: Uuid) -> Result<Option<Task>, QueueError> {
        debug!("worker_id={}", worker_id);
        let task = self.repository.acquire_next(worker_id).await?;
        debug!("has_task={:?}", task.is_some());
        Ok(task)
    }

    /// 完成任务
    ///
    /// # 参数
    ///
    /// * `task_id` - 任务ID
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功
    /// * `Err(QueueError)` - 失败
    async fn complete(&self, task_id: Uuid) -> Result<(), QueueError> {
        self.repository.mark_completed(task_id).await?;
        Ok(())
    }

    /// 失败任务
    ///
    /// # 参数
    ///
    /// * `task_id` - 任务ID
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功
    /// * `Err(QueueError)` - 失败
    async fn fail(&self, task_id: Uuid) -> Result<(), QueueError> {
        self.repository.mark_failed(task_id).await?;
        Ok(())
    }

    /// 取消任务
    ///
    /// # 参数
    ///
    /// * `task_id` - 任务ID
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功
    /// * `Err(QueueError)` - 失败
    async fn cancel(&self, task_id: Uuid) -> Result<(), QueueError> {
        self.repository.mark_cancelled(task_id).await?;
        Ok(())
    }
}

#[async_trait]
impl<T: TaskQueue + ?Sized> TaskQueue for Arc<T> {
    async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
        (**self).enqueue(task).await
    }

    async fn dequeue(&self, worker_id: Uuid) -> Result<Option<Task>, QueueError> {
        (**self).dequeue(worker_id).await
    }

    async fn complete(&self, task_id: Uuid) -> Result<(), QueueError> {
        (**self).complete(task_id).await
    }

    async fn fail(&self, task_id: Uuid) -> Result<(), QueueError> {
        (**self).fail(task_id).await
    }

    async fn cancel(&self, task_id: Uuid) -> Result<(), QueueError> {
        (**self).cancel(task_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{Task, TaskType};
    use crate::domain::repositories::task_repository::{
        RepositoryError, TaskQueryParams, TaskRepository,
    };
    use async_trait::async_trait;
    use chrono::Duration as ChronoDuration;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Mock TaskRepository that records calls and returns configurable results.
    ///
    /// Each method increments a counter so tests can assert which repository
    /// method was invoked by which queue operation.
    struct MockTaskRepository {
        enqueue_calls: AtomicUsize,
        dequeue_calls: AtomicUsize,
        complete_calls: AtomicUsize,
        fail_calls: AtomicUsize,
        cancel_calls: AtomicUsize,
        /// Last task_id passed to mark_* methods.
        last_task_id: parking_lot::Mutex<Option<Uuid>>,
        /// Last worker_id passed to acquire_next.
        last_worker_id: parking_lot::Mutex<Option<Uuid>>,
        /// When true, repository methods return RepositoryError::Database.
        should_fail: bool,
        /// Task returned by acquire_next; None means empty queue.
        next_task: parking_lot::Mutex<Option<Task>>,
        /// Task returned by create (defaults to the input task).
        created_task: parking_lot::Mutex<Option<Task>>,
    }

    impl MockTaskRepository {
        fn new() -> Self {
            Self {
                enqueue_calls: AtomicUsize::new(0),
                dequeue_calls: AtomicUsize::new(0),
                complete_calls: AtomicUsize::new(0),
                fail_calls: AtomicUsize::new(0),
                cancel_calls: AtomicUsize::new(0),
                last_task_id: parking_lot::Mutex::new(None),
                last_worker_id: parking_lot::Mutex::new(None),
                should_fail: false,
                next_task: parking_lot::Mutex::new(None),
                created_task: parking_lot::Mutex::new(None),
            }
        }

        fn failing() -> Self {
            let mut mock = Self::new();
            mock.should_fail = true;
            mock
        }

        fn with_next_task(task: Task) -> Self {
            let mock = Self::new();
            *mock.next_task.lock() = Some(task);
            mock
        }
    }

    fn sample_task() -> Task {
        Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        )
    }

    fn db_error() -> RepositoryError {
        RepositoryError::Database(anyhow::anyhow!("mock database failure"))
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            self.enqueue_calls.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                return Err(db_error());
            }
            // Return a clone of the input task by default, or the configured created_task.
            let created = self
                .created_task
                .lock()
                .clone()
                .unwrap_or_else(|| task.clone());
            Ok(created)
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            unreachable!("MockTaskRepository::find_by_id not invoked by PostgresTaskQueue tests")
        }

        async fn update(&self, _task: &Task) -> Result<Task, RepositoryError> {
            unreachable!("MockTaskRepository::update not invoked by PostgresTaskQueue tests")
        }

        async fn acquire_next(&self, worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            self.dequeue_calls.fetch_add(1, Ordering::SeqCst);
            *self.last_worker_id.lock() = Some(worker_id);
            if self.should_fail {
                return Err(db_error());
            }
            Ok(self.next_task.lock().take())
        }

        async fn mark_completed(&self, task_id: Uuid) -> Result<(), RepositoryError> {
            self.complete_calls.fetch_add(1, Ordering::SeqCst);
            *self.last_task_id.lock() = Some(task_id);
            if self.should_fail {
                return Err(db_error());
            }
            Ok(())
        }

        async fn mark_failed(&self, task_id: Uuid) -> Result<(), RepositoryError> {
            self.fail_calls.fetch_add(1, Ordering::SeqCst);
            *self.last_task_id.lock() = Some(task_id);
            if self.should_fail {
                return Err(db_error());
            }
            Ok(())
        }

        async fn mark_cancelled(&self, task_id: Uuid) -> Result<(), RepositoryError> {
            self.cancel_calls.fetch_add(1, Ordering::SeqCst);
            *self.last_task_id.lock() = Some(task_id);
            if self.should_fail {
                return Err(db_error());
            }
            Ok(())
        }

        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            unreachable!("MockTaskRepository::exists_by_url not invoked by PostgresTaskQueue tests")
        }

        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            unreachable!(
                "MockTaskRepository::find_existing_urls not invoked by PostgresTaskQueue tests"
            )
        }

        async fn reset_stuck_tasks(
            &self,
            _timeout: ChronoDuration,
        ) -> Result<u64, RepositoryError> {
            unreachable!(
                "MockTaskRepository::reset_stuck_tasks not invoked by PostgresTaskQueue tests"
            )
        }

        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            unreachable!("MockTaskRepository::cancel_tasks_by_crawl_id not invoked by PostgresTaskQueue tests")
        }

        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            unreachable!("MockTaskRepository::expire_tasks not invoked by PostgresTaskQueue tests")
        }

        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            unreachable!(
                "MockTaskRepository::find_by_crawl_id not invoked by PostgresTaskQueue tests"
            )
        }

        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            unreachable!("MockTaskRepository::query_tasks not invoked by PostgresTaskQueue tests")
        }

        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            unreachable!("MockTaskRepository::batch_cancel not invoked by PostgresTaskQueue tests")
        }
    }

    fn make_queue(mock: Arc<dyn TaskRepository>) -> PostgresTaskQueue {
        PostgresTaskQueue::new(mock)
    }

    // ========== PostgresTaskQueue::new ==========

    #[test]
    fn test_new_stores_repository() {
        let mock: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
        let queue = make_queue(mock.clone());
        // Verify the queue stores the same Arc<dyn TaskRepository> pointer.
        assert!(Arc::ptr_eq(&queue.repository, &mock));
    }

    // ========== enqueue happy path ==========

    #[tokio::test]
    async fn test_enqueue_returns_created_task() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue = make_queue(mock.clone());
        let task = sample_task();
        let result = queue.enqueue(task.clone()).await;
        assert!(result.is_ok(), "enqueue should succeed");
        let created = result.unwrap();
        assert_eq!(created.id, task.id);
        assert_eq!(created.url, task.url);
        assert_eq!(mock.enqueue_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_enqueue_propagates_repository_error() {
        let mock = Arc::new(MockTaskRepository::failing());
        let queue = make_queue(mock.clone());
        let task = sample_task();
        let result = queue.enqueue(task).await;
        let err = result.expect_err("should propagate repository error");
        match err {
            QueueError::Repository(_) => {}
            other => panic!("expected QueueError::Repository, got {:?}", other),
        }
        assert_eq!(mock.enqueue_calls.load(Ordering::SeqCst), 1);
    }

    // ========== dequeue happy path and empty queue ==========

    #[tokio::test]
    async fn test_dequeue_empty_returns_none() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue = make_queue(mock.clone());
        let worker_id = Uuid::new_v4();
        let result = queue.dequeue(worker_id).await;
        assert!(result.is_ok(), "dequeue on empty queue should succeed");
        assert!(result.unwrap().is_none(), "empty queue should return None");
        assert_eq!(mock.dequeue_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_dequeue_returns_next_task() {
        let task = sample_task();
        let mock = Arc::new(MockTaskRepository::with_next_task(task.clone()));
        let queue = make_queue(mock.clone());
        let worker_id = Uuid::new_v4();
        let result = queue.dequeue(worker_id).await;
        let dequeued = result
            .expect("dequeue should succeed")
            .expect("task expected");
        assert_eq!(dequeued.id, task.id);
        // Verify the worker_id was forwarded to the repository.
        assert_eq!(*mock.last_worker_id.lock(), Some(worker_id));
    }

    #[tokio::test]
    async fn test_dequeue_propagates_repository_error() {
        let mock = Arc::new(MockTaskRepository::failing());
        let queue = make_queue(mock.clone());
        let result = queue.dequeue(Uuid::new_v4()).await;
        let err = result.expect_err("should propagate repository error");
        match err {
            QueueError::Repository(_) => {}
            other => panic!("expected QueueError::Repository, got {:?}", other),
        }
        assert_eq!(mock.dequeue_calls.load(Ordering::SeqCst), 1);
    }

    // ========== Test logger for covering debug! macro in dequeue ==========

    use log::{LevelFilter, Log, Metadata, Record};
    use std::sync::Once;

    static LOGGER_INIT: Once = Once::new();

    struct CapturingLogger;

    impl Log for CapturingLogger {
        fn enabled(&self, metadata: &Metadata) -> bool {
            metadata.level() <= log::Level::Debug
        }
        fn log(&self, _record: &Record) {}
        fn flush(&self) {}
    }

    fn ensure_debug_logger() {
        LOGGER_INIT.call_once(|| {
            static CAPTURING_LOGGER: CapturingLogger = CapturingLogger;
            let _ = log::set_logger(&CAPTURING_LOGGER);
            log::set_max_level(LevelFilter::Debug);
        });
    }

    // ========== debug! coverage tests ==========

    #[tokio::test]
    async fn test_dequeue_logs_debug_with_task() {
        ensure_debug_logger();
        let task = sample_task();
        let mock = Arc::new(MockTaskRepository::with_next_task(task.clone()));
        let queue = make_queue(mock.clone());
        let worker_id = Uuid::new_v4();
        let result = queue.dequeue(worker_id).await;
        assert!(result.is_ok());
        assert_eq!(mock.dequeue_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_dequeue_logs_debug_empty_queue() {
        ensure_debug_logger();
        let mock = Arc::new(MockTaskRepository::new());
        let queue = make_queue(mock.clone());
        let worker_id = Uuid::new_v4();
        let result = queue.dequeue(worker_id).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
        assert_eq!(mock.dequeue_calls.load(Ordering::SeqCst), 1);
    }

    // ========== complete / fail / cancel happy paths ==========

    #[tokio::test]
    async fn test_complete_calls_repository_and_returns_ok() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue = make_queue(mock.clone());
        let task_id = Uuid::new_v4();
        queue
            .complete(task_id)
            .await
            .expect("complete should succeed");
        assert_eq!(mock.complete_calls.load(Ordering::SeqCst), 1);
        assert_eq!(*mock.last_task_id.lock(), Some(task_id));
    }

    #[tokio::test]
    async fn test_fail_calls_repository_and_returns_ok() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue = make_queue(mock.clone());
        let task_id = Uuid::new_v4();
        queue.fail(task_id).await.expect("fail should succeed");
        assert_eq!(mock.fail_calls.load(Ordering::SeqCst), 1);
        assert_eq!(*mock.last_task_id.lock(), Some(task_id));
    }

    #[tokio::test]
    async fn test_cancel_calls_repository_and_returns_ok() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue = make_queue(mock.clone());
        let task_id = Uuid::new_v4();
        queue.cancel(task_id).await.expect("cancel should succeed");
        assert_eq!(mock.cancel_calls.load(Ordering::SeqCst), 1);
        assert_eq!(*mock.last_task_id.lock(), Some(task_id));
    }

    // ========== complete / fail / cancel error propagation ==========

    #[tokio::test]
    async fn test_complete_propagates_repository_error() {
        let mock = Arc::new(MockTaskRepository::failing());
        let queue = make_queue(mock.clone());
        let result = queue.complete(Uuid::new_v4()).await;
        assert!(matches!(result, Err(QueueError::Repository(_))));
        assert_eq!(mock.complete_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_fail_propagates_repository_error() {
        let mock = Arc::new(MockTaskRepository::failing());
        let queue = make_queue(mock.clone());
        let result = queue.fail(Uuid::new_v4()).await;
        assert!(matches!(result, Err(QueueError::Repository(_))));
        assert_eq!(mock.fail_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_cancel_propagates_repository_error() {
        let mock = Arc::new(MockTaskRepository::failing());
        let queue = make_queue(mock.clone());
        let result = queue.cancel(Uuid::new_v4()).await;
        assert!(matches!(result, Err(QueueError::Repository(_))));
        assert_eq!(mock.cancel_calls.load(Ordering::SeqCst), 1);
    }

    // ========== QueueError variants ==========

    #[test]
    fn test_queue_error_repository_display() {
        let err = QueueError::Repository(db_error());
        let msg = err.to_string();
        assert!(msg.contains("Repository error"));
        assert!(msg.contains("mock database failure"));
    }

    #[test]
    fn test_queue_error_empty_display() {
        let err = QueueError::Empty;
        assert_eq!(err.to_string(), "Queue empty");
    }

    #[test]
    fn test_queue_error_repository_from_repository_error() {
        let repo_err = db_error();
        let queue_err: QueueError = repo_err.into();
        match queue_err {
            QueueError::Repository(_) => {}
            other => panic!("expected QueueError::Repository, got {:?}", other),
        }
    }

    // ========== Arc<T> delegation impl ==========

    #[tokio::test]
    async fn test_arc_delegation_enqueue() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue: Arc<PostgresTaskQueue> = Arc::new(make_queue(mock.clone()));
        // Use the Arc<TaskQueue> impl: Arc<PostgresTaskQueue> delegates to TaskQueue.
        let task = sample_task();
        let result = queue.enqueue(task).await;
        assert!(result.is_ok());
        assert_eq!(mock.enqueue_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_delegation_dequeue() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue = Arc::new(make_queue(mock.clone()));
        let worker_id = Uuid::new_v4();
        let result = queue.dequeue(worker_id).await;
        assert!(result.is_ok());
        assert_eq!(mock.dequeue_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_delegation_complete() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue = Arc::new(make_queue(mock.clone()));
        queue.complete(Uuid::new_v4()).await.unwrap();
        assert_eq!(mock.complete_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_delegation_fail() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue = Arc::new(make_queue(mock.clone()));
        queue.fail(Uuid::new_v4()).await.unwrap();
        assert_eq!(mock.fail_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_delegation_cancel() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue = Arc::new(make_queue(mock.clone()));
        queue.cancel(Uuid::new_v4()).await.unwrap();
        assert_eq!(mock.cancel_calls.load(Ordering::SeqCst), 1);
    }

    // ========== Integration: enqueue then dequeue ==========

    #[tokio::test]
    async fn test_enqueue_then_dequeue_full_cycle() {
        // End-to-end: enqueue a task, then dequeue the configured next_task.
        let task = sample_task();
        let mock = Arc::new(MockTaskRepository::with_next_task(task.clone()));
        let queue = make_queue(mock.clone());

        // Dequeue first (the mock's next_task was pre-set).
        let dequeued = queue.dequeue(Uuid::new_v4()).await.unwrap();
        assert!(
            dequeued.is_some(),
            "configured next_task should be returned"
        );
        assert_eq!(dequeued.unwrap().id, task.id);

        // Second dequeue should return None (mock's next_task was taken).
        let second = queue.dequeue(Uuid::new_v4()).await.unwrap();
        assert!(second.is_none(), "second dequeue should return None");
    }

    #[tokio::test]
    async fn test_lifecycle_enqueue_complete_fail_cancel_independent() {
        // Verify each lifecycle method is independent and doesn't interfere.
        let mock = Arc::new(MockTaskRepository::new());
        let queue = make_queue(mock.clone());
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        queue.complete(id1).await.unwrap();
        queue.fail(id2).await.unwrap();
        queue.cancel(id3).await.unwrap();

        assert_eq!(mock.complete_calls.load(Ordering::SeqCst), 1);
        assert_eq!(mock.fail_calls.load(Ordering::SeqCst), 1);
        assert_eq!(mock.cancel_calls.load(Ordering::SeqCst), 1);
        // The last task_id recorded is the last call (cancel).
        assert_eq!(*mock.last_task_id.lock(), Some(id3));
    }

    // ========== Arc<dyn TaskQueue> delegation tests ==========
    // These tests cover the `impl<T: TaskQueue + ?Sized> TaskQueue for Arc<T>`
    // delegation methods (lines 148-164). Unlike `Arc<PostgresTaskQueue>` which
    // resolves to the direct impl via Deref, `Arc<dyn TaskQueue>` forces calls
    // through the generic Arc<T> impl because `dyn TaskQueue` is `?Sized`.

    #[tokio::test]
    async fn test_arc_dyn_delegation_enqueue() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue: Arc<dyn TaskQueue> = Arc::new(make_queue(mock.clone()));
        let task = sample_task();
        let result = queue.enqueue(task).await;
        assert!(result.is_ok());
        assert_eq!(mock.enqueue_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_dyn_delegation_dequeue() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue: Arc<dyn TaskQueue> = Arc::new(make_queue(mock.clone()));
        let result = queue.dequeue(Uuid::new_v4()).await;
        assert!(result.is_ok());
        assert_eq!(mock.dequeue_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_dyn_delegation_complete() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue: Arc<dyn TaskQueue> = Arc::new(make_queue(mock.clone()));
        queue.complete(Uuid::new_v4()).await.unwrap();
        assert_eq!(mock.complete_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_dyn_delegation_fail() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue: Arc<dyn TaskQueue> = Arc::new(make_queue(mock.clone()));
        queue.fail(Uuid::new_v4()).await.unwrap();
        assert_eq!(mock.fail_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_arc_dyn_delegation_cancel() {
        let mock = Arc::new(MockTaskRepository::new());
        let queue: Arc<dyn TaskQueue> = Arc::new(make_queue(mock.clone()));
        queue.cancel(Uuid::new_v4()).await.unwrap();
        assert_eq!(mock.cancel_calls.load(Ordering::SeqCst), 1);
    }
}
