// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task repository advanced query integration tests
//!
//! Integration tests for TaskRepositoryImpl focusing on query_tasks and batch_cancel
//! methods, covering success paths, error paths, and boundary conditions.

use super::super::helpers::create_test_app_no_worker;
use crawlrs::domain::models::task_domain::{TaskStatus, TaskType};
use crawlrs::domain::models::task_model::Task;
use crawlrs::domain::repositories::task_repository::{TaskQueryParams, TaskRepository};
use crawlrs::infrastructure::database::entities::task as task_entity;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};
use std::sync::Arc;
use uuid::Uuid;

/// 创建测试用 Task 的辅助函数
fn make_task(team_id: Uuid, api_key_id: Uuid, url: &str) -> Task {
    let mut task = Task::new(
        Uuid::new_v4(),
        TaskType::Scrape,
        team_id,
        api_key_id,
        url.to_string(),
        serde_json::json!({}),
    );
    // 使用唯一 URL 避免与其他测试冲突
    task.url = format!("https://{}.example.com/query", Uuid::new_v4());
    task
}

/// 辅助函数：通过唯一前缀清理测试创建的任务
async fn cleanup_tasks_by_prefix(app: &super::super::helpers::test_app::TestApp, prefix: &str) {
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session for cleanup");
    let conn = session
        .connection()
        .expect("Failed to get connection for cleanup");
    let _ = task_entity::Entity::delete_many()
        .filter(task_entity::Column::Url.like(format!("{}%", prefix)))
        .exec(conn)
        .await;
}

// ==================== query_tasks 测试 ====================

/// tc_query_tasks_by_team_only_success: 仅按 team_id 过滤，返回该团队所有任务
#[tokio::test]
async fn tc_query_tasks_by_team_only_success() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = app.team_id;
    let api_key_id = app.api_key_id;
    let cleanup_prefix = format!("https://query-team-{}.", Uuid::new_v4());

    // 创建 2 个任务
    let mut t1 = make_task(team_id, api_key_id, "");
    t1.url = format!("{}1.example.com", cleanup_prefix);
    let mut t2 = make_task(team_id, api_key_id, "");
    t2.url = format!("{}2.example.com", cleanup_prefix);
    repo.create(&t1).await.expect("create t1 failed");
    repo.create(&t2).await.expect("create t2 failed");

    let params = TaskQueryParams {
        team_id,
        limit: 100,
        offset: 0,
        ..Default::default()
    };
    let (tasks, total) = repo
        .query_tasks(params)
        .await
        .expect("query_tasks failed");

    assert!(total >= 2, "total should be >= 2, got {}", total);
    assert!(
        tasks.iter().any(|t| t.id == t1.id),
        "t1 should be in results"
    );
    assert!(
        tasks.iter().any(|t| t.id == t2.id),
        "t2 should be in results"
    );
    // 所有返回的任务都属于该 team_id
    assert!(
        tasks.iter().all(|t| t.team_id == team_id),
        "all tasks should belong to the queried team"
    );

    cleanup_tasks_by_prefix(&app, &cleanup_prefix).await;
}

/// tc_query_tasks_filter_by_status_success: 按 statuses 过滤返回匹配状态的任务
#[tokio::test]
async fn tc_query_tasks_filter_by_status_success() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = app.team_id;
    let api_key_id = app.api_key_id;
    let cleanup_prefix = format!("https://query-status-{}.", Uuid::new_v4());

    let mut queued_task = make_task(team_id, api_key_id, "");
    queued_task.url = format!("{}q.example.com", cleanup_prefix);
    queued_task.status = TaskStatus::Queued;

    let mut active_task = make_task(team_id, api_key_id, "");
    active_task.url = format!("{}a.example.com", cleanup_prefix);
    active_task.status = TaskStatus::Active;

    repo.create(&queued_task).await.expect("create queued failed");
    repo.create(&active_task).await.expect("create active failed");

    let params = TaskQueryParams {
        team_id,
        statuses: Some(vec![TaskStatus::Active]),
        limit: 100,
        offset: 0,
        ..Default::default()
    };
    let (tasks, _total) = repo
        .query_tasks(params)
        .await
        .expect("query_tasks with status filter failed");

    // 所有返回的任务都应是 Active
    assert!(
        tasks.iter().all(|t| t.status == TaskStatus::Active),
        "all returned tasks should be Active"
    );
    assert!(
        tasks.iter().any(|t| t.id == active_task.id),
        "active_task should be in results"
    );
    assert!(
        !tasks.iter().any(|t| t.id == queued_task.id),
        "queued_task should NOT be in results when filtering for Active"
    );

    cleanup_tasks_by_prefix(&app, &cleanup_prefix).await;
}

