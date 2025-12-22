// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crawlrs::config::settings::Settings;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::queue::task_queue::{PostgresTaskQueue, TaskQueue};
use crawlrs::domain::models::task::{Task, TaskType};
use crawlrs::domain::models::task::TaskStatus;
use crawlrs::domain::repositories::task_repository::TaskRepository;
use sea_orm::{Database, DatabaseConnection};
use std::sync::Arc;
use uuid::Uuid;
use chrono::Duration;

/// 测试 PostgresTaskQueue 真实队列实现
///
/// 验证任务队列的真实数据库操作功能
#[tokio::test]
async fn test_postgres_task_queue_real_implementation() {
    // 使用内存 SQLite 数据库进行测试
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let db_pool = Arc::new(db);
    
    // 运行迁移
    migration::Migrator::up(db_pool.as_ref(), None).await.unwrap();
    
    // 创建任务仓库
    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_pool.clone(),
        Duration::seconds(10),
    ));
    
    // 创建真实的 PostgresTaskQueue
    let queue: Arc<dyn TaskQueue> = Arc::new(PostgresTaskQueue::new(task_repo.clone()));
    
    // 创建测试任务
    let task = Task {
        id: Uuid::new_v4(),
        crawl_id: None,
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        scheduled_at: None,
        started_at: None,
        completed_at: None,
        lock_token: None,
        lock_expires_at: None,
        expires_at: None,
    };
    
    // 测试任务入队
    let result = queue.enqueue(task.clone()).await;
    assert!(result.is_ok());
    
    // 验证任务已存储到数据库
    let stored_task = task_repo.find_by_id(&task.id).await.unwrap();
    assert!(stored_task.is_some());
    assert_eq!(stored_task.unwrap().id, task.id);
    
    // 测试任务出队
    let dequeued_task = queue.dequeue(Uuid::new_v4()).await.unwrap();
    assert!(dequeued_task.is_some());
    assert_eq!(dequeued_task.unwrap().id, task.id);
    
    // 测试完成任务
    let complete_result = queue.complete(task.id).await;
    assert!(complete_result.is_ok());
    
    // 验证任务状态已更新
    let completed_task = task_repo.find_by_id(&task.id).await.unwrap();
    assert!(completed_task.is_some());
    assert_eq!(completed_task.unwrap().status, TaskStatus::Completed);
}
