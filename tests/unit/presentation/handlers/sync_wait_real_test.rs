// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use chrono::{DateTime, FixedOffset, Utc};
use std::time::Instant;
use uuid::Uuid;

use chrono::Duration;
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::TaskRepository;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::presentation::handlers::task_handler::wait_for_tasks_completion;
use migration::MigratorTrait;
use sea_orm::Database;
use std::sync::Arc;

fn create_test_task(id: Uuid, team_id: Uuid, status: TaskStatus) -> Task {
    let now = Utc::now();
    let fixed_now = DateTime::<FixedOffset>::from(now);

    Task {
        id,
        task_type: TaskType::Scrape,
        status,
        priority: 0,
        team_id,
        url: "https://example.com".to_string(),
        payload: serde_json::json!({}),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: fixed_now,
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: fixed_now,
        lock_token: None,
        lock_expires_at: None,
    }
}

#[tokio::test]
async fn test_sync_wait_returns_result_immediately_with_real_repo() {
    // Given: 设置真实的数据库和仓库
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let db_pool = Arc::new(db);

    // 运行迁移
    migration::Migrator::up(db_pool.as_ref(), None)
        .await
        .unwrap();

    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_pool.clone(),
        Duration::seconds(10),
    ));

    let task_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    // 创建一个已完成的任务
    let completed_task = create_test_task(task_id, team_id, TaskStatus::Completed);
    let _saved_task = task_repo.create(&completed_task).await.unwrap();

    // When: 调用同步等待函数
    let start = Instant::now();
    let result =
        wait_for_tasks_completion(task_repo.as_ref(), &[task_id], team_id, 5000, 1000).await;
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
async fn test_sync_wait_timeout_with_uncompleted_tasks_real_repo() {
    // Given: 设置真实的数据库和仓库
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let db_pool = Arc::new(db);

    // 运行迁移
    migration::Migrator::up(db_pool.as_ref(), None)
        .await
        .unwrap();

    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_pool.clone(),
        Duration::seconds(10),
    ));

    let task_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    // 创建一个未完成的任务
    let active_task = create_test_task(task_id, team_id, TaskStatus::Active);
    let _saved_task = task_repo.create(&active_task).await.unwrap();

    // When: 调用同步等待函数，设置较短的超时时间
    let start = Instant::now();
    let result = wait_for_tasks_completion(
        task_repo.as_ref(),
        &[task_id],
        team_id,
        1500, // 1.5秒超时
        500,  // 0.5秒轮询间隔
    )
    .await;
    let elapsed = start.elapsed();

    // Then: 应该在超时后返回成功（超时不是错误）
    assert!(result.is_ok());
    assert!(
        elapsed.as_millis() >= 1500,
        "Should wait for timeout, took {:?}",
        elapsed
    );
    assert!(
        elapsed.as_millis() < 2500,
        "Should not wait too long, took {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_sync_wait_multiple_tasks_real_repo() {
    // Given: 设置真实的数据库和仓库
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let db_pool = Arc::new(db);

    // 运行迁移
    migration::Migrator::up(db_pool.as_ref(), None)
        .await
        .unwrap();

    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_pool.clone(),
        Duration::seconds(10),
    ));

    let task_ids = vec![Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    let team_id = Uuid::new_v4();

    // 创建多个已完成的任务
    for &task_id in &task_ids {
        let completed_task = create_test_task(task_id, team_id, TaskStatus::Completed);
        let _saved_task = task_repo.create(&completed_task).await.unwrap();
    }

    // When: 调用同步等待函数
    let result =
        wait_for_tasks_completion(task_repo.as_ref(), &task_ids, team_id, 5000, 1000).await;

    // Then: 应该立即返回成功，因为所有任务都已完成
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_sync_wait_empty_task_list_real_repo() {
    // Given: 设置真实的数据库和仓库
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let db_pool = Arc::new(db);

    // 运行迁移
    migration::Migrator::up(db_pool.as_ref(), None)
        .await
        .unwrap();

    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_pool.clone(),
        Duration::seconds(10),
    ));

    let team_id = Uuid::new_v4();

    // When: 调用同步等待函数，传入空任务列表
    let result = wait_for_tasks_completion(task_repo.as_ref(), &[], team_id, 5000, 1000).await;

    // Then: 应该立即返回成功
    assert!(result.is_ok());
}
