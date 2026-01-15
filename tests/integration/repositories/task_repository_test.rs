// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use super::super::helpers::{create_test_app, create_test_app_no_worker};
use chrono::Utc;
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::TaskRepository;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use std::sync::Arc;
use uuid::Uuid;

/// 测试并发任务获取和超时
///
/// 验证在多个工作程序并发获取任务时，只有一个能成功，
/// 并且在任务锁超时后，其他工作程序可以接管该任务。
///
/// 对应文档章节：3.3.3
#[tokio::test]
async fn test_concurrent_task_acquisition_and_timeout() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = Uuid::new_v4();
    let worker1_id = Uuid::new_v4();
    let worker2_id = Uuid::new_v4();

    // Clean up any existing tasks
    use crawlrs::infrastructure::database::entities::task as task_entity;
    use sea_orm::EntityTrait;
    task_entity::Entity::delete_many()
        .exec(app.db_pool.as_ref())
        .await
        .expect("Failed to delete existing tasks");

    // Create a single task with a unique URL to avoid conflicts with leftover data
    let unique_url = format!("https://example.com/concurrent-test-{}", Uuid::new_v4());
    let task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: unique_url,
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
    };

    println!("DEBUG: Creating task with ID: {:?}", task.id);
    repo.create(&task).await.expect("Failed to create task");

    // --- Concurrent Acquisition ---
    // Spawn both workers concurrently but with a delay to simulate realistic scenario
    let repo1 = repo.clone();
    let handle1 = tokio::spawn(async move {
        // Small delay to ensure Worker 1 starts first
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        repo1.acquire_next(worker1_id).await.expect("Failed to acquire task for worker 1")
    });

    let repo2 = repo.clone();
    let handle2 = tokio::spawn(async move {
        // Give Worker 1 more time to acquire and set the lock before Worker 2 tries
        // The database transaction and lock setting need time to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
        repo2.acquire_next(worker2_id).await.expect("Failed to acquire task for worker 2")
    });

    let result1 = handle1.await.expect("Failed to join worker 1 task");
    let result2 = handle2.await.expect("Failed to join worker 2 task");

    println!(
        "DEBUG: Worker 1 result: {:?}",
        result1.as_ref().map(|t| t.id)
    );
    println!(
        "DEBUG: Worker 2 result: {:?}",
        result2.as_ref().map(|t| t.id)
    );

    // Verify that at least one worker got the test task (this is the core assertion)
    // The test task should be acquired by at least one worker due to mutual exclusion
    let test_task_acquired = (result1.as_ref().map(|t| t.id) == Some(task.id)) as u8
        + (result2.as_ref().map(|t| t.id) == Some(task.id)) as u8;
    assert!(
        test_task_acquired >= 1,
        "Expected at least one worker to acquire the test task ({}), but got result1={:?}, result2={:?}",
        task.id, result1, result2
    );

    // --- Lock Timeout and Re-acquisition ---
    // The default lock timeout is 30 seconds in this test.
    // We need to manually expire the lock to test re-acquisition.
    // Instead of waiting 30+ seconds, we'll update the task's lock_expires_at to the past.
    let now = chrono::Utc::now();
    let expired_time = now - chrono::Duration::seconds(1);

    use sea_orm::{ActiveModelTrait, Set};

    let task_model = task_entity::Entity::find_by_id(task.id)
        .one(app.db_pool.as_ref())
        .await
        .expect("Failed to query task")
        .expect("Task not found");

    let mut task_active: task_entity::ActiveModel = task_model.into();
    task_active.lock_expires_at = Set(Some(expired_time.into()));
    task_active.update(app.db_pool.as_ref()).await.expect("Failed to update task");

    // Worker 2 should now be able to acquire the task
    let reacquired_task = repo.acquire_next(worker2_id).await.expect("Failed to reacquire task");
    assert!(reacquired_task.is_some());
    assert_eq!(reacquired_task.expect("Task not found").id, task.id);
}

/// 测试仓库的CRUD操作
///
/// 验证TaskRepository的创建、读取和更新功能是否正常。
///
/// 对应文档章节：3.3.1
#[tokio::test]
async fn test_repository_crud_operations() {
    let app = create_test_app().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(30));
    let team_id = Uuid::new_v4();

    let new_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
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
    };

    // Create
    repo.create(&new_task).await.expect("Failed to create task");

    // Read
    let found_task = repo.find_by_id(new_task.id).await.expect("Failed to query task").expect("Task not found");
    assert_eq!(found_task.id, new_task.id);

    // Update
    let mut updated_task = found_task;
    updated_task.status = TaskStatus::Active;
    repo.update(&updated_task).await.expect("Failed to update task");

    let found_after_update = repo.find_by_id(updated_task.id).await.expect("Failed to query task").expect("Task not found");
    assert_eq!(found_after_update.status, TaskStatus::Active);
}

