// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Tasks backlog repository integration tests
//!
//! Integration tests for TasksBacklogRepositoryImpl using a real PostgreSQL database.
//! Covers create, find_by_id, find_by_task_id, update, delete, get_pending_tasks,
//! get_expired_tasks, count_by_status, and update_status_batch.

use super::super::helpers::create_test_app_no_worker;
use crawlrs::domain::repositories::tasks_backlog_repository::{
    TasksBacklog, TasksBacklogRepository, TasksBacklogStatus,
};
use crawlrs::infrastructure::database::entities::tasks_backlog;
use crawlrs::infrastructure::repositories::tasks_backlog_repo_impl::TasksBacklogRepositoryImpl;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

/// 创建测试用的 TasksBacklog（辅助函数）
fn make_backlog(team_id: Uuid, task_id: Uuid, priority: i32) -> TasksBacklog {
    TasksBacklog::new(
        task_id,
        team_id,
        "scrape".to_string(),
        priority,
        serde_json::json!({"url": "https://example.com"}),
        None,
    )
}

/// 测试创建 backlog 并通过 ID 查询
#[tokio::test]
async fn test_create_and_find_by_id() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;
    let task_id = Uuid::new_v4();

    let backlog = make_backlog(team_id, task_id, 1);
    let created = repo
        .create(&backlog)
        .await
        .expect("Failed to create backlog");
    assert_eq!(created.task_id, task_id);
    assert_eq!(created.team_id, team_id);
    assert_eq!(created.status, TasksBacklogStatus::Pending);

    let found = repo
        .find_by_id(backlog.id)
        .await
        .expect("Failed to find backlog by id");
    let found = found.expect("Backlog should be found by id");
    assert_eq!(found.id, backlog.id);
    assert_eq!(found.task_id, task_id);
    assert_eq!(found.priority, 1);

    cleanup_backlogs(&app, team_id).await;
}

/// 测试通过 task_id 查询
#[tokio::test]
async fn test_find_by_task_id() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;
    let task_id = Uuid::new_v4();

    let backlog = make_backlog(team_id, task_id, 2);
    repo.create(&backlog)
        .await
        .expect("Failed to create backlog");

    let found = repo
        .find_by_task_id(task_id)
        .await
        .expect("Failed to find backlog by task_id");
    let found = found.expect("Backlog should be found by task_id");
    assert_eq!(found.task_id, task_id);
    assert_eq!(found.id, backlog.id);

    cleanup_backlogs(&app, team_id).await;
}

/// 测试通过不存在的 task_id 查询：应返回 None
#[tokio::test]
async fn test_find_by_task_id_returns_none_for_unknown() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());

    let unknown_task_id = Uuid::new_v4();
    let result = repo
        .find_by_task_id(unknown_task_id)
        .await
        .expect("Failed to query unknown task_id");
    assert!(result.is_none(), "Should return None for unknown task_id");
}

/// 测试更新 backlog
#[tokio::test]
async fn test_update_backlog() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;
    let task_id = Uuid::new_v4();

    let mut backlog = make_backlog(team_id, task_id, 1);
    repo.create(&backlog)
        .await
        .expect("Failed to create backlog");

    // 修改状态并更新
    backlog
        .mark_processing()
        .expect("mark_processing should succeed");
    repo.update(&backlog)
        .await
        .expect("Failed to update backlog");

    let found = repo
        .find_by_id(backlog.id)
        .await
        .expect("Failed to find updated backlog");
    let found = found.expect("Updated backlog should be found");
    assert_eq!(found.status, TasksBacklogStatus::Processing);

    cleanup_backlogs(&app, team_id).await;
}

/// 测试删除 backlog
#[tokio::test]
async fn test_delete_backlog() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;
    let task_id = Uuid::new_v4();

    let backlog = make_backlog(team_id, task_id, 1);
    repo.create(&backlog)
        .await
        .expect("Failed to create backlog");

    repo.delete(backlog.id)
        .await
        .expect("Failed to delete backlog");

    let found = repo
        .find_by_id(backlog.id)
        .await
        .expect("Failed to query after delete");
    assert!(found.is_none(), "Backlog should be None after delete");
}

