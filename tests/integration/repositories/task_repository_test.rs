// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::super::helpers::{create_test_app, create_test_app_no_worker};
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
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(10),
    ));
    let team_id = Uuid::new_v4();
    let worker1_id = Uuid::new_v4();
    let worker2_id = Uuid::new_v4();

    // Clean up any existing tasks
    use sea_orm::EntityTrait;
    use crawlrs::infrastructure::database::entities::task as task_entity;
    task_entity::Entity::delete_many()
        .exec(app.db_pool.as_ref())
        .await
        .unwrap();

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

    println!("DEBUG: Worker 1 result: {:?}", result1.as_ref().map(|t| t.id));
    println!("DEBUG: Worker 2 result: {:?}", result2.as_ref().map(|t| t.id));

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
    let app = create_test_app_no_worker().await;
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

    repo.create(&task).await.unwrap();

    // Test mark_completed
    repo.mark_completed(task.id).await.unwrap();
    let completed_task = repo.find_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(completed_task.status, TaskStatus::Completed);
    assert!(completed_task.completed_at.is_some());

    // Test mark_failed
    let failed_task_id = Uuid::new_v4();
    let mut failed_task = task.clone();
    failed_task.id = failed_task_id;
    failed_task.status = TaskStatus::Active;
    repo.create(&failed_task).await.unwrap();
    
    repo.mark_failed(failed_task_id).await.unwrap();
    let found_failed_task = repo.find_by_id(failed_task_id).await.unwrap().unwrap();
    assert_eq!(found_failed_task.status, TaskStatus::Failed);

    // Test mark_cancelled
    let cancelled_task_id = Uuid::new_v4();
    let mut cancelled_task = task.clone();
    cancelled_task.id = cancelled_task_id;
    cancelled_task.status = TaskStatus::Queued;
    repo.create(&cancelled_task).await.unwrap();
    
    repo.mark_cancelled(cancelled_task_id).await.unwrap();
    let found_cancelled_task = repo.find_by_id(cancelled_task_id).await.unwrap().unwrap();
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
    assert!(!repo.exists_by_url(test_url).await.unwrap());

    // Create a task with the URL
    let task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: test_url.to_string(),
        payload: serde_json::json!({}),
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

    repo.create(&task).await.unwrap();

    // Now URL should exist
    assert!(repo.exists_by_url(test_url).await.unwrap());

    // Test with different URL
    assert!(!repo.exists_by_url("https://different-url.com").await.unwrap());
}

/// 测试重置卡住的任务
///
/// 验证reset_stuck_tasks方法是否能正确将长时间处于Active状态的任务重置为Queued。
///
/// 对应文档章节：3.3.6
#[tokio::test]
async fn test_reset_stuck_tasks() {
    let app = create_test_app().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(10));
    let team_id = Uuid::new_v4();

    // Create a stuck task (Active but old)
    let stuck_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Active,
        priority: 0,
        team_id,
        url: "https://example.com/stuck".to_string(),
        payload: serde_json::json!({}),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: (Utc::now() - chrono::Duration::hours(2)).into(),
        started_at: Some((Utc::now() - chrono::Duration::hours(1)).into()),
        completed_at: None,
        crawl_id: None,
        updated_at: (Utc::now() - chrono::Duration::hours(1)).into(),
        lock_token: None,
        lock_expires_at: None,
    };

    // Create a recent active task (should not be reset)
    let recent_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Active,
        priority: 0,
        team_id,
        url: "https://example.com/recent".to_string(),
        payload: serde_json::json!({}),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: Utc::now().into(),
        started_at: Some(Utc::now().into()),
        completed_at: None,
        crawl_id: None,
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    };

    repo.create(&stuck_task).await.unwrap();
    repo.create(&recent_task).await.unwrap();

    // Reset tasks that have been active for more than 30 minutes
    let reset_count = repo.reset_stuck_tasks(chrono::Duration::minutes(30)).await.unwrap();
    assert_eq!(reset_count, 1);

    // Verify the stuck task was reset
    let reset_task = repo.find_by_id(stuck_task.id).await.unwrap().unwrap();
    assert_eq!(reset_task.status, TaskStatus::Queued);

    // Verify the recent task was not reset
    let unchanged_task = repo.find_by_id(recent_task.id).await.unwrap().unwrap();
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
        repo.create(&task).await.unwrap();
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
    repo.create(&different_crawl_task).await.unwrap();

    // Cancel tasks by crawl_id
    let cancelled_count = repo.cancel_tasks_by_crawl_id(crawl_id).await.unwrap();
    assert_eq!(cancelled_count, 3);

    // Verify tasks with the target crawl_id were cancelled
    let tasks_by_crawl_id = repo.find_by_crawl_id(crawl_id).await.unwrap();
    assert_eq!(tasks_by_crawl_id.len(), 3);
    for task in tasks_by_crawl_id {
        assert_eq!(task.status, TaskStatus::Cancelled);
    }

    // Verify task with different crawl_id was not affected
    let unchanged_task = repo.find_by_id(different_crawl_task.id).await.unwrap().unwrap();
    assert_eq!(unchanged_task.status, TaskStatus::Queued);
}

/// 测试任务过期处理
///
/// 验证expire_tasks方法是否能正确将过期的任务标记为失败。
///
/// 对应文档章节：3.3.8
#[tokio::test]
async fn test_expire_tasks() {
    let app = create_test_app_no_worker().await;
    let repo = TaskRepositoryImpl::new(app.db_pool.clone(), chrono::Duration::seconds(10));
    let team_id = Uuid::new_v4();

    // Create an expired task (old created_at)
    let expired_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: "https://example.com/expired".to_string(),
        payload: serde_json::json!({}),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: (Utc::now() - chrono::Duration::days(2)).into(),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: (Utc::now() - chrono::Duration::days(2)).into(),
        lock_token: None,
        lock_expires_at: None,
    };

    // Create a recent task (should not be expired)
    let recent_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        url: "https://example.com/recent".to_string(),
        payload: serde_json::json!({}),
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

    repo.create(&expired_task).await.unwrap();
    repo.create(&recent_task).await.unwrap();

    // Expire tasks older than 1 day
    let expired_count = repo.expire_tasks().await.unwrap();
    assert_eq!(expired_count, 1);

    // Verify the expired task was marked as failed
    let expired_task_after = repo.find_by_id(expired_task.id).await.unwrap().unwrap();
    assert_eq!(expired_task_after.status, TaskStatus::Failed);

    // Verify the recent task was not affected
    let recent_task_after = repo.find_by_id(recent_task.id).await.unwrap().unwrap();
    assert_eq!(recent_task_after.status, TaskStatus::Queued);
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
        repo.create(&task).await.unwrap();
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
    repo.create(&different_crawl_task).await.unwrap();

    // Find tasks by crawl_id
    let found_tasks = repo.find_by_crawl_id(crawl_id).await.unwrap();
    assert_eq!(found_tasks.len(), 3);

    // Verify all found tasks have the correct crawl_id
    for task in &found_tasks {
        assert_eq!(task.crawl_id, Some(crawl_id));
        assert!(task_ids.contains(&task.id));
    }

    // Verify no tasks are found for non-existent crawl_id
    let no_tasks = repo.find_by_crawl_id(Uuid::new_v4()).await.unwrap();
    assert!(no_tasks.is_empty());
}
