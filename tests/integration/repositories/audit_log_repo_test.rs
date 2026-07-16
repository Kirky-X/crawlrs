// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Audit log repository integration tests
//!
//! Integration tests for AuditLogRepositoryImpl using a real PostgreSQL database.
//! Covers create, find_by_api_key_id, find_by_team_id, find_denied_for_key, and cleanup_old_logs.

use super::super::helpers::create_test_app_no_worker;
use crawlrs::domain::auth::AuditDecision;
use crawlrs::domain::repositories::audit_log_repository::AuditLogRepository;
use crawlrs::domain::services::audit_service::AuditLogBuilder;
use crawlrs::infrastructure::database::entities::auth::audit_log::{
    Column as AuditColumn, Entity as AuditEntity,
};
use crawlrs::infrastructure::repositories::audit_log_repo_impl::AuditLogRepositoryImpl;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

/// 测试创建审计日志并通过 API Key ID 查询
#[tokio::test]
async fn test_create_and_find_by_api_key_id() {
    let app = create_test_app_no_worker().await;
    let repo = AuditLogRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;
    let team_id = app.team_id;

    // 创建一条 Allow 审计日志
    let entry = AuditLogBuilder::new("scrape:start", AuditDecision::Allow)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .with_request_path("/api/scrape")
        .with_request_method("POST")
        .build();

    let created = repo
        .create(&entry)
        .await
        .expect("Failed to create audit log");
    assert_eq!(created.id, entry.id);
    assert_eq!(created.api_key_id, Some(api_key_id));
    assert_eq!(created.decision, AuditDecision::Allow);

    // 通过 api_key_id 查询
    let logs = repo
        .find_by_api_key_id(api_key_id, 10, 0)
        .await
        .expect("Failed to find audit logs by api key id");

    assert!(
        logs.iter().any(|l| l.id == entry.id),
        "Created log should be found by api_key_id"
    );

    // 清理
    cleanup_audit_logs(&app, team_id).await;
}

/// 测试通过 Team ID 查询审计日志
#[tokio::test]
async fn test_find_by_team_id() {
    let app = create_test_app_no_worker().await;
    let repo = AuditLogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;
    let api_key_id = app.api_key_id;

    // 创建多条日志
    let entry1 = AuditLogBuilder::new("search:query", AuditDecision::Allow)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .build();
    let entry2 = AuditLogBuilder::new("search:query", AuditDecision::Deny)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .with_denial_reason("rate limit exceeded")
        .build();

    repo.create(&entry1)
        .await
        .expect("Failed to create audit log 1");
    repo.create(&entry2)
        .await
        .expect("Failed to create audit log 2");

    // 通过 team_id 查询
    let logs = repo
        .find_by_team_id(team_id, 10, 0)
        .await
        .expect("Failed to find audit logs by team id");

    assert!(
        logs.iter().any(|l| l.id == entry1.id),
        "Entry 1 should be found by team_id"
    );
    assert!(
        logs.iter().any(|l| l.id == entry2.id),
        "Entry 2 should be found by team_id"
    );

    // 清理
    cleanup_audit_logs(&app, team_id).await;
}

/// 测试查询被拒绝的请求
#[tokio::test]
async fn test_find_denied_for_key() {
    let app = create_test_app_no_worker().await;
    let repo = AuditLogRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;
    let team_id = app.team_id;

    // 创建一条 Allow 和一条 Deny 日志
    let allow_entry = AuditLogBuilder::new("scrape:allowed", AuditDecision::Allow)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .build();
    let deny_entry = AuditLogBuilder::new("scrape:denied", AuditDecision::Deny)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .with_denial_reason("insufficient credits")
        .build();

    repo.create(&allow_entry)
        .await
        .expect("Failed to create allow entry");
    repo.create(&deny_entry)
        .await
        .expect("Failed to create deny entry");

    // 查询被拒绝的请求
    let denied = repo
        .find_denied_for_key(api_key_id, 10)
        .await
        .expect("Failed to find denied logs");

    // 验证只返回 Deny 决策的日志
    assert!(
        denied.iter().all(|l| l.decision == AuditDecision::Deny),
        "All returned logs should be Deny decisions"
    );
    assert!(
        denied.iter().any(|l| l.id == deny_entry.id),
        "Deny entry should be in results"
    );
    assert!(
        !denied.iter().any(|l| l.id == allow_entry.id),
        "Allow entry should NOT be in denied results"
    );

    // 清理
    cleanup_audit_logs(&app, team_id).await;
}

