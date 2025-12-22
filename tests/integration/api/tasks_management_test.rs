// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::json;
use uuid::Uuid;
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::TaskRepository;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use sea_orm::{Database, ConnectionTrait, Statement, DatabaseBackend};
use std::sync::Arc;
use chrono::{Utc, DateTime, FixedOffset};

async fn create_test_app_with_tasks() -> (TestServer, String, Uuid, Arc<TaskRepositoryImpl>) {
    use migration::{Migrator, MigratorTrait};
    
    let db = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    let db_pool = Arc::new(db);
    
    let api_key = Uuid::new_v4().to_string();
    let team_id = Uuid::new_v4();
    
    // 创建团队和API密钥
    db_pool.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, 'test-team', datetime('now'), datetime('now'))",
        vec![team_id.into()],
    )).await.unwrap();
    
    db_pool.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES (?, ?, ?, datetime('now'), datetime('now'))",
        vec![Uuid::new_v4().into(), api_key.clone().into(), team_id.into()],
    )).await.unwrap();
    
    let task_repo = Arc::new(TaskRepositoryImpl::new(db_pool.clone(), chrono::Duration::seconds(30)));
    let scrape_result_repo = Arc::new(ScrapeResultRepositoryImpl::new(db_pool.clone()));
    
    // 创建测试应用
    let app = crawlrs::presentation::routes::routes()
        .layer(axum::Extension(task_repo.clone()))
        .layer(axum::Extension(scrape_result_repo.clone()));
    
    let server = TestServer::new(app).unwrap();
    (server, api_key, team_id, task_repo)
}

async fn create_test_task(
    task_repo: &TaskRepositoryImpl,
    team_id: Uuid,
    task_type: TaskType,
    status: TaskStatus,
    url: &str,
) -> String {
    let task_id = Uuid::new_v4();
    let now = Utc::now();
    let fixed_now = DateTime::<FixedOffset>::from(now);
    
    let task = Task {
        id: task_id,
        task_type,
        status,
        priority: 0,
        team_id,
        url: url.to_string(),
        payload: serde_json::json!({}),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: fixed_now,
        started_at: if status == TaskStatus::Active { Some(fixed_now) } else { None },
        completed_at: if status == TaskStatus::Completed { Some(fixed_now) } else { None },
        crawl_id: None,
        updated_at: fixed_now,
        lock_token: None,
        lock_expires_at: None,
    };
    
    task_repo.create(&task).await.unwrap();
    task_id.to_string()
}

#[tokio::test]
async fn test_batch_task_query_basic() {
    let (server, api_key, team_id, task_repo) = create_test_app_with_tasks().await;
    
    // Given: 创建 3 个不同类型的任务
    let task_ids = vec![
        create_test_task(&task_repo, team_id, TaskType::Scrape, TaskStatus::Completed, "https://example1.com").await,
        create_test_task(&task_repo, team_id, TaskType::Extract, TaskStatus::Active, "https://example2.com").await,
        create_test_task(&task_repo, team_id, TaskType::Crawl, TaskStatus::Queued, "https://example3.com").await,
    ];
    
    // When: POST /v2/tasks/query
    let response = server
        .post("/v2/tasks/query")
        .add_header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "task_ids": task_ids,
            "team_id": team_id,
            "include_results": true
        }))
        .await;
    
    // Then: 返回所有任务
    assert_eq!(response.status_code(), StatusCode::OK);
    let body: serde_json::Value = response.json();
    println!("Response body: {}", serde_json::to_string_pretty(&body).unwrap());
    let tasks = body["data"]["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 3);
    
    // 验证任务类型和状态 (注意：查询结果按创建时间倒序排列，最新的在前)
    assert_eq!(tasks[0]["task_type"], "crawl");
    assert_eq!(tasks[0]["status"], "queued");
    assert_eq!(tasks[1]["task_type"], "extract");
    assert_eq!(tasks[1]["status"], "active");
    assert_eq!(tasks[2]["task_type"], "scrape");
    assert_eq!(tasks[2]["status"], "completed");
}

