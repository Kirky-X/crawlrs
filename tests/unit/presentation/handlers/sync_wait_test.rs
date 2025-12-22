// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use chrono::{DateTime, FixedOffset, Utc};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::TaskRepository;
use crawlrs::presentation::handlers::task_handler::wait_for_tasks_completion;

use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use sea_orm::Database;

fn create_test_task(id: Uuid, team_id: Uuid, status: TaskStatus) -> Task {
    let now = Utc::now();
    let fixed_now = DateTime::<FixedOffset>::from(now);

    Task {
        id,
        task_type: TaskType::Scrape,
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
        started_at: None,
        completed_at: if status == TaskStatus::Completed {
            Some(fixed_now)
        } else {
            None
        },
        crawl_id: None,
        updated_at: fixed_now,
        lock_token: None,
        lock_expires_at: None,
    }
}

async fn setup_test_repo() -> TaskRepositoryImpl {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    // 运行迁移以创建表
    use migration::{Migrator, MigratorTrait};
    Migrator::up(&db, None).await.unwrap();
    TaskRepositoryImpl::new(Arc::new(db), chrono::Duration::seconds(30))
}

#[tokio::test]
async fn test_sync_wait_returns_result_immediately() {
    // Given: 创建一个立即完成的任务
    let repo = setup_test_repo().await;
    let task_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    let completed_task = create_test_task(task_id, team_id, TaskStatus::Completed);
    repo.create(&completed_task).await.unwrap();

    // When: 调用同步等待函数
    let start = Instant::now();
    let result = wait_for_tasks_completion(&repo, &[task_id], team_id, 5000, 100).await;
    let elapsed = start.elapsed();

    // Then: 应该立即返回成功
    assert!(result.is_ok());
    assert!(
        elapsed.as_millis() < 1000,
        "Should complete immediately, took {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_sync_wait_timeout_with_uncompleted_tasks() {
    // Given: 创建一个未完成的任务
    let repo = setup_test_repo().await;
    let task_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    let active_task = create_test_task(task_id, team_id, TaskStatus::Active);
    repo.create(&active_task).await.unwrap();

    // When: 调用同步等待函数，设置较短的超时时间
    let start = Instant::now();
    let result = wait_for_tasks_completion(
        &repo,
        &[task_id],
        team_id,
        1000, // 1秒超时
        200,  // 0.2秒轮询间隔
    )
    .await;
    let elapsed = start.elapsed();

    // Then: 应该在超时后返回成功（超时不是错误）
    assert!(result.is_ok());
    assert!(
        elapsed.as_millis() >= 1000,
        "Should wait for timeout, took {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_sync_wait_multiple_tasks() {
    // Given: 创建多个任务
    let repo = setup_test_repo().await;
    let team_id = Uuid::new_v4();
    let task_ids = vec![Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];

    for &id in &task_ids {
        let completed_task = create_test_task(id, team_id, TaskStatus::Completed);
        repo.create(&completed_task).await.unwrap();
    }

    // When: 调用同步等待函数
    let result = wait_for_tasks_completion(&repo, &task_ids, team_id, 5000, 100).await;

    // Then: 应该立即返回成功，因为所有任务都已完成
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_sync_wait_empty_task_list() {
    // Given: 空任务列表
    let repo = setup_test_repo().await;
    let team_id = Uuid::new_v4();

    // When: 调用同步等待函数，传入空任务列表
    let result = wait_for_tasks_completion(&repo, &[], team_id, 5000, 100).await;

    // Then: 应该立即返回成功
    assert!(result.is_ok());
}