/// 测试清理旧审计日志
#[tokio::test]
async fn test_cleanup_old_logs() {
    let app = create_test_app_no_worker().await;
    let repo = AuditLogRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;
    let api_key_id = app.api_key_id;

    // 创建一条审计日志
    let entry = AuditLogBuilder::new("cleanup:test", AuditDecision::Allow)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .build();

    repo.create(&entry)
        .await
        .expect("Failed to create audit log");

    // 验证日志存在
    let logs_before = repo
        .find_by_api_key_id(api_key_id, 10, 0)
        .await
        .expect("Failed to find logs");
    assert!(
        logs_before.iter().any(|l| l.id == entry.id),
        "Log should exist before cleanup"
    );

    // 手动将日志的 created_at 更新为 10 天前，使其被清理
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session");
    let conn = session.connection().expect("Failed to get connection");
    sea_orm::ConnectionTrait::execute_unprepared(
        conn,
        &format!(
            "UPDATE audit_logs SET created_at = NOW() - INTERVAL '10 days' WHERE id = '{}'",
            entry.id
        ),
    )
    .await
    .expect("Failed to update log timestamp");

    // 清理 7 天前的日志
    let deleted = repo
        .cleanup_old_logs(7)
        .await
        .expect("Failed to cleanup old logs");

    assert!(
        deleted >= 1,
        "At least 1 log should be deleted, got {}",
        deleted
    );

    // 验证日志已被删除
    let logs_after = repo
        .find_by_api_key_id(api_key_id, 10, 0)
        .await
        .expect("Failed to find logs after cleanup");
    assert!(
        !logs_after.iter().any(|l| l.id == entry.id),
        "Log should be deleted after cleanup"
    );

    // 清理剩余数据
    cleanup_audit_logs(&app, team_id).await;
}

/// tc_find_by_team_id_returns_empty_for_unknown_team: 未知团队返回空列表
#[tokio::test]
async fn tc_find_by_team_id_returns_empty_for_unknown_team() {
    let app = create_test_app_no_worker().await;
    let repo = AuditLogRepositoryImpl::new(app.db_pool.clone());

    let unknown_team_id = Uuid::new_v4();
    let logs = repo
        .find_by_team_id(unknown_team_id, 10, 0)
        .await
        .expect("Failed to find logs for unknown team");

    assert!(logs.is_empty(), "Should return empty list for unknown team");
}

/// tc_find_denied_for_key_returns_empty_when_no_denials: 无拒绝记录时返回空列表
#[tokio::test]
async fn tc_find_denied_for_key_returns_empty_when_no_denials() {
    let app = create_test_app_no_worker().await;
    let repo = AuditLogRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;
    let team_id = app.team_id;

    // 只创建 Allow 日志
    let allow_entry = AuditLogBuilder::new("scrape:allow_only", AuditDecision::Allow)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .build();
    repo.create(&allow_entry)
        .await
        .expect("Failed to create allow entry");

    // 查询被拒绝的请求——应返回空
    let denied = repo
        .find_denied_for_key(api_key_id, 10)
        .await
        .expect("Failed to find denied logs");

    assert!(
        denied.iter().all(|l| l.decision == AuditDecision::Deny),
        "All returned logs should be Deny decisions"
    );
    assert!(
        !denied.iter().any(|l| l.id == allow_entry.id),
        "Allow entry should not be in denied results"
    );

    cleanup_audit_logs(&app, team_id).await;
}

