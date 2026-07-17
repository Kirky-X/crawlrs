// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scrape result repository integration tests
//!
//! Integration tests for ScrapeResultRepositoryImpl using a real PostgreSQL database.
//! Covers save, find_by_task_id, find_by_task_ids, and get_team_avg_response_time.

use super::super::helpers::create_test_app_no_worker;
use crawlrs::domain::models::ScrapeResult;
use crawlrs::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crawlrs::infrastructure::database::entities::scrape_result as db_entity;
use crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use std::sync::Arc;
use uuid::Uuid;

/// 创建测试用的 ScrapeResult（辅助函数）
fn make_scrape_result(
    task_id: Uuid,
    url: &str,
    status_code: i32,
    response_time_ms: i64,
) -> ScrapeResult {
    ScrapeResult {
        id: Uuid::new_v4(),
        task_id,
        url: url.to_string(),
        status_code,
        content: format!("<html><body>{}</body></html>", url),
        content_type: "text/html".to_string(),
        headers: serde_json::json!({"content-type": "text/html"}),
        meta_data: serde_json::json!({"timestamp": "2025-01-01T00:00:00Z"}),
        screenshot: None,
        response_time_ms,
        created_at: chrono::Utc::now().naive_utc(),
    }
}

/// 测试保存爬取结果并通过 task_id 查询
#[tokio::test]
async fn test_save_and_find_by_task_id() {
    let app = create_test_app_no_worker().await;
    let repo = ScrapeResultRepositoryImpl::new(app.db_pool.clone());
    let task_id = Uuid::new_v4();
    let unique_url = format!("https://{}.example.com/page", Uuid::new_v4());

    let result = make_scrape_result(task_id, &unique_url, 200, 150);

    repo.save(result.clone())
        .await
        .expect("Failed to save scrape result");

    let found = repo
        .find_by_task_id(task_id)
        .await
        .expect("Failed to find by task_id");
    let found = found.expect("Scrape result should be found by task_id");
    assert_eq!(found.task_id, task_id);
    assert_eq!(found.url, unique_url);
    assert_eq!(found.status_code, 200);
    assert_eq!(found.response_time_ms, 150);
    assert_eq!(found.content_type, "text/html");

    cleanup_scrape_results(&app, task_id).await;
}

/// 测试通过不存在的 task_id 查询：应返回 None
#[tokio::test]
async fn test_find_by_task_id_returns_none_for_unknown() {
    let app = create_test_app_no_worker().await;
    let repo = ScrapeResultRepositoryImpl::new(app.db_pool.clone());

    let unknown_task_id = Uuid::new_v4();
    let result = repo
        .find_by_task_id(unknown_task_id)
        .await
        .expect("Failed to query unknown task_id");
    assert!(result.is_none(), "Should return None for unknown task_id");
}

/// 测试批量查询多个 task_id 的结果
#[tokio::test]
async fn test_find_by_task_ids_returns_multiple() {
    let app = create_test_app_no_worker().await;
    let repo = ScrapeResultRepositoryImpl::new(app.db_pool.clone());

    let task1_id = Uuid::new_v4();
    let task2_id = Uuid::new_v4();
    let task3_id = Uuid::new_v4();

    let r1 = make_scrape_result(task1_id, "https://a.example.com", 200, 100);
    let r2 = make_scrape_result(task2_id, "https://b.example.com", 404, 200);
    let r3 = make_scrape_result(task3_id, "https://c.example.com", 500, 300);

    repo.save(r1).await.expect("Failed to save r1");
    repo.save(r2).await.expect("Failed to save r2");
    repo.save(r3).await.expect("Failed to save r3");

    let results = repo
        .find_by_task_ids(&[task1_id, task2_id, task3_id])
        .await
        .expect("Failed to find by task_ids");

    assert!(
        results.len() >= 3,
        "Should return at least 3 results, got {}",
        results.len()
    );
    assert!(
        results.iter().any(|r| r.task_id == task1_id),
        "Result for task1 should be in results"
    );
    assert!(
        results.iter().any(|r| r.task_id == task2_id),
        "Result for task2 should be in results"
    );
    assert!(
        results.iter().any(|r| r.task_id == task3_id),
        "Result for task3 should be in results"
    );

    cleanup_scrape_results(&app, task1_id).await;
    cleanup_scrape_results(&app, task2_id).await;
    cleanup_scrape_results(&app, task3_id).await;
}

