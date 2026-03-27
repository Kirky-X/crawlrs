// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task queue tests
//!
//! Tests for the TaskQueue trait and PostgresTaskQueue implementation

use std::sync::Arc;
use chrono::Utc;
use uuid::Uuid;

use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::{RepositoryError, TaskRepository};
use crawlrs::queue::task_queue::{PostgresTaskQueue, QueueError, TaskQueue};

// === Mock Task Repository for Testing ===

/// Mock repository for testing TaskQueue without database
struct MockTaskRepository {
    tasks: Arc<std::sync::Mutex<Vec<Task>>>,
    should_fail: Arc<std::sync::atomic::AtomicBool>,
}

impl MockTaskRepository {
    fn new() -> Self {
        Self {
            tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            should_fail: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn with_failure() -> Self {
        let mut repo = Self::new();
        repo.should_fail
            .store(true, std::sync::atomic::Ordering::SeqCst);
        repo
    }

    fn add_task(&self, task: Task) {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.push(task);
    }

    fn find_task(&self, id: Uuid) -> Option<Task> {
        let tasks = self.tasks.lock().unwrap();
        tasks.iter().find(|t| t.id == id).cloned()
    }

    fn task_count(&self) -> usize {
        let tasks = self.tasks.lock().unwrap();
        tasks.len()
    }
}

#[async_trait::async_trait]
impl TaskRepository for MockTaskRepository {
    async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(RepositoryError::Database(
                "Mock database error".to_string(),
            ));
        }

        let mut tasks = self.tasks.lock().unwrap();
        let mut new_task = task.clone();
        new_task.created_at = Utc::now().into();
        tasks.push(new_task.clone());
        Ok(new_task)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(RepositoryError::Database(
                "Mock database error".to_string(),
            ));
        }

        let tasks = self.tasks.lock().unwrap();
        Ok(tasks.iter().find(|t| t.id == id).cloned())
    }

    async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(RepositoryError::Database(
                "Mock database error".to_string(),
            ));
        }

        let mut tasks = self.tasks.lock().unwrap();
        if let Some(t) = tasks.iter_mut().find(|t| t.id == task.id) {
            *t = task.clone();
            Ok(task.clone())
        } else {
            Err(RepositoryError::NotFound)
        }
    }

    async fn delete(&self, _id: Uuid) -> Result<(), RepositoryError> {
        Ok(())
    }

    async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(RepositoryError::Database(
                "Mock database error".to_string(),
            ));
        }

        let mut tasks = self.tasks.lock().unwrap();
        if let Some(pos) = tasks
            .iter()
            .position(|t| t.status == TaskStatus::Queued)
        {
            let mut task = tasks.remove(pos);
            task.status = TaskStatus::Active;
            task.started_at = Some(Utc::now());
            tasks.push(task.clone());
            Ok(Some(task))
        } else {
            Ok(None)
        }
    }

    async fn mark_completed(&self, id: Uuid) -> Result<Task, RepositoryError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(RepositoryError::Database(
                "Mock database error".to_string(),
            ));
        }

        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.status = TaskStatus::Completed;
            task.completed_at = Some(Utc::now());
            Ok(task.clone())
        } else {
            Err(RepositoryError::NotFound)
        }
    }

    async fn mark_failed(&self, id: Uuid) -> Result<Task, RepositoryError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(RepositoryError::Database(
                "Mock database error".to_string(),
            ));
        }

        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.status = TaskStatus::Failed;
            Ok(task.clone())
        } else {
            Err(RepositoryError::NotFound)
        }
    }

    async fn mark_cancelled(&self, id: Uuid) -> Result<Task, RepositoryError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(RepositoryError::Database(
                "Mock database error".to_string(),
            ));
        }

        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.status = TaskStatus::Cancelled;
            Ok(task.clone())
        } else {
            Err(RepositoryError::NotFound)
        }
    }

    async fn query_tasks(
        &self,
        _filters: &crawlrs::application::dto::task_query_request::TaskQueryRequest,
    ) -> Result<Vec<Task>, RepositoryError> {
        let tasks = self.tasks.lock().unwrap();
        Ok(tasks.clone())
    }

    async fn batch_cancel(&self, _ids: &[Uuid]) -> Result<usize, RepositoryError> {
        Ok(0)
    }

    async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<(), RepositoryError> {
        Ok(())
    }

    async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
        Ok(vec![])
    }

    async fn reset_stuck_tasks(
        &self,
        _duration: chrono::Duration,
    ) -> Result<usize, RepositoryError> {
        Ok(0)
    }

    async fn expire_tasks(&self) -> Result<usize, RepositoryError> {
        Ok(0)
    }
}

// === Helper Functions ===

fn create_test_task(status: TaskStatus) -> Task {
    Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status,
        priority: 0,
        team_id: Uuid::new_v4(),
        api_key_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    }
}

// === QueueError Tests ===

