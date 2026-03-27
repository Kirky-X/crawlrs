// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task scheduler tests
//!
//! Tests for the TaskScheduler including scheduling logic and task management

use std::sync::Arc;
use chrono::{Duration, Utc};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::{RepositoryError, TaskRepository};
use crawlrs::queue::scheduler::TaskScheduler;
use crawlrs::queue::task_queue::QueueError;

// === Mock Task Repository for Testing ===

struct MockTaskRepository {
    tasks: Arc<std::sync::Mutex<Vec<Task>>>,
    should_fail: Arc<std::sync::atomic::AtomicBool>,
    reset_count: Arc<std::sync::atomic::AtomicUsize>,
    expire_count: Arc<std::sync::atomic::AtomicUsize>,
}

impl MockTaskRepository {
    fn new() -> Self {
        Self {
            tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            should_fail: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            reset_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            expire_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    fn add_task(&self, task: Task) {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.push(task);
    }

    fn get_reset_count(&self) -> usize {
        self.reset_count.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn get_expire_count(&self) -> usize {
        self.expire_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl TaskRepository for MockTaskRepository {
    async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(RepositoryError::Database("Mock error".to_string()));
        }

        let mut tasks = self.tasks.lock().unwrap();
        let mut new_task = task.clone();
        new_task.created_at = Utc::now().into();
        tasks.push(new_task.clone());
        Ok(new_task)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
        let tasks = self.tasks.lock().unwrap();
        Ok(tasks.iter().find(|t| t.id == id).cloned())
    }

    async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(RepositoryError::Database("Mock error".to_string()));
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
        Ok(None)
    }

    async fn mark_completed(&self, _id: Uuid) -> Result<Task, RepositoryError> {
        Err(RepositoryError::NotFound)
    }

    async fn mark_failed(&self, _id: Uuid) -> Result<Task, RepositoryError> {
        Err(RepositoryError::NotFound)
    }

    async fn mark_cancelled(&self, _id: Uuid) -> Result<Task, RepositoryError> {
        Err(RepositoryError::NotFound)
    }

    async fn query_tasks(
        &self,
        _filters: &crawlrs::application::dto::task_query_request::TaskQueryRequest,
    ) -> Result<Vec<Task>, RepositoryError> {
        Ok(vec![])
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

    async fn reset_stuck_tasks(&self, _duration: Duration) -> Result<usize, RepositoryError> {
        self.reset_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(0)
    }

    async fn expire_tasks(&self) -> Result<usize, RepositoryError> {
        self.expire_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(0)
    }
}

// === Helper Functions ===

fn create_test_task() -> Task {
    Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
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

// === Scheduler Creation Tests ===

#[test]
fn test_scheduler_creation() {
    let mock_repo = MockTaskRepository::new();
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    // Scheduler created successfully
    assert_eq!(scheduler.get_reset_count(), 0);
    assert_eq!(scheduler.get_expire_count(), 0);
}

// === Schedule At Tests ===

#[tokio::test]
async fn test_schedule_at_future_time() {
    let mock_repo = MockTaskRepository::new();
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let task = create_test_task();
    let future_time = Utc::now() + Duration::seconds(60);

    let result = scheduler.schedule_at(task, future_time).await;

    assert!(result.is_ok());
    let scheduled_task = result.unwrap();
    assert_eq!(scheduled_task.status, TaskStatus::Queued);
    assert!(scheduled_task.scheduled_at.is_some());
}

#[tokio::test]
async fn test_schedule_at_past_time() {
    let mock_repo = MockTaskRepository::new();
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let task = create_test_task();
    let past_time = Utc::now() - Duration::seconds(60);

    let result = scheduler.schedule_at(task, past_time).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_schedule_at_repository_error() {
    let mock_repo = MockTaskRepository {
        tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
        should_fail: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        reset_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        expire_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let task = create_test_task();
    let future_time = Utc::now() + Duration::seconds(60);

    let result = scheduler.schedule_at(task, future_time).await;

    assert!(result.is_err());
}

// === Schedule In Tests ===

#[tokio::test]
async fn test_schedule_in_seconds() {
    let mock_repo = MockTaskRepository::new();
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let task = create_test_task();
    let delay = Duration::seconds(30);

    let result = scheduler.schedule_in(task, delay).await;

    assert!(result.is_ok());
    let scheduled_task = result.unwrap();
    assert!(scheduled_task.scheduled_at.is_some());
}

#[tokio::test]
async fn test_schedule_in_zero_duration() {
    let mock_repo = MockTaskRepository::new();
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let task = create_test_task();
    let delay = Duration::seconds(0);

    let result = scheduler.schedule_in(task, delay).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_schedule_in_negative_duration() {
    let mock_repo = MockTaskRepository::new();
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let task = create_test_task();
    let delay = Duration::seconds(-10);

    let result = scheduler.schedule_in(task, delay).await;

    // Should schedule in the past
    assert!(result.is_ok());
}

// === Reschedule Retry Tests ===

#[tokio::test]
async fn test_reschedule_retry_with_remaining_attempts() {
    let mock_repo = MockTaskRepository::new();
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let mut task = create_test_task();
    task.status = TaskStatus::Failed;
    task.retry_count = 1;
    task.attempt_count = 1;
    task.max_retries = 3;

    let delay = Duration::seconds(10);

    let result = scheduler.reschedule_retry(task, delay).await;

    assert!(result.is_ok());
    let rescheduled = result.unwrap();
    assert_eq!(rescheduled.status, TaskStatus::Queued);
    assert_eq!(rescheduled.attempt_count, 2);
    assert!(rescheduled.started_at.is_none());
    assert!(rescheduled.completed_at.is_none());
}

#[tokio::test]
async fn test_reschedule_retry_max_retries_exceeded() {
    let mock_repo = MockTaskRepository::new();
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let mut task = create_test_task();
    task.status = TaskStatus::Failed;
    task.retry_count = 3;
    task.attempt_count = 3;
    task.max_retries = 3;

    let delay = Duration::seconds(10);

    let result = scheduler.reschedule_retry(task, delay).await;

    assert!(result.is_ok());
    let final_task = result.unwrap();
    assert_eq!(final_task.status, TaskStatus::Failed);
    assert!(final_task.completed_at.is_some());
}

#[tokio::test]
async fn test_reschedule_retry_repository_error() {
    let mock_repo = MockTaskRepository {
        tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
        should_fail: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        reset_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        expire_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let task = create_test_task();
    let delay = Duration::seconds(10);

    let result = scheduler.reschedule_retry(task, delay).await;

    assert!(result.is_err());
}

// === Schedule Urgent Tests ===

#[tokio::test]
async fn test_schedule_urgent() {
    let mock_repo = MockTaskRepository::new();
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let task = create_test_task();

    let result = scheduler.schedule_urgent(task).await;

    assert!(result.is_ok());
    let urgent_task = result.unwrap();
    assert_eq!(urgent_task.status, TaskStatus::Queued);
    assert_eq!(urgent_task.priority, 100);
    assert!(urgent_task.scheduled_at.is_some());
}

#[tokio::test]
async fn test_schedule_urgent_repository_error() {
    let mock_repo = MockTaskRepository {
        tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
        should_fail: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        reset_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        expire_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let task = create_test_task();

    let result = scheduler.schedule_urgent(task).await;

    assert!(result.is_err());
}

// === Background Task Tests ===

#[tokio::test]
async fn test_scheduler_background_task() {
    let mock_repo = Arc::new(MockTaskRepository::new());
    let scheduler = TaskScheduler::new(mock_repo.clone());

    // Start the background task
    let handle = scheduler.start();

    // Wait a bit for the scheduler to run
    sleep(tokio::time::Duration::from_millis(150)).await;

    // Verify that maintenance tasks ran
    assert!(mock_repo.get_reset_count() > 0 || mock_repo.get_expire_count() > 0);

    // The handle is a JoinHandle - we don't need to abort it for this test
    // In production, you'd use abort() to stop the scheduler
    drop(handle);
}

#[tokio::test]
async fn test_scheduler_background_task_continues_running() {
    let mock_repo = Arc::new(MockTaskRepository::new());
    let scheduler = TaskScheduler::new(mock_repo.clone());

    let handle = scheduler.start();

    // Wait for multiple ticks
    sleep(tokio::time::Duration::from_millis(250)).await;

    let reset_count = mock_repo.get_reset_count();
    let expire_count = mock_repo.get_expire_count();

    // Should have run multiple times
    assert!(reset_count >= 2 || expire_count >= 2);

    drop(handle);
}

// === Edge Cases ===

#[tokio::test]
async fn test_schedule_task_with_zero_timestamp() {
    let mock_repo = MockTaskRepository::new();
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let mut task = create_test_task();
    task.created_at = chrono::DateTime::from_timestamp(0, 0).unwrap().into();

    let future_time = Utc::now() + Duration::seconds(60);

    let result = scheduler.schedule_at(task, future_time).await;

    assert!(result.is_ok());
    let scheduled = result.unwrap();
    assert!(scheduled.created_at.timestamp() > 0);
}

#[tokio::test]
async fn test_reschedule_with_zero_delay() {
    let mock_repo = MockTaskRepository::new();
    let scheduler = TaskScheduler::new(Arc::new(mock_repo));

    let mut task = create_test_task();
    task.status = TaskStatus::Failed;
    task.retry_count = 0;
    task.max_retries = 3;

    let delay = Duration::seconds(0);

    let result = scheduler.reschedule_retry(task, delay).await;

    assert!(result.is_ok());
    let rescheduled = result.unwrap();
    assert_eq!(rescheduled.status, TaskStatus::Queued);
}

// === QueueError Conversion Tests ===

#[test]
fn test_queue_error_from_repository_error() {
    let repo_error = RepositoryError::Database("Test error".to_string());
    let queue_error: QueueError = repo_error.into();

    assert!(matches!(queue_error, QueueError::Repository(_)));
}