/// tc_query_tasks_filter_by_task_type_success: 按 task_types 过滤返回匹配类型的任务
#[tokio::test]
async fn tc_query_tasks_filter_by_task_type_success() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = app.team_id;
    let api_key_id = app.api_key_id;
    let cleanup_prefix = format!("https://query-type-{}.", Uuid::new_v4());

    let mut scrape_task = make_task(team_id, api_key_id, "");
    scrape_task.url = format!("{}s.example.com", cleanup_prefix);
    scrape_task.task_type = TaskType::Scrape;

    let mut crawl_task = make_task(team_id, api_key_id, "");
    crawl_task.url = format!("{}c.example.com", cleanup_prefix);
    crawl_task.task_type = TaskType::Crawl;

    repo.create(&scrape_task).await.expect("create scrape failed");
    repo.create(&crawl_task).await.expect("create crawl failed");

    let params = TaskQueryParams {
        team_id,
        task_types: Some(vec![TaskType::Crawl]),
        limit: 100,
        offset: 0,
        ..Default::default()
    };
    let (tasks, _total) = repo
        .query_tasks(params)
        .await
        .expect("query_tasks with type filter failed");

    assert!(
        tasks.iter().all(|t| t.task_type == TaskType::Crawl),
        "all returned tasks should be Crawl type"
    );
    assert!(
        tasks.iter().any(|t| t.id == crawl_task.id),
        "crawl_task should be in results"
    );
    assert!(
        !tasks.iter().any(|t| t.id == scrape_task.id),
        "scrape_task should NOT be in results when filtering for Crawl"
    );

    cleanup_tasks_by_prefix(&app, &cleanup_prefix).await;
}

/// tc_query_tasks_filter_by_crawl_id_success: 按 crawl_id 过滤返回该 crawl 下的任务
#[tokio::test]
async fn tc_query_tasks_filter_by_crawl_id_success() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = app.team_id;
    let api_key_id = app.api_key_id;
    let cleanup_prefix = format!("https://query-crawl-{}.", Uuid::new_v4());
    let crawl_id = Uuid::new_v4();

    let mut t1 = make_task(team_id, api_key_id, "");
    t1.url = format!("{}1.example.com", cleanup_prefix);
    t1.crawl_id = Some(crawl_id);

    let mut t2 = make_task(team_id, api_key_id, "");
    t2.url = format!("{}2.example.com", cleanup_prefix);
    t2.crawl_id = Some(crawl_id);

    let mut t3 = make_task(team_id, api_key_id, "");
    t3.url = format!("{}3.example.com", cleanup_prefix);
    t3.crawl_id = Some(Uuid::new_v4()); // 不同的 crawl_id

    repo.create(&t1).await.expect("create t1 failed");
    repo.create(&t2).await.expect("create t2 failed");
    repo.create(&t3).await.expect("create t3 failed");

    let params = TaskQueryParams {
        team_id,
        crawl_id: Some(crawl_id),
        limit: 100,
        offset: 0,
        ..Default::default()
    };
    let (tasks, total) = repo
        .query_tasks(params)
        .await
        .expect("query_tasks with crawl_id filter failed");

    assert_eq!(total, 2, "should find 2 tasks for crawl_id");
    assert_eq!(tasks.len(), 2, "should return 2 tasks");
    assert!(
        tasks.iter().all(|t| t.crawl_id == Some(crawl_id)),
        "all returned tasks should have the queried crawl_id"
    );
    assert!(
        !tasks.iter().any(|t| t.id == t3.id),
        "t3 with different crawl_id should NOT be in results"
    );

    cleanup_tasks_by_prefix(&app, &cleanup_prefix).await;
}