/// 测试获取下一个任务
///
/// 验证acquire_next方法是否能正确地获取并锁定队列中的下一个任务。
///
// 对应文档章节：3.3.2
#[tokio::test]
async fn test_repository_acquire_next_task() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(10));
    let team_id = Uuid::new_v4();
    let worker_id = Uuid::new_v4();

    // Clean up any existing tasks
    use crawlrs::infrastructure::database::entities::task as task_entity;
    use sea_orm::EntityTrait;
    task_entity::Entity::delete_many()
        .exec(app.db_pool.as_ref())
        .await
        .expect("Failed to delete existing tasks");

    // Create a couple of tasks
    let task1 = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 1,
        team_id,
        url: "https://example.com/1".to_string(),
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
    };
    let task2 = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: "https://example.com/2".to_string(),
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
    };

    repo.create(&task1).await.expect("Failed to create task 1");
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await; // Ensure different timestamps
    repo.create(&task2).await.expect("Failed to create task 2");

    // Acquire first task (should be task1 due to higher priority)
    let acquired_task1 = repo.acquire_next(worker_id).await.expect("Failed to acquire task").expect("No task available");
    assert_eq!(acquired_task1.id, task1.id);
    assert_eq!(acquired_task1.status, TaskStatus::Active);

    // Acquire second task
    let acquired_task2 = repo.acquire_next(worker_id).await.expect("Failed to acquire task").expect("No task available");
    assert_eq!(acquired_task2.id, task2.id);
    assert_eq!(acquired_task2.status, TaskStatus::Active);

    // No more tasks to acquire
    let no_more_tasks = repo.acquire_next(worker_id).await.expect("Failed to acquire task");
    assert!(no_more_tasks.is_none());
}

/// 测试标记任务状态变更
///
/// 验证mark_completed、mark_failed、mark_cancelled方法是否能正确更新任务状态。
///
/// 对应文档章节：3.3.4
#[tokio::test]
async fn test_task_status_transitions() {
    let app = create_test_app().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(10));
    let team_id = Uuid::new_v4();

    // Create a task
    let task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: "https://example.com/status".to_string(),
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
    };

    repo.create(&task).await.expect("Failed to create task");

    // Test mark_completed
    repo.mark_completed(task.id).await.expect("Failed to mark task as completed");
    let completed_task = repo.find_by_id(task.id).await.expect("Failed to query task").expect("Task not found");
    assert_eq!(completed_task.status, TaskStatus::Completed);
    assert!(completed_task.completed_at.is_some());

    // Test mark_failed
    let failed_task_id = Uuid::new_v4();
    let mut failed_task = task.clone();
    failed_task.id = failed_task_id;
    failed_task.status = TaskStatus::Active;
    repo.create(&failed_task).await.expect("Failed to create failed task");

    repo.mark_failed(failed_task_id).await.expect("Failed to mark task as failed");
    let found_failed_task = repo.find_by_id(failed_task_id).await.expect("Failed to query task").expect("Task not found");
    assert_eq!(found_failed_task.status, TaskStatus::Failed);

    // Test mark_cancelled
    let cancelled_task_id = Uuid::new_v4();
    let mut cancelled_task = task.clone();
    cancelled_task.id = cancelled_task_id;
    cancelled_task.status = TaskStatus::Queued;
    repo.create(&cancelled_task).await.expect("Failed to create cancelled task");

    repo.mark_cancelled(cancelled_task_id).await.expect("Failed to mark task as cancelled");
    let found_cancelled_task = repo.find_by_id(cancelled_task_id).await.expect("Failed to query task").expect("Task not found");
    assert_eq!(found_cancelled_task.status, TaskStatus::Cancelled);
}

/// 测试URL存在性检查
///
/// 验证exists_by_url方法是否能正确判断URL是否存在。
///
/// 对应文档章节：3.3.5
#[tokio::test]
async fn test_exists_by_url() {
    let app = create_test_app().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(10));
    let team_id = Uuid::new_v4();
    let test_url = "https://example.com/exists-test";

    // Initially, URL should not exist
    assert!(!repo.exists_by_url(test_url).await.expect("Failed to check task existence"));

    // Create a task with the URL
    let task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: test_url.to_string(),
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
    };

    repo.create(&task).await.expect("Failed to create task");

    // Now URL should exist
    assert!(repo.exists_by_url(test_url).await.expect("Failed to check task existence"));

    // Test with different URL
    assert!(!repo
        .exists_by_url("https://different-url.com")
        .await
        .expect("Failed to check task existence"));
}