#[test]
fn test_queue_error_display() {
    let error = QueueError::Empty;
    assert_eq!(format!("{}", error), "Queue empty");

    let repo_error = RepositoryError::Database("DB error".to_string());
    let error = QueueError::Repository(repo_error);
    assert!(format!("{}", error).contains("Repository error"));
}

#[test]
fn test_queue_error_from_repository() {
    let repo_error = RepositoryError::NotFound;
    let queue_error: QueueError = repo_error.into();
    assert!(matches!(queue_error, QueueError::Repository(_)));
}

// === PostgresTaskQueue Creation Tests ===

#[test]
fn test_postgres_task_queue_creation() {
    let mock_repo = MockTaskRepository::new();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    // Queue created successfully
    assert_eq!(queue.repository.task_count(), 0);
}

// === Enqueue Tests ===

#[tokio::test]
async fn test_enqueue_task_success() {
    let mock_repo = MockTaskRepository::new();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let task = create_test_task(TaskStatus::Queued);
    let result = queue.enqueue(task.clone()).await;

    assert!(result.is_ok());
    assert_eq!(queue.repository.task_count(), 1);
}

#[tokio::test]
async fn test_enqueue_task_repository_error() {
    let mock_repo = MockTaskRepository::with_failure();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let task = create_test_task(TaskStatus::Queued);
    let result = queue.enqueue(task).await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), QueueError::Repository(_)));
}

// === Dequeue Tests ===

#[tokio::test]
async fn test_dequeue_task_success() {
    let mock_repo = MockTaskRepository::new();
    let task = create_test_task(TaskStatus::Queued);
    mock_repo.add_task(task);

    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));
    let worker_id = Uuid::new_v4();

    let result = queue.dequeue(worker_id).await;

    assert!(result.is_ok());
    let dequeued = result.unwrap();
    assert!(dequeued.is_some());
    assert_eq!(dequeued.unwrap().status, TaskStatus::Active);
}