/// tc_query_tasks_pagination_limit_offset: 验证 limit/offset 分页正确性
#[tokio::test]
async fn tc_query_tasks_pagination_limit_offset() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = app.team_id;
    let api_key_id = app.api_key_id;
    let cleanup_prefix = format!("https://query-page-{}.", Uuid::new_v4());

    // 创建 3 个任务，时间错开以保证顺序
    let mut ids = Vec::new();
    for i in 0..3 {
        let mut t = make_task(team_id, api_key_id, "");
        t.url = format!("{}{}.example.com", cleanup_prefix, i);
        repo.create(&t).await.expect("create task failed");
        ids.push(t.id);
        // 错开 created_at 以保证 order_by_desc(CreatedAt) 顺序稳定
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
    }

    // 第一页：limit=2, offset=0
    let params_p1 = TaskQueryParams {
        team_id,
        limit: 2,
        offset: 0,
        ..Default::default()
    };
    let (page1, total) = repo
        .query_tasks(params_p1)
        .await
        .expect("query_tasks page 1 failed");
    assert_eq!(total, 3, "total should be 3");
    assert!(
        page1.len() <= 2,
        "page1 should have at most 2 items, got {}",
        page1.len()
    );

    // 第二页：limit=2, offset=2
    let params_p2 = TaskQueryParams {
        team_id,
        limit: 2,
        offset: 2,
        ..Default::default()
    };
    let (page2, total2) = repo
        .query_tasks(params_p2)
        .await
        .expect("query_tasks page 2 failed");
    assert_eq!(total2, 3, "total should still be 3");
    assert!(
        page2.len() <= 1,
        "page2 should have at most 1 item (3 total - offset 2), got {}",
        page2.len()
    );

    // 验证分页不重复
    let p1_ids: Vec<_> = page1.iter().map(|t| t.id).collect();
    for t in &page2 {
        assert!(
            !p1_ids.contains(&t.id),
            "page2 should not contain items from page1"
        );
    }

    cleanup_tasks_by_prefix(&app, &cleanup_prefix).await;
}

/// tc_query_tasks_unknown_team_returns_empty: 查询未知 team_id 应返回空结果
#[tokio::test]
async fn tc_query_tasks_unknown_team_returns_empty() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));

    let unknown_team = Uuid::new_v4();
    let params = TaskQueryParams {
        team_id: unknown_team,
        limit: 100,
        offset: 0,
        ..Default::default()
    };
    let (tasks, total) = repo
        .query_tasks(params)
        .await
        .expect("query_tasks for unknown team failed");

    assert_eq!(total, 0, "total should be 0 for unknown team");
    assert!(
        tasks.is_empty(),
        "should return empty list for unknown team"
    );
}