#[tokio::test]
async fn test_batch_task_query_with_filters() {
    let (server, api_key, team_id, task_repo) = create_test_app_with_tasks().await;
    
    // Given: 创建多个不同状态的任务
    create_test_task(&task_repo, team_id, TaskType::Scrape, TaskStatus::Completed, "https://completed.com").await;
    create_test_task(&task_repo, team_id, TaskType::Extract, TaskStatus::Failed, "https://failed.com").await;
    create_test_task(&task_repo, team_id, TaskType::Crawl, TaskStatus::Active, "https://active.com").await;
    
    // 获取所有任务ID
    let params = crawlrs::domain::repositories::task_repository::TaskQueryParams {
        team_id,
        limit: 100,
        offset: 0,
        ..Default::default()
    };
    let (all_tasks, _) = task_repo.query_tasks(params).await.unwrap();
    let task_ids: Vec<String> = all_tasks.iter().map(|t| t.id.to_string()).collect();
    
    // When: 只查询已完成和失败的任务
    let response = server
        .post("/v2/tasks/query")
        .add_header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "task_ids": task_ids,
            "team_id": team_id,
            "statuses": ["completed", "failed"]
        }))
        .await;
    
    // Then: 只返回过滤后的任务
    assert_eq!(response.status_code(), StatusCode::OK);
    let body: serde_json::Value = response.json();
    let tasks = body["data"]["tasks"].as_array().unwrap();
    
    // 应该只返回 completed 和 failed 状态的任务
    assert_eq!(tasks.len(), 2);
    let statuses: Vec<&str> = tasks.iter().map(|t| t["status"].as_str().unwrap()).collect();
    assert!(statuses.contains(&"completed"));
    assert!(statuses.contains(&"failed"));
    assert!(!statuses.contains(&"processing"));
}

#[tokio::test]
async fn test_batch_task_query_exclude_results() {
    let (server, api_key, team_id, task_repo) = create_test_app_with_tasks().await;
    
    // Given: 创建一个任务
    let task_id = create_test_task(&task_repo, team_id, TaskType::Scrape, TaskStatus::Completed, "https://example.com").await;
    
    // When: include_results=false
    let response = server
        .post("/v2/tasks/query")
        .add_header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "task_ids": [task_id.to_string()],
            "team_id": team_id,
            "include_results": false
        }))
        .await;
    
    // Then: 响应中不包含 result 字段
    assert_eq!(response.status_code(), StatusCode::OK);
    let body: serde_json::Value = response.json();
    let tasks = body["data"]["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 1);
    
    // 不应该包含结果数据
    assert!(tasks[0].get("result").is_none() || tasks[0]["result"].is_null());
}

#[tokio::test]
async fn test_batch_task_cancel_success() {
    let (server, api_key, team_id, task_repo) = create_test_app_with_tasks().await;
    
    // Given: 创建 3 个处理中的任务
    let task_ids = vec![
        create_test_task(&task_repo, team_id, TaskType::Scrape, TaskStatus::Active, "https://example1.com").await,
        create_test_task(&task_repo, team_id, TaskType::Extract, TaskStatus::Queued, "https://example2.com").await,
        create_test_task(&task_repo, team_id, TaskType::Crawl, TaskStatus::Queued, "https://example3.com").await,
    ];
    
    // When: DELETE /v2/tasks/cancel (使用force=true来取消active状态的任务)
    let response = server
        .delete("/v2/tasks/cancel")
        .add_header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "task_ids": task_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
            "team_id": team_id,
            "force": true
        }))
        .await;
    
    // Then: 所有任务被取消
    assert_eq!(response.status_code(), StatusCode::OK);
    let body: serde_json::Value = response.json();
    let cancelled_tasks = body["data"]["cancelled_tasks"].as_array().unwrap();
    let failed_tasks = body["data"]["failed_tasks"].as_array().unwrap();
    
    // 验证所有任务都被取消（3个取消，0个失败）
    assert_eq!(cancelled_tasks.len(), 3);
    assert_eq!(failed_tasks.len(), 0);
    assert_eq!(body["data"]["total_cancelled"].as_u64().unwrap(), 3);
    assert_eq!(body["data"]["total_failed"].as_u64().unwrap(), 0);
    
    // 验证任务状态已更新
    for task_id in &task_ids {
        let task_uuid = Uuid::parse_str(task_id).unwrap();
        let task = task_repo.find_by_id(task_uuid).await.unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::Cancelled);
    }
}