#[tokio::test]
async fn test_dequeue_empty_queue() {
    let mock_repo = MockTaskRepository::new();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let worker_id = Uuid::new_v4();
    let result = queue.dequeue(worker_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_dequeue_repository_error() {
    let mock_repo = MockTaskRepository::with_failure();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let worker_id = Uuid::new_v4();
    let result = queue.dequeue(worker_id).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_dequeue_multiple_workers() {
    let mock_repo = MockTaskRepository::new();

    // Add multiple tasks
    for _ in 0..5 {
        let task = create_test_task(TaskStatus::Queued);
        mock_repo.add_task(task);
    }

    let queue = Arc::new(PostgresTaskQueue::new(Arc::new(mock_repo)));

    // Simulate multiple workers
    let worker1_id = Uuid::new_v4();
    let worker2_id = Uuid::new_v4();
    let queue1 = queue.clone();
    let queue2 = queue.clone();

    let result1 = queue1.dequeue(worker1_id).await;
    let result2 = queue2.dequeue(worker2_id).await;

    assert!(result1.is_ok());
    assert!(result2.is_ok());
    assert!(result1.unwrap().is_some());
    assert!(result2.unwrap().is_some());
}

// === Complete Tests ===

#[tokio::test]
async fn test_complete_task_success() {
    let mock_repo = MockTaskRepository::new();
    let task = create_test_task(TaskStatus::Active);
    let task_id = task.id;
    mock_repo.add_task(task);

    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let result = queue.complete(task_id).await;

    assert!(result.is_ok());
    let completed_task = queue.repository.find_task(task_id).unwrap();
    assert_eq!(completed_task.status, TaskStatus::Completed);
    assert!(completed_task.completed_at.is_some());
}

#[tokio::test]
async fn test_complete_task_not_found() {
    let mock_repo = MockTaskRepository::new();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let task_id = Uuid::new_v4();
    let result = queue.complete(task_id).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_complete_task_repository_error() {
    let mock_repo = MockTaskRepository::with_failure();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let task_id = Uuid::new_v4();
    let result = queue.complete(task_id).await;

    assert!(result.is_err());
}

// === Fail Tests ===

#[tokio::test]
async fn test_fail_task_success() {
    let mock_repo = MockTaskRepository::new();
    let task = create_test_task(TaskStatus::Active);
    let task_id = task.id;
    mock_repo.add_task(task);

    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let result = queue.fail(task_id).await;

    assert!(result.is_ok());
    let failed_task = queue.repository.find_task(task_id).unwrap();
    assert_eq!(failed_task.status, TaskStatus::Failed);
}

#[tokio::test]
async fn test_fail_task_not_found() {
    let mock_repo = MockTaskRepository::new();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let task_id = Uuid::new_v4();
    let result = queue.fail(task_id).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_fail_task_repository_error() {
    let mock_repo = MockTaskRepository::with_failure();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let task_id = Uuid::new_v4();
    let result = queue.fail(task_id).await;

    assert!(result.is_err());
}

// === Cancel Tests ===

#[tokio::test]
async fn test_cancel_task_success() {
    let mock_repo = MockTaskRepository::new();
    let task = create_test_task(TaskStatus::Queued);
    let task_id = task.id;
    mock_repo.add_task(task);

    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let result = queue.cancel(task_id).await;

    assert!(result.is_ok());
    let cancelled_task = queue.repository.find_task(task_id).unwrap();
    assert_eq!(cancelled_task.status, TaskStatus::Cancelled);
}

#[tokio::test]
async fn test_cancel_active_task() {
    let mock_repo = MockTaskRepository::new();
    let task = create_test_task(TaskStatus::Active);
    let task_id = task.id;
    mock_repo.add_task(task);

    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let result = queue.cancel(task_id).await;

    assert!(result.is_ok());
    let cancelled_task = queue.repository.find_task(task_id).unwrap();
    assert_eq!(cancelled_task.status, TaskStatus::Cancelled);
}

#[tokio::test]
async fn test_cancel_task_not_found() {
    let mock_repo = MockTaskRepository::new();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let task_id = Uuid::new_v4();
    let result = queue.cancel(task_id).await;

    assert!(result.is_err());
}

// === Arc<TaskQueue> Tests ===

#[tokio::test]
async fn test_arc_task_queue_enqueue() {
    let mock_repo = MockTaskRepository::new();
    let queue: Arc<dyn TaskQueue> = Arc::new(PostgresTaskQueue::new(Arc::new(mock_repo)));

    let task = create_test_task(TaskStatus::Queued);
    let result = queue.enqueue(task).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_arc_task_queue_dequeue() {
    let mock_repo = MockTaskRepository::new();
    let task = create_test_task(TaskStatus::Queued);
    mock_repo.add_task(task);

    let queue: Arc<dyn TaskQueue> = Arc::new(PostgresTaskQueue::new(Arc::new(mock_repo)));
    let worker_id = Uuid::new_v4();

    let result = queue.dequeue(worker_id).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_arc_task_queue_complete() {
    let mock_repo = MockTaskRepository::new();
    let task = create_test_task(TaskStatus::Active);
    let task_id = task.id;
    mock_repo.add_task(task);

    let queue: Arc<dyn TaskQueue> = Arc::new(PostgresTaskQueue::new(Arc::new(mock_repo)));

    let result = queue.complete(task_id).await;

    assert!(result.is_ok());
}

// === Complex Workflow Tests ===

#[tokio::test]
async fn test_complete_task_lifecycle() {
    let mock_repo = MockTaskRepository::new();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    // Enqueue task
    let task = create_test_task(TaskStatus::Queued);
    let task_id = task.id;
    let enqueued = queue.enqueue(task).await.unwrap();

    // Dequeue task
    let worker_id = Uuid::new_v4();
    let dequeued = queue.dequeue(worker_id).await.unwrap().unwrap();

    assert_eq!(dequeued.id, task_id);
    assert_eq!(dequeued.status, TaskStatus::Active);

    // Complete task
    queue.complete(task_id).await.unwrap();

    // Verify completion
    let completed = queue.repository.find_task(task_id).unwrap();
    assert_eq!(completed.status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_task_fail_and_retry() {
    let mock_repo = MockTaskRepository::new();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    // Enqueue task
    let task = create_test_task(TaskStatus::Queued);
    let task_id = task.id;
    queue.enqueue(task).await.unwrap();

    // Dequeue
    let worker_id = Uuid::new_v4();
    queue.dequeue(worker_id).await.unwrap().unwrap();

    // Fail task
    queue.fail(task_id).await.unwrap();

    // Verify failed status
    let failed = queue.repository.find_task(task_id).unwrap();
    assert_eq!(failed.status, TaskStatus::Failed);
}

#[tokio::test]
async fn test_task_cancellation_from_queued() {
    let mock_repo = MockTaskRepository::new();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    // Enqueue task
    let task = create_test_task(TaskStatus::Queued);
    let task_id = task.id;
    queue.enqueue(task).await.unwrap();

    // Cancel without dequeuing
    queue.cancel(task_id).await.unwrap();

    // Verify cancellation
    let cancelled = queue.repository.find_task(task_id).unwrap();
    assert_eq!(cancelled.status, TaskStatus::Cancelled);
}

// === Edge Cases ===

#[tokio::test]
async fn test_dequeue_with_no_tasks_returns_none() {
    let mock_repo = MockTaskRepository::new();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let worker_id = Uuid::new_v4();
    let result = queue.dequeue(worker_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_complete_non_existent_task_errors() {
    let mock_repo = MockTaskRepository::new();
    let queue = PostgresTaskQueue::new(Arc::new(mock_repo));

    let random_id = Uuid::new_v4();
    let result = queue.complete(random_id).await;

    assert!(result.is_err());
}