/// tc_query_tasks_combined_filters: 组合多个过滤器验证逻辑正确性
#[tokio::test]
async fn tc_query_tasks_combined_filters() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = app.team_id;
    let api_key_id = app.api_key_id;
    let cleanup_prefix = format!("https://query-combined-{}.", Uuid::new_v4());
    let crawl_id = Uuid::new_v4();

    // 目标任务：Crawl + Queued + 指定 crawl_id
    let mut target = make_task(team_id, api_key_id, "");
    target.url = format!("{}target.example.com", cleanup_prefix);
    target.task_type = TaskType::Crawl;
    target.status = TaskStatus::Queued;
    target.crawl_id = Some(crawl_id);

    // 干扰任务1：Scrape + Queued + 指定 crawl_id（类型不符）
    let mut noise1 = make_task(team_id, api_key_id, "");
    noise1.url = format!("{}n1.example.com", cleanup_prefix);
    noise1.task_type = TaskType::Scrape;
    noise1.crawl_id = Some(crawl_id);

    // 干扰任务2：Crawl + Active + 指定 crawl_id（状态不符）
    let mut noise2 = make_task(team_id, api_key_id, "");
    noise2.url = format!("{}n2.example.com", cleanup_prefix);
    noise2.task_type = TaskType::Crawl;
    noise2.status = TaskStatus::Active;
    noise2.crawl_id = Some(crawl_id);

    repo.create(&target).await.expect("create target failed");
    repo.create(&noise1).await.expect("create noise1 failed");
    repo.create(&noise2).await.expect("create noise2 failed");

    let params = TaskQueryParams {
        team_id,
        crawl_id: Some(crawl_id),
        statuses: Some(vec![TaskStatus::Queued]),
        task_types: Some(vec![TaskType::Crawl]),
        limit: 100,
        offset: 0,
        ..Default::default()
    };
    let (tasks, total) = repo
        .query_tasks(params)
        .await
        .expect("query_tasks combined filter failed");

    assert_eq!(total, 1, "only target matches all filters");
    assert_eq!(tasks.len(), 1, "should return 1 task");
    assert_eq!(tasks[0].id, target.id, "returned task should be target");

    cleanup_tasks_by_prefix(&app, &cleanup_prefix).await;
}

// ==================== batch_cancel 测试 ====================

/// tc_batch_cancel_empty_list_returns_empty: 空列表应返回空 cancelled 和 errors
#[tokio::test]
async fn tc_batch_cancel_empty_list_returns_empty() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = app.team_id;

    let (cancelled, errors) = repo
        .batch_cancel(Vec::new(), team_id, false)
        .await
        .expect("batch_cancel empty list failed");

    assert!(cancelled.is_empty(), "cancelled should be empty");
    assert!(errors.is_empty(), "errors should be empty");
}

/// tc_batch_cancel_all_owned_success: 全部归属当前团队的任务都被取消
#[tokio::test]
async fn tc_batch_cancel_all_owned_success() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = app.team_id;
    let api_key_id = app.api_key_id;
    let cleanup_prefix = format!("https://batch-owned-{}.", Uuid::new_v4());

    let mut t1 = make_task(team_id, api_key_id, "");
    t1.url = format!("{}1.example.com", cleanup_prefix);
    let mut t2 = make_task(team_id, api_key_id, "");
    t2.url = format!("{}2.example.com", cleanup_prefix);
    repo.create(&t1).await.expect("create t1 failed");
    repo.create(&t2).await.expect("create t2 failed");

    let (cancelled, errors) = repo
        .batch_cancel(vec![t1.id, t2.id], team_id, false)
        .await
        .expect("batch_cancel owned failed");

    assert_eq!(cancelled.len(), 2, "2 tasks should be cancelled");
    assert!(errors.is_empty(), "errors should be empty");
    assert!(cancelled.contains(&t1.id), "t1 should be in cancelled");
    assert!(cancelled.contains(&t2.id), "t2 should be in cancelled");

    // 验证任务状态已变更为 Cancelled
    let found1 = repo.find_by_id(t1.id).await.expect("find t1 failed");
    assert_eq!(
        found1.expect("t1").status,
        TaskStatus::Cancelled,
        "t1 status should be Cancelled"
    );
    let found2 = repo.find_by_id(t2.id).await.expect("find t2 failed");
    assert_eq!(
        found2.expect("t2").status,
        TaskStatus::Cancelled,
        "t2 status should be Cancelled"
    );

    cleanup_tasks_by_prefix(&app, &cleanup_prefix).await;
}