/// 测试空 task_id 列表查询：应返回空列表
#[tokio::test]
async fn test_find_by_task_ids_empty_input_returns_empty() {
    let app = create_test_app_no_worker().await;
    let repo = ScrapeResultRepositoryImpl::new(app.db_pool.clone());

    let results = repo
        .find_by_task_ids(&[])
        .await
        .expect("Failed to find by empty task_ids");

    assert!(
        results.is_empty(),
        "Should return empty list for empty input"
    );
}

/// 测试 get_team_avg_response_time：当前实现返回 0.0（占位）
#[tokio::test]
async fn test_get_team_avg_response_time_returns_zero() {
    let app = create_test_app_no_worker().await;
    let repo = ScrapeResultRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    // 当前实现是占位的，始终返回 0.0
    let avg = repo
        .get_team_avg_response_time(team_id)
        .await
        .expect("Failed to get team avg response time");

    assert_eq!(
        avg, 0.0,
        "Current implementation returns 0.0 as placeholder"
    );
}

/// 测试保存带 screenshot 的爬取结果
#[tokio::test]
async fn test_save_with_screenshot() {
    let app = create_test_app_no_worker().await;
    let repo = ScrapeResultRepositoryImpl::new(app.db_pool.clone());
    let task_id = Uuid::new_v4();

    let mut result = make_scrape_result(task_id, "https://screenshot.example.com", 200, 500);
    result.screenshot = Some("base64encodedscreenshot".to_string());

    repo.save(result.clone())
        .await
        .expect("Failed to save scrape result with screenshot");

    let found = repo
        .find_by_task_id(task_id)
        .await
        .expect("Failed to find by task_id");
    let found = found.expect("Scrape result should be found");
    assert_eq!(
        found.screenshot,
        Some("base64encodedscreenshot".to_string()),
        "Screenshot should be preserved"
    );

    cleanup_scrape_results(&app, task_id).await;
}

/// tc_pool_returns_database_pool: pool() getter 返回与构造时相同的 pool 引用
#[tokio::test]
async fn tc_pool_returns_database_pool() {
    let app = create_test_app_no_worker().await;
    let repo = ScrapeResultRepositoryImpl::new(app.db_pool.clone());

    let pool_ref = repo.pool();
    assert!(
        std::ptr::eq(Arc::as_ptr(pool_ref), Arc::as_ptr(&app.db_pool)),
        "pool() should return the same Arc<DbPool> passed to new()"
    );
}

/// tc_find_by_task_ids_with_partial_match: 部分 task_id 存在部分不存在时返回已存在的
#[tokio::test]
async fn tc_find_by_task_ids_with_partial_match() {
    let app = create_test_app_no_worker().await;
    let repo = ScrapeResultRepositoryImpl::new(app.db_pool.clone());

    let existing_task_id = Uuid::new_v4();
    let missing_task_id = Uuid::new_v4();
    let unique_url = format!("https://{}.example.com/partial", Uuid::new_v4());

    let result = make_scrape_result(existing_task_id, &unique_url, 200, 120);
    repo.save(result).await.expect("Failed to save result");

    let results = repo
        .find_by_task_ids(&[existing_task_id, missing_task_id])
        .await
        .expect("Failed to find by partial task_ids");

    assert_eq!(
        results.len(),
        1,
        "Should return only the existing task's result"
    );
    assert_eq!(
        results[0].task_id, existing_task_id,
        "Result should match the existing task"
    );

    cleanup_scrape_results(&app, existing_task_id).await;
}

/// 辅助函数：清理指定 task_id 的 scrape_results
async fn cleanup_scrape_results(app: &super::super::helpers::test_app::TestApp, task_id: Uuid) {
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session for cleanup");
    let conn = session
        .connection()
        .expect("Failed to get connection for cleanup");
    let _ = db_entity::Entity::delete_many()
        .filter(db_entity::Column::TaskId.eq(task_id))
        .exec(conn)
        .await;
}