#[tokio::test]
async fn test_cancel_completed_task() {
    let (server, api_key, team_id, task_repo) = create_test_app_with_tasks().await;
    
    // Given: 已完成的任务
    let task_id = create_test_task(&task_repo, team_id, TaskType::Scrape, TaskStatus::Completed, "https://completed.com").await;
    
    // When: 尝试取消（不强制）
    let response = server
        .delete("/v2/tasks/cancel")
        .add_header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "task_ids": [task_id.to_string()],
            "team_id": team_id,
            "force": false
        }))
        .await;
    
    // Then: 取消失败，返回原因
    assert_eq!(response.status_code(), StatusCode::OK);
    let body: serde_json::Value = response.json();
    let cancelled_tasks = body["data"]["cancelled_tasks"].as_array().unwrap();
    let failed_tasks = body["data"]["failed_tasks"].as_array().unwrap();
    
    // 验证任务取消失败（0个取消，1个失败）
    assert_eq!(cancelled_tasks.len(), 0);
    assert_eq!(failed_tasks.len(), 1);
    assert_eq!(body["data"]["total_cancelled"].as_u64().unwrap(), 0);
    assert_eq!(body["data"]["total_failed"].as_u64().unwrap(), 1);
    
    // 验证失败原因
    assert!(failed_tasks[0]["reason"].as_str().unwrap().contains("already completed"));
}

#[tokio::test]
async fn test_force_cancel_completed_task() {
    let (server, api_key, team_id, task_repo) = create_test_app_with_tasks().await;
    
    // Given: 已完成的任务
    let task_id = create_test_task(&task_repo, team_id, TaskType::Scrape, TaskStatus::Completed, "https://completed.com").await;
    
    // When: 强制取消
    let response = server
        .delete("/v2/tasks/cancel")
        .add_header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "task_ids": [task_id.to_string()],
            "team_id": team_id,
            "force": true
        }))
        .await;
    
    // Then: 强制取消也失败，因为任务已完成
    assert_eq!(response.status_code(), StatusCode::OK);
    let body: serde_json::Value = response.json();
    let cancelled_tasks = body["data"]["cancelled_tasks"].as_array().unwrap();
    let failed_tasks = body["data"]["failed_tasks"].as_array().unwrap();
    
    // 验证任务强制取消失败（0个取消，1个失败）- 已完成的任务不能被取消，即使强制
    assert_eq!(cancelled_tasks.len(), 0);
    assert_eq!(failed_tasks.len(), 1);
    assert_eq!(body["data"]["total_cancelled"].as_u64().unwrap(), 0);
    assert_eq!(body["data"]["total_failed"].as_u64().unwrap(), 1);
    
    // 验证失败原因
    assert!(failed_tasks[0]["reason"].as_str().unwrap().contains("already completed"));
}

#[tokio::test]
async fn test_batch_operations_empty_list() {
    let (server, api_key, team_id, _) = create_test_app_with_tasks().await;
    
    // When: 查询空任务列表
    let response = server
        .post("/v2/tasks/query")
        .add_header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "task_ids": [],
            "team_id": team_id,
            "include_results": true
        }))
        .await;
    
    // Then: 返回空结果
    assert_eq!(response.status_code(), StatusCode::OK);
    let body: serde_json::Value = response.json();
    let tasks = body["data"]["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 0);
    
    // When: 尝试取消空任务列表 (应该返回验证错误)
    let response = server
        .delete("/v2/tasks/cancel")
        .add_header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "task_ids": [],
            "team_id": team_id
        }))
        .await;
    
    // Then: 应该返回验证错误 (400)，因为任务ID列表不能为空
    assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
}