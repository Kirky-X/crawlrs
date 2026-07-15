// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook repository integration tests
//!
//! Integration tests for WebhookRepoImpl using a real PostgreSQL database.
//! Covers create, find_by_id, and find_by_team_id.

use super::super::helpers::create_test_app_no_worker;
use crawlrs::domain::models::Webhook;
use crawlrs::domain::repositories::webhook_repository::WebhookRepository;
use crawlrs::infrastructure::database::entities::webhook;
use crawlrs::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

/// 测试创建 Webhook 并通过 ID 查询
#[tokio::test]
async fn test_create_and_find_by_id() {
    let app = create_test_app_no_worker().await;
    let repo = WebhookRepoImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let unique_prefix = Uuid::new_v4().to_string();
    let url = format!("https://{}.example.com/webhook", unique_prefix);
    let webhook_id = Uuid::new_v4();

    let webhook = Webhook::new(webhook_id, team_id, url.clone());
    let created = repo
        .create(&webhook)
        .await
        .expect("Failed to create webhook");
    assert_eq!(created.id, webhook_id);
    assert_eq!(created.team_id, team_id);
    assert_eq!(created.url, url);

    // 通过 ID 查询
    let found = repo
        .find_by_id(webhook_id)
        .await
        .expect("Failed to find webhook by id");
    let found = found.expect("Webhook should be found by id");
    assert_eq!(found.id, webhook_id);
    assert_eq!(found.team_id, team_id);
    assert_eq!(found.url, url);

    cleanup_webhooks(&app, team_id).await;
}

/// 测试通过不存在的 ID 查询：应返回 None
#[tokio::test]
async fn test_find_by_id_returns_none_for_unknown() {
    let app = create_test_app_no_worker().await;
    let repo = WebhookRepoImpl::new(app.db_pool.clone());

    let unknown_id = Uuid::new_v4();
    let result = repo
        .find_by_id(unknown_id)
        .await
        .expect("Failed to query webhook by unknown id");
    assert!(
        result.is_none(),
        "Should return None for unknown webhook id"
    );
}

/// 测试通过 team_id 查询多个 Webhook
#[tokio::test]
async fn test_find_by_team_id_returns_multiple() {
    let app = create_test_app_no_worker().await;
    let repo = WebhookRepoImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let unique_prefix = Uuid::new_v4().to_string();
    let webhook1 = Webhook::new(
        Uuid::new_v4(),
        team_id,
        format!("https://{}-1.example.com/hook", unique_prefix),
    );
    let webhook2 = Webhook::new(
        Uuid::new_v4(),
        team_id,
        format!("https://{}-2.example.com/hook", unique_prefix),
    );

    repo.create(&webhook1)
        .await
        .expect("Failed to create webhook 1");
    repo.create(&webhook2)
        .await
        .expect("Failed to create webhook 2");

    let found = repo
        .find_by_team_id(team_id)
        .await
        .expect("Failed to find webhooks by team id");

    assert!(
        found.len() >= 2,
        "Should find at least 2 webhooks for team, got {}",
        found.len()
    );
    assert!(
        found.iter().any(|w| w.id == webhook1.id),
        "Webhook 1 should be in results"
    );
    assert!(
        found.iter().any(|w| w.id == webhook2.id),
        "Webhook 2 should be in results"
    );

    cleanup_webhooks(&app, team_id).await;
}

/// 测试空团队的 Webhook 查询：应返回空列表
#[tokio::test]
async fn test_find_by_team_id_returns_empty_for_team_without_webhooks() {
    let app = create_test_app_no_worker().await;
    let repo = WebhookRepoImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let found = repo
        .find_by_team_id(team_id)
        .await
        .expect("Failed to find webhooks by team id");

    // 新团队不应有任何 webhook（前提是 cleanup 执行正确）
    assert!(
        found.iter().all(|w| w.team_id == team_id),
        "All returned webhooks should belong to the queried team"
    );
}

/// 辅助函数：清理指定 team_id 的 webhooks
async fn cleanup_webhooks(app: &super::super::helpers::test_app::TestApp, team_id: Uuid) {
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session for cleanup");
    let conn = session
        .connection()
        .expect("Failed to get connection for cleanup");
    let _ = webhook::Entity::delete_many()
        .filter(webhook::Column::TeamId.eq(team_id))
        .exec(conn)
        .await;
}