/// 测试获取待处理的任务（按优先级排序）
#[tokio::test]
async fn test_get_pending_tasks_filtered_by_team() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    // 创建两个 pending 任务，优先级不同
    let b1 = make_backlog(team_id, Uuid::new_v4(), 5);
    let b2 = make_backlog(team_id, Uuid::new_v4(), 1);
    repo.create(&b1).await.expect("Failed to create b1");
    repo.create(&b2).await.expect("Failed to create b2");

    let pending = repo
        .get_pending_tasks(Some(team_id), None)
        .await
        .expect("Failed to get pending tasks");

    assert!(
        pending.iter().any(|b| b.id == b1.id),
        "b1 should be in pending"
    );
    assert!(
        pending.iter().any(|b| b.id == b2.id),
        "b2 should be in pending"
    );

    // 验证按优先级升序排序：b2 (priority=1) 应在 b1 (priority=5) 之前
    let pos_b1 = pending.iter().position(|b| b.id == b1.id);
    let pos_b2 = pending.iter().position(|b| b.id == b2.id);
    if let (Some(p1), Some(p2)) = (pos_b1, pos_b2) {
        assert!(
            p2 < p1,
            "b2 (priority=1) should come before b1 (priority=5)"
        );
    }

    cleanup_backlogs(&app, team_id).await;
}

/// 测试获取待处理任务带 limit
#[tokio::test]
async fn test_get_pending_tasks_with_limit() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    for _ in 0..5 {
        let b = make_backlog(team_id, Uuid::new_v4(), 1);
        repo.create(&b).await.expect("Failed to create backlog");
    }

    let pending = repo
        .get_pending_tasks(Some(team_id), Some(2))
        .await
        .expect("Failed to get pending tasks with limit");

    // 注意：可能有其他测试的 pending 任务，但 limit 限制总返回数量
    assert!(
        pending.len() <= 2,
        "Should return at most 2 pending tasks, got {}",
        pending.len()
    );

    cleanup_backlogs(&app, team_id).await;
}

/// 测试获取过期任务
#[tokio::test]
async fn test_get_expired_tasks() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;
    let task_id = Uuid::new_v4();

    // 创建一个已过期的 backlog（expires_at 在过去）
    let mut backlog = make_backlog(team_id, task_id, 1);
    backlog.expires_at = Some(chrono::Utc::now() - chrono::Duration::hours(1));
    repo.create(&backlog)
        .await
        .expect("Failed to create expired backlog");

    let expired = repo
        .get_expired_tasks(None)
        .await
        .expect("Failed to get expired tasks");

    assert!(
        expired.iter().any(|b| b.id == backlog.id),
        "Expired backlog should be in results"
    );

    cleanup_backlogs(&app, team_id).await;
}

/// 测试按状态统计任务数量
#[tokio::test]
async fn test_count_by_status() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let count_before = repo
        .count_by_status(Some(team_id), TasksBacklogStatus::Pending)
        .await
        .expect("Failed to count before insert");

    let b1 = make_backlog(team_id, Uuid::new_v4(), 1);
    let b2 = make_backlog(team_id, Uuid::new_v4(), 2);
    repo.create(&b1).await.expect("Failed to create b1");
    repo.create(&b2).await.expect("Failed to create b2");

    let count_after = repo
        .count_by_status(Some(team_id), TasksBacklogStatus::Pending)
        .await
        .expect("Failed to count after insert");

    assert_eq!(
        count_after,
        count_before + 2,
        "Pending count should increase by 2"
    );

    cleanup_backlogs(&app, team_id).await;
}

/// 测试批量更新状态
#[tokio::test]
async fn test_update_status_batch() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let b1 = make_backlog(team_id, Uuid::new_v4(), 1);
    let b2 = make_backlog(team_id, Uuid::new_v4(), 2);
    repo.create(&b1).await.expect("Failed to create b1");
    repo.create(&b2).await.expect("Failed to create b2");

    // 批量更新为 Processing
    let affected = repo
        .update_status_batch(&[b1.id, b2.id], TasksBacklogStatus::Processing)
        .await
        .expect("Failed to batch update status");

    assert_eq!(affected, 2, "Should affect 2 rows");

    // 验证状态已更新
    let found1 = repo.find_by_id(b1.id).await.expect("Failed to find b1");
    assert_eq!(
        found1.expect("b1").status,
        TasksBacklogStatus::Processing,
        "b1 status should be Processing"
    );

    let found2 = repo.find_by_id(b2.id).await.expect("Failed to find b2");
    assert_eq!(
        found2.expect("b2").status,
        TasksBacklogStatus::Processing,
        "b2 status should be Processing"
    );

    cleanup_backlogs(&app, team_id).await;
}