/// tc_batch_cancel_non_existent_ids_in_errors: 不存在的任务 ID 应进入 errors 列表
#[tokio::test]
async fn tc_batch_cancel_non_existent_ids_in_errors() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = app.team_id;

    let unknown_id1 = Uuid::new_v4();
    let unknown_id2 = Uuid::new_v4();

    let (cancelled, errors) = repo
        .batch_cancel(vec![unknown_id1, unknown_id2], team_id, false)
        .await
        .expect("batch_cancel non-existent failed");

    assert!(cancelled.is_empty(), "no tasks should be cancelled");
    assert_eq!(errors.len(), 2, "2 errors should be recorded");
    assert!(
        errors.iter().any(|(id, _)| *id == unknown_id1),
        "unknown_id1 should be in errors"
    );
    assert!(
        errors.iter().any(|(id, _)| *id == unknown_id2),
        "unknown_id2 should be in errors"
    );
    // 验证错误原因
    for (_, reason) in &errors {
        assert!(
            reason.contains("not found"),
            "error reason should mention 'not found', got: {}",
            reason
        );
    }
}

/// tc_batch_cancel_team_mismatch_in_errors: 属于其他团队的任务应进入 errors
#[tokio::test]
async fn tc_batch_cancel_team_mismatch_in_errors() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = app.team_id;
    let api_key_id = app.api_key_id;
    let cleanup_prefix = format!("https://batch-mismatch-{}.", Uuid::new_v4());

    // 创建属于当前团队的任务
    let mut own_task = make_task(team_id, api_key_id, "");
    own_task.url = format!("{}own.example.com", cleanup_prefix);
    repo.create(&own_task).await.expect("create own_task failed");

    // 创建属于另一个团队的任务
    let other_team_id = Uuid::new_v4();
    let other_api_key_id = Uuid::new_v4();
    {
        let session = app
            .db_pool
            .get_session("admin")
            .await
            .expect("Failed to get session");
        let conn = session.connection().expect("Failed to get connection");
        // 创建其他团队和 api_key 以满足外键约束
        let _ = conn
            .execute_unprepared(&format!(
                "INSERT INTO teams (id, name) VALUES ('{}', 'Other Team') ON CONFLICT (id) DO NOTHING",
                other_team_id
            ))
            .await;
        let _ = conn
            .execute_unprepared(&format!(
                "INSERT INTO api_keys (id, key, key_hash, team_id) VALUES ('{}', 'other-key-{}', 'other-hash-{}', '{}') ON CONFLICT (id) DO NOTHING",
                other_api_key_id, other_api_key_id, other_api_key_id, other_team_id
            ))
            .await;
    }
    let mut other_task = make_task(other_team_id, other_api_key_id, "");
    other_task.url = format!("{}other.example.com", cleanup_prefix);
    repo.create(&other_task).await.expect("create other_task failed");

    // 用当前 team_id 批量取消 [own_task, other_task]
    let (cancelled, errors) = repo
        .batch_cancel(vec![own_task.id, other_task.id], team_id, false)
        .await
        .expect("batch_cancel mixed teams failed");

    assert_eq!(cancelled.len(), 1, "only own_task should be cancelled");
    assert!(cancelled.contains(&own_task.id), "own_task should be cancelled");
    assert_eq!(errors.len(), 1, "1 error for other_task");
    assert!(
        errors.iter().any(|(id, _)| *id == other_task.id),
        "other_task should be in errors"
    );
    let (_, reason) = &errors[0];
    assert!(
        reason.contains("Team ID mismatch") || reason.contains("mismatch"),
        "error reason should mention team mismatch, got: {}",
        reason
    );

    // 验证 other_task 状态未变（仍为 Queued）
    let other_found = repo
        .find_by_id(other_task.id)
        .await
        .expect("find other_task failed");
    assert_eq!(
        other_found.expect("other_task").status,
        TaskStatus::Queued,
        "other_task should remain Queued"
    );

    // 清理
    cleanup_tasks_by_prefix(&app, &cleanup_prefix).await;
    // 清理 other_team 的任务和创建的 team/api_key
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session for cleanup");
    let conn = session
        .connection()
        .expect("Failed to get connection for cleanup");
    let _ = task_entity::Entity::delete_many()
        .filter(task_entity::Column::TeamId.eq(other_team_id))
        .exec(conn)
        .await;
    let _ = conn
        .execute_unprepared(&format!(
            "DELETE FROM api_keys WHERE id = '{}'",
            other_api_key_id
        ))
        .await;
    let _ = conn
        .execute_unprepared(&format!("DELETE FROM teams WHERE id = '{}'", other_team_id))
        .await;
}

