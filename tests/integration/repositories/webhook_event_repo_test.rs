// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook event repository integration tests
//!
//! Integration tests for WebhookEventRepoImpl using a real PostgreSQL database.
//! Covers create, find_by_id, find_pending, update, find_by_team_id_paginated,
//! and count_by_team_id.

use super::super::helpers::create_test_app_no_worker;
use crawlrs::domain::models::{WebhookEvent, WebhookEventType, WebhookStatus};
use crawlrs::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crawlrs::infrastructure::database::entities::webhook_event;
use crawlrs::infrastructure::repositories::webhook_event_repo_impl::WebhookEventRepoImpl;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

/// 创建测试用的 WebhookEvent（辅助函数）
fn make_event(team_id: Uuid, webhook_url: &str) -> WebhookEvent {
    WebhookEvent::new(
        Uuid::new_v4(),
        team_id,
        Uuid::new_v4(),
        WebhookEventType::CrawlCompleted,
        serde_json::json!({"event": "test", "url": webhook_url}),
        webhook_url.to_string(),
    )
}

/// 测试创建 WebhookEvent 并通过 ID 查询
#[tokio::test]
async fn test_create_and_find_by_id() {
    let app = create_test_app_no_worker().await;
    let repo = WebhookEventRepoImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let unique_url = format!("https://{}.example.com/hook", Uuid::new_v4());
    let event = make_event(team_id, &unique_url);

    let created = repo
        .create(&event)
        .await
        .expect("Failed to create webhook event");
    assert_eq!(created.id, event.id);
    assert_eq!(created.team_id, team_id);
    assert_eq!(created.webhook_url, unique_url);
    assert_eq!(created.status, WebhookStatus::Pending);

    let found = repo
        .find_by_id(event.id)
        .await
        .expect("Failed to find event by id");
    let found = found.expect("Event should be found by id");
    assert_eq!(found.id, event.id);
    assert_eq!(found.team_id, team_id);
    assert_eq!(found.webhook_url, unique_url);
    assert_eq!(found.event_type, WebhookEventType::CrawlCompleted);

    cleanup_events(&app, team_id).await;
}

/// 测试通过不存在的 ID 查询：应返回 None
#[tokio::test]
async fn test_find_by_id_returns_none_for_unknown() {
    let app = create_test_app_no_worker().await;
    let repo = WebhookEventRepoImpl::new(app.db_pool.clone());

    let unknown_id = Uuid::new_v4();
    let result = repo
        .find_by_id(unknown_id)
        .await
        .expect("Failed to query unknown event id");
    assert!(result.is_none(), "Should return None for unknown event id");
}

/// 测试查询待处理的 Webhook 事件
#[tokio::test]
async fn test_find_pending_returns_pending_events() {
    let app = create_test_app_no_worker().await;
    let repo = WebhookEventRepoImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let unique_url = format!("https://{}.example.com/hook", Uuid::new_v4());
    let event = make_event(team_id, &unique_url);

    repo.create(&event).await.expect("Failed to create event");

    let pending = repo
        .find_pending(100)
        .await
        .expect("Failed to find pending events");

    assert!(
        pending.iter().any(|e| e.id == event.id),
        "Created pending event should be in find_pending results"
    );

    cleanup_events(&app, team_id).await;
}

/// 测试更新 Webhook 事件
#[tokio::test]
async fn test_update_event() {
    let app = create_test_app_no_worker().await;
    let repo = WebhookEventRepoImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let unique_url = format!("https://{}.example.com/hook", Uuid::new_v4());
    let mut event = make_event(team_id, &unique_url);

    repo.create(&event).await.expect("Failed to create event");

    // 修改事件状态为 Delivered
    event.mark_delivered(200, Some("ok".to_string()));
    repo.update(&event).await.expect("Failed to update event");

    let found = repo
        .find_by_id(event.id)
        .await
        .expect("Failed to find updated event");
    let found = found.expect("Updated event should be found");
    assert_eq!(
        found.status,
        WebhookStatus::Delivered,
        "Status should be Delivered"
    );
    assert_eq!(
        found.response_status,
        Some(200),
        "Response status should be 200"
    );
    assert!(found.delivered_at.is_some(), "delivered_at should be set");

    cleanup_events(&app, team_id).await;
}

