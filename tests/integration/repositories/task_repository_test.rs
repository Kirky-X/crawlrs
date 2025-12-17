// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::helpers::create_test_app;
use chrono::Utc;
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::TaskRepository;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use uuid::Uuid;
use std::sync::Arc;

/// 测试并发任务获取和超时
///
/// 验证在多个工作程序并发获取任务时，只有一个能成功，
/// 并且在任务锁超时后，其他工作程序可以接管该任务。
///
/// 对应文档章节：3.3.3
#[tokio::test]
async fn test_concurrent_task_acquisition_and_timeout() {
    let app = create_test_app().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(10),
    ));
    let team_id = Uuid::new_v4();
    let worker1_id = Uuid::new_v4();
    let worker2_id = Uuid::new_v4();

    // Create a single task
    let task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: "https://example.com/concurrent".to_string(),
        payload: serde_json::json!({}),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    };

    repo.create(&task).await.unwrap();

    // --- Concurrent Acquisition ---
    let repo1 = repo.clone();
    let handle1 = tokio::spawn(async move {
        repo1.acquire_next(worker1_id).await.unwrap()
    });

    let repo2 = repo.clone();
    let handle2 = tokio::spawn(async move {
        // Give the first worker a moment to acquire the task
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        repo2.acquire_next(worker2_id).await.unwrap()
    });

    let result1 = handle1.await.unwrap();
    let result2 = handle2.await.unwrap();

    // Assert that only one worker got the task
    assert!(result1.is_some());
    assert!(result2.is_none());

    // --- Lock Timeout and Re-acquisition ---
    // The default lock timeout is 10 seconds in the test environment.
    // We wait for 11 seconds to ensure the lock expires.
    tokio::time::sleep(tokio::time::Duration::from_secs(11)).await;

    // Worker 2 should now be able to acquire the task
    let reacquired_task = repo.acquire_next(worker2_id).await.unwrap();
    assert!(reacquired_task.is_some());
    assert_eq!(reacquired_task.unwrap().id, task.id);
}


/// 测试仓库的CRUD操作
///
/// 验证TaskRepository的创建、读取和更新功能是否正常。
///
/// 对应文档章节：3.3.1
#[tokio::test]
async fn test_repository_crud_operations() {
    let app = create_test_app().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(10));
    let team_id = Uuid::new_v4();

    let new_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: "https://example.com".to_string(),
        payload: serde_json::json!({}),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    };

    // Create
    repo.create(&new_task).await.unwrap();

    // Read
    let found_task = repo.find_by_id(new_task.id).await.unwrap().unwrap();
    assert_eq!(found_task.id, new_task.id);

    // Update
    let mut updated_task = found_task;
    updated_task.status = TaskStatus::Active;
    repo.update(&updated_task).await.unwrap();

    let found_after_update = repo.find_by_id(updated_task.id).await.unwrap().unwrap();
    assert_eq!(found_after_update.status, TaskStatus::Active);
}

/// 测试获取下一个任务
///
/// 验证acquire_next方法是否能正确地获取并锁定队列中的下一个任务。
///
// 对应文档章节：3.3.2
#[tokio::test]
async fn test_repository_acquire_next_task() {
    let app = create_test_app().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(10));
    let team_id = Uuid::new_v4();
    let worker_id = Uuid::new_v4();

    // Create a couple of tasks
    let task1 = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 1,
        team_id,
        url: "https://example.com/1".to_string(),
        payload: serde_json::json!({}),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    };
    let task2 = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: "https://example.com/2".to_string(),
        payload: serde_json::json!({}),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    };

    repo.create(&task1).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await; // Ensure different timestamps
    repo.create(&task2).await.unwrap();

    // Acquire first task (should be task1 due to higher priority)
    let acquired_task1 = repo.acquire_next(worker_id).await.unwrap().unwrap();
    assert_eq!(acquired_task1.id, task1.id);
    assert_eq!(acquired_task1.status, TaskStatus::Active);

    // Acquire second task
    let acquired_task2 = repo.acquire_next(worker_id).await.unwrap().unwrap();
    assert_eq!(acquired_task2.id, task2.id);
    assert_eq!(acquired_task2.status, TaskStatus::Active);

    // No more tasks to acquire
    let no_more_tasks = repo.acquire_next(worker_id).await.unwrap();
    assert!(no_more_tasks.is_none());
}
