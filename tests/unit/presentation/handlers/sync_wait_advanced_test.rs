// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use std::time::Duration;
use tokio::time::{sleep, Instant};
use crawlrs::presentation::handlers::task_handler::wait_for_tasks_completion;
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::TaskRepository;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use chrono::{DateTime, FixedOffset, Utc};
use uuid::Uuid;
use std::sync::Arc;

fn create_test_task_with_status(id: Uuid, team_id: Uuid, status: TaskStatus, task_type: TaskType) -> Task {
    let now = Utc::now();
    let fixed_now = DateTime::<FixedOffset>::from(now);

    Task {
        id,
        task_type,
        status,
        priority: 0,
        team_id,
        url: format!("https://example.com/{}", id),
        payload: serde_json::json!({}),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: fixed_now,
        started_at: if status == TaskStatus::Processing { Some(fixed_now) } else { None },
        completed_at: if status == TaskStatus::Completed { Some(fixed_now) } else { None },
        crawl_id: None,
        updated_at: fixed_now,
        lock_token: None,
        lock_expires_at: None,
    }
}

async fn setup_test_repo() -> TaskRepositoryImpl {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    use migration::{Migrator, MigratorTrait};
    Migrator::up(&db, None).await.unwrap();
    TaskRepositoryImpl::new(Arc::new(db), chrono::Duration::seconds(30))
}

#[tokio::test]
async fn test_sync_wait_smart_logic_immediate() {
    // Given: 任务会在 1 秒内完成
    let repo = setup_test_repo().await;
    let task_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    
    let completed_task = create_test_task_with_status(task_id, team_id, TaskStatus::Completed, TaskType::Scrape);
    repo.create(&completed_task).await.unwrap();

    // When: 调用同步等待函数，设置 5 秒等待时间
    let start = Instant::now();
    let result = wait_for_tasks_completion(&repo, &[task_id], team_id, 5000, 100).await;
    let elapsed = start.elapsed();

    // Then: 应该立即返回成功（任务已完成）
    assert!(result.is_ok());
    assert!(elapsed.as_millis() < 500, "Should complete immediately, took {:?}", elapsed);
}

#[tokio::test]
async fn test_sync_wait_smart_logic_timeout() {
    // Given: 任务需要较长时间完成
    let repo = setup_test_repo().await;
    let task_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    
    let processing_task = create_test_task_with_status(task_id, team_id, TaskStatus::Processing, TaskType::Scrape);
    repo.create(&processing_task).await.unwrap();

    // When: 调用同步等待函数，设置较短的超时时间
    let start = Instant::now();
    let result = wait_for_tasks_completion(&repo, &[task_id], team_id, 1000, 200).await;
    let elapsed = start.elapsed();

    // Then: 应该在超时后返回成功（超时不是错误）
    assert!(result.is_ok());
    assert!(elapsed.as_millis() >= 1000, "Should wait for timeout, took {:?}", elapsed);
}

#[tokio::test]
async fn test_sync_wait_multiple_tasks_mixed_status() {
    // Given: 创建多个不同状态的任务
    let repo = setup_test_repo().await;
    let team_id = Uuid::new_v4();
    let task_ids = vec![Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];

    // 创建：1个已完成，1个处理中，1个排队
    let completed_task = create_test_task_with_status(task_ids[0], team_id, TaskStatus::Completed, TaskType::Scrape);
    let processing_task = create_test_task_with_status(task_ids[1], team_id, TaskStatus::Processing, TaskType::Scrape);
    let queued_task = create_test_task_with_status(task_ids[2], team_id, TaskStatus::Queued, TaskType::Scrape);
    
    repo.create(&completed_task).await.unwrap();
    repo.create(&processing_task).await.unwrap();
    repo.create(&queued_task).await.unwrap();

    // When: 调用同步等待函数，设置较长等待时间
    let start = Instant::now();
    let result = wait_for_tasks_completion(&repo, &task_ids, team_id, 3000, 500).await;
    let elapsed = start.elapsed();

    // Then: 应该返回成功，但不会等待所有任务完成（因为有些还在处理中）
    assert!(result.is_ok());
    // 由于有任务还在处理，应该接近超时时间
    assert!(elapsed.as_millis() >= 2500, "Should wait for most of the timeout, took {:?}", elapsed);
}

#[tokio::test]
async fn test_sync_wait_default_behavior() {
    // Given: 创建一个新任务
    let repo = setup_test_repo().await;
    let task_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    
    let queued_task = create_test_task_with_status(task_id, team_id, TaskStatus::Queued, TaskType::Scrape);
    repo.create(&queued_task).await.unwrap();

    // When: 调用同步等待函数，使用默认参数
    let start = Instant::now();
    let result = wait_for_tasks_completion(&repo, &[task_id], team_id, 2000, 100).await;
    let elapsed = start.elapsed();

    // Then: 应该等待直到超时（因为任务还在排队）
    assert!(result.is_ok());
    assert!(elapsed.as_millis() >= 1800, "Should wait for timeout, took {:?}", elapsed);
}

#[tokio::test]
async fn test_sync_wait_task_type_variations() {
    // Given: 创建不同类型但相同状态的任务
    let repo = setup_test_repo().await;
    let team_id = Uuid::new_v4();
    let scrape_task_id = Uuid::new_v4();
    let search_task_id = Uuid::new_v4();
    let crawl_task_id = Uuid::new_v4();

    let scrape_task = create_test_task_with_status(scrape_task_id, team_id, TaskStatus::Completed, TaskType::Scrape);
    let search_task = create_test_task_with_status(search_task_id, team_id, TaskStatus::Completed, TaskType::Search);
    let crawl_task = create_test_task_with_status(crawl_task_id, team_id, TaskStatus::Completed, TaskType::Crawl);
    
    repo.create(&scrape_task).await.unwrap();
    repo.create(&search_task).await.unwrap();
    repo.create(&crawl_task).await.unwrap();

    // When: 调用同步等待函数，等待所有任务
    let start = Instant::now();
    let result = wait_for_tasks_completion(&repo, &[scrape_task_id, search_task_id, crawl_task_id], team_id, 3000, 100).await;
    let elapsed = start.elapsed();

    // Then: 应该立即返回成功（所有任务都已完成）
    assert!(result.is_ok());
    assert!(elapsed.as_millis() < 500, "Should complete immediately, took {:?}", elapsed);
}