/// tc_find_by_api_key_id_with_pagination: 分页查询验证 offset 跳过记录
#[tokio::test]
async fn tc_find_by_api_key_id_with_pagination() {
    let app = create_test_app_no_worker().await;
    let repo = AuditLogRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;
    let team_id = app.team_id;

    // 创建 3 条日志
    let entry1 = AuditLogBuilder::new("page:test1", AuditDecision::Allow)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .build();
    let entry2 = AuditLogBuilder::new("page:test2", AuditDecision::Allow)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .build();
    let entry3 = AuditLogBuilder::new("page:test3", AuditDecision::Allow)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .build();

    repo.create(&entry1).await.expect("Failed to create entry1");
    repo.create(&entry2).await.expect("Failed to create entry2");
    repo.create(&entry3).await.expect("Failed to create entry3");

    // 第一页（limit=2, offset=0）
    let page1 = repo
        .find_by_api_key_id(api_key_id, 2, 0)
        .await
        .expect("Failed to find page 1");
    assert!(page1.len() <= 2, "Page 1 should have at most 2 entries");

    // 第二页（limit=2, offset=2）
    let page2 = repo
        .find_by_api_key_id(api_key_id, 2, 2)
        .await
        .expect("Failed to find page 2");

    // 验证分页不重复
    let page1_ids: Vec<_> = page1.iter().map(|l| l.id).collect();
    let page2_ids: Vec<_> = page2.iter().map(|l| l.id).collect();
    for id in &page2_ids {
        assert!(
            !page1_ids.contains(id),
            "Page 2 should not contain items from Page 1"
        );
    }

    // 验证三条日志都在两页中
    let all_ids: Vec<_> = page1_ids.into_iter().chain(page2_ids.into_iter()).collect();
    assert!(all_ids.contains(&entry1.id), "Entry1 should be in pages");
    assert!(all_ids.contains(&entry2.id), "Entry2 should be in pages");
    assert!(all_ids.contains(&entry3.id), "Entry3 should be in pages");

    cleanup_audit_logs(&app, team_id).await;
}

/// tc_cleanup_old_logs_returns_zero_when_nothing_to_delete: 无旧日志时返回 0
#[tokio::test]
async fn tc_cleanup_old_logs_returns_zero_when_nothing_to_delete() {
    let app = create_test_app_no_worker().await;
    let repo = AuditLogRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;
    let team_id = app.team_id;

    // 创建一条新日志（created_at 为当前时间）
    let entry = AuditLogBuilder::new("cleanup:zero", AuditDecision::Allow)
        .with_api_key_id(api_key_id)
        .with_team_id(team_id)
        .build();
    repo.create(&entry)
        .await
        .expect("Failed to create entry");

    // 清理 30 天前的日志——新日志不应被删除
    let _deleted = repo
        .cleanup_old_logs(30)
        .await
        .expect("Failed to cleanup old logs");

    // 可能删除了其他测试的旧日志，但当前日志不应被删除
    let logs = repo
        .find_by_api_key_id(api_key_id, 10, 0)
        .await
        .expect("Failed to find logs");
    assert!(
        logs.iter().any(|l| l.id == entry.id),
        "Recent log should not be deleted by cleanup_old_logs(30)"
    );

    cleanup_audit_logs(&app, team_id).await;
}

/// 辅助函数：清理指定 team_id 的审计日志
async fn cleanup_audit_logs(app: &super::super::helpers::test_app::TestApp, team_id: Uuid) {
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session for cleanup");
    let conn = session
        .connection()
        .expect("Failed to get connection for cleanup");
    let _ = AuditEntity::delete_many()
        .filter(AuditColumn::TeamId.eq(team_id))
        .exec(conn)
        .await;
}