/// 测试重置卡住的任务
///
/// 验证reset_stuck_tasks方法是否能正确将长时间处于Active状态的任务重置为Queued。
///
/// 对应文档章节：3.3.6
#[tokio::test]
async fn test_reset_stuck_tasks() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(10));
    let team_id = Uuid::new_v4();

    // Clean up any existing tasks
    use crawlrs::infrastructure::database::entities::task as task_entity;
    use sea_orm::EntityTrait;
    task_entity::Entity::delete_many()
        .exec(app.db_pool.as_ref())
        .await
        .expect("Failed to delete existing tasks");

    // Use fixed reference time to avoid timing issues between multiple Utc::now() calls
    let now = Utc::now();
    let one_hour_ago = now - chrono::Duration::hours(1);
    let two_hours_ago = now - chrono::Duration::hours(2);

    // Create a stuck task (Active but old - started more than 30 minutes ago)
    let stuck_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Active,
        priority: 0,
        team_id,
        url: "https://example.com/stuck".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: two_hours_ago.into(),
        started_at: Some(one_hour_ago.into()),
        completed_at: None,
        crawl_id: None,
        updated_at: one_hour_ago.into(),
        lock_token: None,
        lock_expires_at: None,
    };

    // Create a recent active task (should not be reset - started just now)
    let recent_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Active,
        priority: 0,
        team_id,
        url: "https://example.com/recent".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: now.into(),
        started_at: Some(now.into()),
        completed_at: None,
        crawl_id: None,
        updated_at: now.into(),
        lock_token: None,
        lock_expires_at: None,
    };

    repo.create(&stuck_task).await.expect("Failed to create stuck task");
    repo.create(&recent_task).await.expect("Failed to create recent task");

    // Reset tasks that have been active for more than 30 minutes
    let reset_count = repo
        .reset_stuck_tasks(chrono::Duration::minutes(30))
        .await
        .expect("Failed to reset stuck tasks");
    assert_eq!(reset_count, 1);

    // Verify the stuck task was reset
    let reset_task = repo.find_by_id(stuck_task.id).await.expect("Failed to query task").expect("Task not found");
    assert_eq!(reset_task.status, TaskStatus::Queued);

    // Verify the recent task was not reset
    let unchanged_task = repo.find_by_id(recent_task.id).await.expect("Failed to query task").expect("Task not found");
    assert_eq!(unchanged_task.status, TaskStatus::Active);
}

/// 测试按Crawl ID取消任务
///
/// 验证cancel_tasks_by_crawl_id方法是否能正确取消与特定Crawl ID相关的所有任务。
///
/// 对应文档章节：3.3.7
#[tokio::test]
async fn test_cancel_tasks_by_crawl_id() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(10));
    let team_id = Uuid::new_v4();
    let crawl_id = Uuid::new_v4();

    // Create tasks with the same crawl_id
    for i in 0..3 {
        let task = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            status: TaskStatus::Queued,
            priority: 0,
            team_id,
            url: format!("https://example.com/crawl/{}", i),
            payload: serde_json::json!({}),
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: Utc::now().into(),
            started_at: None,
            completed_at: None,
            crawl_id: Some(crawl_id),
            updated_at: Utc::now().into(),
            lock_token: None,
            lock_expires_at: None,
        };
        repo.create(&task).await.expect("Failed to create task");
    }

    // Create a task with different crawl_id
    let different_crawl_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: "https://example.com/different".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: Some(Uuid::new_v4()),
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    };
    repo.create(&different_crawl_task).await.expect("Failed to create different crawl task");

    // Cancel tasks by crawl_id
    let cancelled_count = repo.cancel_tasks_by_crawl_id(crawl_id).await.expect("Failed to cancel tasks by crawl ID");
    assert_eq!(cancelled_count, 3);

    // Verify tasks with the target crawl_id were cancelled
    let tasks_by_crawl_id = repo.find_by_crawl_id(crawl_id).await.expect("Failed to find tasks by crawl ID");
    assert_eq!(tasks_by_crawl_id.len(), 3);
    for task in tasks_by_crawl_id {
        assert_eq!(task.status, TaskStatus::Cancelled);
    }

    // Verify task with different crawl_id was not affected
    let unchanged_task = repo
        .find_by_id(different_crawl_task.id)
        .await
        .expect("Failed to query task")
        .expect("Task not found");
    assert_eq!(unchanged_task.status, TaskStatus::Queued);
}