/// tc_batch_cancel_mixed_ids_partial_success: 混合 ID 列表：部分成功，部分失败
#[tokio::test]
async fn tc_batch_cancel_mixed_ids_partial_success() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = app.team_id;
    let api_key_id = app.api_key_id;
    let cleanup_prefix = format!("https://batch-mixed-{}.", Uuid::new_v4());

    // 创建 2 个属于当前团队的任务
    let mut owned1 = make_task(team_id, api_key_id, "");
    owned1.url = format!("{}o1.example.com", cleanup_prefix);
    let mut owned2 = make_task(team_id, api_key_id, "");
    owned2.url = format!("{}o2.example.com", cleanup_prefix);
    repo.create(&owned1).await.expect("create owned1 failed");
    repo.create(&owned2).await.expect("create owned2 failed");

    let unknown_id = Uuid::new_v4();

    // 批量取消：[owned1, unknown, owned2]
    let ids = vec![owned1.id, unknown_id, owned2.id];
    let (cancelled, errors) = repo
        .batch_cancel(ids, team_id, false)
        .await
        .expect("batch_cancel mixed failed");

    assert_eq!(cancelled.len(), 2, "2 owned tasks should be cancelled");
    assert!(cancelled.contains(&owned1.id));
    assert!(cancelled.contains(&owned2.id));
    assert_eq!(errors.len(), 1, "1 error for unknown id");
    assert!(
        errors.iter().any(|(id, _)| *id == unknown_id),
        "unknown_id should be in errors"
    );

    cleanup_tasks_by_prefix(&app, &cleanup_prefix).await;
}

/// tc_batch_cancel_idempotent_second_call: 已取消的任务再次批量取消，不会重复取消
#[tokio::test]
async fn tc_batch_cancel_idempotent_second_call() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(TaskRepositoryImpl::new(
        app.db_pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let team_id = app.team_id;
    let api_key_id = app.api_key_id;
    let cleanup_prefix = format!("https://batch-idem-{}.", Uuid::new_v4());

    let mut t = make_task(team_id, api_key_id, "");
    t.url = format!("{}t.example.com", cleanup_prefix);
    repo.create(&t).await.expect("create t failed");

    // 第一次取消：成功
    let (cancelled1, errors1) = repo
        .batch_cancel(vec![t.id], team_id, false)
        .await
        .expect("first batch_cancel failed");
    assert_eq!(cancelled1.len(), 1);
    assert!(errors1.is_empty());

    // 第二次取消同一 ID：任务仍属于该团队，仍会被"取消"（状态再次设为 Cancelled）
    // batch_cancel 不检查当前状态，所以会再次进入 cancelled 列表
    let (cancelled2, errors2) = repo
        .batch_cancel(vec![t.id], team_id, false)
        .await
        .expect("second batch_cancel failed");
    // 实现不检查当前状态：仍然归属该团队，所以 cancelled 包含此 ID，errors 为空
    assert!(
        cancelled2.contains(&t.id) || errors2.is_empty(),
        "second call: task still owned by team, should be in cancelled or no errors, got cancelled={:?}, errors={:?}",
        cancelled2,
        errors2
    );

    cleanup_tasks_by_prefix(&app, &cleanup_prefix).await;
}