/// 测试批量更新空列表：应返回 0
#[tokio::test]
async fn test_update_status_batch_empty_list() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());

    let affected = repo
        .update_status_batch(&[], TasksBacklogStatus::Processing)
        .await
        .expect("Failed to batch update empty list");

    assert_eq!(affected, 0, "Should affect 0 rows for empty list");
}

/// tc_find_by_id_returns_none_for_unknown: 通过不存在的 ID 查询应返回 None
#[tokio::test]
async fn tc_find_by_id_returns_none_for_unknown() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());

    let unknown_id = Uuid::new_v4();
    let result = repo
        .find_by_id(unknown_id)
        .await
        .expect("Failed to query unknown backlog id");
    assert!(
        result.is_none(),
        "Should return None for unknown backlog id"
    );
}

/// tc_get_pending_tasks_no_team_no_limit: 不带 team_id 和 limit 查询所有 pending
#[tokio::test]
async fn tc_get_pending_tasks_no_team_no_limit() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let b = make_backlog(team_id, Uuid::new_v4(), 1);
    repo.create(&b).await.expect("Failed to create backlog");

    // 不带 team_id 和 limit 查询：应返回所有 pending（包含我们的）
    let pending = repo
        .get_pending_tasks(None, None)
        .await
        .expect("Failed to get pending tasks without filters");

    assert!(
        pending.iter().any(|t| t.id == b.id),
        "Created backlog should be in unfiltered pending results"
    );

    cleanup_backlogs(&app, team_id).await;
}

/// tc_count_by_status_no_team: 不带 team_id 统计指定状态数量
#[tokio::test]
async fn tc_count_by_status_no_team() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let b = make_backlog(team_id, Uuid::new_v4(), 1);
    repo.create(&b).await.expect("Failed to create backlog");

    // 不带 team_id 统计 pending 数量
    let count = repo
        .count_by_status(None, TasksBacklogStatus::Pending)
        .await
        .expect("Failed to count by status without team filter");

    assert!(
        count >= 1,
        "Global pending count should be at least 1, got {}",
        count
    );

    cleanup_backlogs(&app, team_id).await;
}

/// tc_get_expired_tasks_with_limit: 带 limit 查询过期任务
#[tokio::test]
async fn tc_get_expired_tasks_with_limit() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    // 创建 3 个过期任务
    for _ in 0..3 {
        let mut backlog = make_backlog(team_id, Uuid::new_v4(), 1);
        backlog.expires_at = Some(chrono::Utc::now() - chrono::Duration::hours(1));
        repo.create(&backlog)
            .await
            .expect("Failed to create expired backlog");
    }

    // 带 limit=2 查询
    let expired = repo
        .get_expired_tasks(Some(2))
        .await
        .expect("Failed to get expired tasks with limit");

    assert!(
        expired.len() <= 2,
        "Should return at most 2 expired tasks with limit, got {}",
        expired.len()
    );

    cleanup_backlogs(&app, team_id).await;
}

/// tc_delete_nonexistent_is_noop: 删除不存在的 ID 不报错
#[tokio::test]
async fn tc_delete_nonexistent_is_noop() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());

    let unknown_id = Uuid::new_v4();
    repo.delete(unknown_id)
        .await
        .expect("Deleting non-existent backlog should not error");
}

/// tc_update_status_batch_with_nonexistent: 批量更新包含不存在的 ID 只影响已存在的
#[tokio::test]
async fn tc_update_status_batch_with_nonexistent() {
    let app = create_test_app_no_worker().await;
    let repo = TasksBacklogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let b = make_backlog(team_id, Uuid::new_v4(), 1);
    repo.create(&b).await.expect("Failed to create backlog");

    // 包含一个存在和一个不存在的 ID
    let unknown_id = Uuid::new_v4();
    let affected = repo
        .update_status_batch(&[b.id, unknown_id], TasksBacklogStatus::Processing)
        .await
        .expect("Failed to batch update with mixed ids");

    assert_eq!(
        affected, 1,
        "Should only affect 1 row (the existing one), got {}",
        affected
    );

    cleanup_backlogs(&app, team_id).await;
}

/// 辅助函数：清理指定 team_id 的 tasks_backlog
async fn cleanup_backlogs(app: &super::super::helpers::test_app::TestApp, team_id: Uuid) {
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session for cleanup");
    let conn = session
        .connection()
        .expect("Failed to get connection for cleanup");
    let _ = tasks_backlog::Entity::delete_many()
        .filter(tasks_backlog::Column::TeamId.eq(team_id))
        .exec(conn)
        .await;
}