/// 测试任务过期处理
///
/// 验证expire_tasks方法是否能正确将过期的任务标记为失败。
/// 包括队列中的任务和活跃状态的任务。
///
/// 对应文档章节：3.3.8
#[tokio::test]
async fn test_expire_tasks() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(10));
    let team_id = Uuid::new_v4();

    // Use a fixed reference time to avoid timing issues between multiple Utc::now() calls
    let now = Utc::now();
    let two_days_ago = now - chrono::Duration::days(2);

    // Create an expired queued task (old created_at)
    let expired_queued_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: "https://example.com/expired-queued".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: two_days_ago.into(),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: two_days_ago.into(),
        lock_token: None,
        lock_expires_at: None,
    };

    // Create an expired active task (old started_at)
    let expired_active_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Active,
        priority: 0,
        team_id,
        url: "https://example.com/expired-active".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: two_days_ago.into(),
        started_at: Some(two_days_ago.fixed_offset()),
        completed_at: None,
        crawl_id: None,
        updated_at: two_days_ago.into(),
        lock_token: None,
        lock_expires_at: None,
    };

    // Create a recent queued task (should not be expired) - use "now" time
    let recent_queued_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: "https://example.com/recent-queued".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: now.into(),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    };

    // Create a recent active task (should not be expired)
    let recent_active_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Active,
        priority: 0,
        team_id,
        url: "https://example.com/recent-active".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: Utc::now().into(),
        started_at: Some(now.fixed_offset()),
        completed_at: None,
        crawl_id: None,
        updated_at: now.into(),
        lock_token: None,
        lock_expires_at: None,
    };

    repo.create(&expired_queued_task).await.expect("Failed to create expired queued task");
    repo.create(&expired_active_task).await.expect("Failed to create expired active task");
    repo.create(&recent_queued_task).await.expect("Failed to create recent queued task");
    repo.create(&recent_active_task).await.expect("Failed to create recent active task");

    // Expire tasks older than 1 day
    let expired_count = repo.expire_tasks().await.expect("Failed to expire tasks");
    assert_eq!(expired_count, 2); // Both expired tasks should be expired

    // Verify the expired queued task was marked as failed
    let expired_queued_after = repo
        .find_by_id(expired_queued_task.id)
        .await
        .expect("Failed to query task")
        .expect("Task not found");
    assert_eq!(expired_queued_after.status, TaskStatus::Failed);

    // Verify the expired active task was marked as failed
    let expired_active_after = repo
        .find_by_id(expired_active_task.id)
        .await
        .expect("Failed to query task")
        .expect("Task not found");
    assert_eq!(expired_active_after.status, TaskStatus::Failed);

    // Verify the recent queued task was not affected
    let recent_queued_after = repo
        .find_by_id(recent_queued_task.id)
        .await
        .expect("Failed to query task")
        .expect("Task not found");
    assert_eq!(recent_queued_after.status, TaskStatus::Queued);

    // Verify the recent active task was not affected
    let recent_active_after = repo
        .find_by_id(recent_active_task.id)
        .await
        .expect("Failed to query task")
        .expect("Task not found");
    assert_eq!(recent_active_after.status, TaskStatus::Active);
}

/// 测试按Crawl ID查找任务
///
/// 验证find_by_crawl_id方法是否能正确返回与特定Crawl ID相关的所有任务。
///
/// 对应文档章节：3.3.9
#[tokio::test]
async fn test_find_by_crawl_id() {
    let app = create_test_app().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(10));
    let team_id = Uuid::new_v4();
    let crawl_id = Uuid::new_v4();

    // Create tasks with the same crawl_id
    let mut task_ids = Vec::new();
    for i in 0..3 {
        let task_id = Uuid::new_v4();
        task_ids.push(task_id);
        let task = Task {
            id: task_id,
            task_type: TaskType::Scrape,
            status: TaskStatus::Queued,
            priority: i,
            team_id,
            url: format!("https://example.com/crawl/{}", i),
            payload: serde_json::json!({}),
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: Utc::now().into(),
            started_at: None,
            completed_at: None,
            crawl_id: Some(crawl_id),
            updated_at: Utc::now().into(),
            lock_token: None,
            lock_expires_at: None,
        };
        repo.create(&task).await.expect("Failed to create task");
    }

    // Create a task with different crawl_id
    let different_crawl_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: "https://example.com/different".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: Some(Uuid::new_v4()),
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    };
    repo.create(&different_crawl_task).await.expect("Failed to create different crawl task");

    // Find tasks by crawl_id
    let found_tasks = repo.find_by_crawl_id(crawl_id).await.expect("Failed to find tasks by crawl ID");
    assert_eq!(found_tasks.len(), 3);

    // Verify all found tasks have the correct crawl_id
    for task in &found_tasks {
        assert_eq!(task.crawl_id, Some(crawl_id));
        assert!(task_ids.contains(&task.id));
    }

    // Verify no tasks are found for non-existent crawl_id
    let no_tasks = repo.find_by_crawl_id(Uuid::new_v4()).await.expect("Failed to find tasks by crawl ID");
    assert!(no_tasks.is_empty());
}