/// 测试通过 team_id 分页查询事件
#[tokio::test]
async fn test_find_by_team_id_paginated() {
    let app = create_test_app_no_worker().await;
    let repo = WebhookEventRepoImpl::new(app.db_pool.clone());
    let team_id = app.team_id;
    let unique_prefix = Uuid::new_v4().to_string();

    // 创建 3 个事件
    for i in 0..3 {
        let url = format!("https://{}-{}.example.com/hook", unique_prefix, i);
        let event = make_event(team_id, &url);
        repo.create(&event).await.expect("Failed to create event");
    }

    let page1 = repo
        .find_by_team_id_paginated(team_id, 2, 0)
        .await
        .expect("Failed to find paginated page 1");
    assert!(page1.len() <= 2, "Page 1 should have at most 2 items");

    let page2 = repo
        .find_by_team_id_paginated(team_id, 2, 2)
        .await
        .expect("Failed to find paginated page 2");

    // 验证分页不重复
    let page1_ids: Vec<_> = page1.iter().map(|e| e.id).collect();
    for e in &page2 {
        assert!(
            !page1_ids.contains(&e.id),
            "Page 2 should not contain items from Page 1"
        );
    }

    cleanup_events(&app, team_id).await;
}

/// 测试统计团队事件数量
#[tokio::test]
async fn test_count_by_team_id() {
    let app = create_test_app_no_worker().await;
    let repo = WebhookEventRepoImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let count_before = repo
        .count_by_team_id(team_id)
        .await
        .expect("Failed to count before insert");

    let unique_url = format!("https://{}.example.com/hook", Uuid::new_v4());
    let event = make_event(team_id, &unique_url);
    repo.create(&event).await.expect("Failed to create event");

    let count_after = repo
        .count_by_team_id(team_id)
        .await
        .expect("Failed to count after insert");

    assert_eq!(
        count_after,
        count_before + 1,
        "Count should increase by 1 after insert"
    );

    cleanup_events(&app, team_id).await;
}

/// 测试不同事件类型的创建和查询
#[tokio::test]
async fn test_create_with_different_event_types() {
    let app = create_test_app_no_worker().await;
    let repo = WebhookEventRepoImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let event_types = vec![
        WebhookEventType::CrawlCompleted,
        WebhookEventType::CrawlFailed,
        WebhookEventType::ScrapeCompleted,
        WebhookEventType::ScrapeFailed,
        WebhookEventType::Custom("custom.event".to_string()),
    ];

    let mut event_ids = Vec::new();
    for (i, et) in event_types.iter().enumerate() {
        let url = format!("https://{}-{}.example.com/hook", Uuid::new_v4(), i);
        let event = WebhookEvent::new(
            Uuid::new_v4(),
            team_id,
            Uuid::new_v4(),
            et.clone(),
            serde_json::json!({"i": i}),
            url,
        );
        repo.create(&event).await.expect("Failed to create event");
        event_ids.push((event.id, et.clone()));
    }

    // 验证每个事件都能找到
    for (id, expected_type) in &event_ids {
        let found = repo.find_by_id(*id).await.expect("Failed to find event");
        let found = found.expect("Event should be found");
        assert_eq!(&found.event_type, expected_type, "Event type should match");
    }

    cleanup_events(&app, team_id).await;
}

/// 测试 find_pending 带 limit 限制
#[tokio::test]
async fn test_find_pending_respects_limit() {
    let app = create_test_app_no_worker().await;
    let repo = WebhookEventRepoImpl::new(app.db_pool.clone());
    let team_id = app.team_id;
    let unique_prefix = Uuid::new_v4().to_string();

    // 创建 5 个 pending 事件
    for i in 0..5 {
        let url = format!("https://{}-{}.example.com/hook", unique_prefix, i);
        let event = make_event(team_id, &url);
        repo.create(&event).await.expect("Failed to create event");
    }

    let pending = repo
        .find_pending(2)
        .await
        .expect("Failed to find pending with limit");

    // find_pending 内部对 pending 和 failed_retry 各自 limit，所以总数最多为 2*2=4
    // 但我们只创建 pending 事件，所以 pending 部分最多 2
    assert!(
        pending.len() <= 4,
        "find_pending(2) should return limited results, got {}",
        pending.len()
    );

    cleanup_events(&app, team_id).await;
}

/// 辅助函数：清理指定 team_id 的 webhook_events
async fn cleanup_events(app: &super::super::helpers::test_app::TestApp, team_id: Uuid) {
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session for cleanup");
    let conn = session
        .connection()
        .expect("Failed to get connection for cleanup");
    let _ = webhook_event::Entity::delete_many()
        .filter(webhook_event::Column::TeamId.eq(team_id))
        .exec(conn)
        .await;
}
