// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl repository integration tests
//!
//! Integration tests for CrawlRepositoryImpl using a real PostgreSQL database.
//! Covers create, find_by_id, update, increment_completed_tasks,
//! increment_failed_tasks, update_status, increment_total_tasks,
//! find_by_team_id_paginated, and count_by_team_id.

use super::super::helpers::create_test_app_no_worker;
use crawlrs::domain::models::{Crawl, CrawlStatus};
use crawlrs::domain::repositories::crawl_repository::CrawlRepository;
use crawlrs::infrastructure::database::entities::crawl;
use crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

/// 测试创建 Crawl 并通过 ID 查询
#[tokio::test]
async fn test_create_and_find_by_id() {
    let app = create_test_app_no_worker().await;
    let repo = CrawlRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let unique_prefix = Uuid::new_v4().to_string();
    let crawl_id = Uuid::new_v4();
    let crawl = Crawl::new(
        crawl_id,
        team_id,
        format!("Test Crawl {}", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        format!("https://{}.example.com/page", unique_prefix),
        serde_json::json!({"depth": 3}),
    );

    let created = repo.create(&crawl).await.expect("Failed to create crawl");
    assert_eq!(created.id, crawl_id);
    assert_eq!(created.team_id, team_id);
    assert_eq!(created.status, CrawlStatus::Queued);

    let found = repo
        .find_by_id(crawl_id)
        .await
        .expect("Failed to find crawl by id");
    let found = found.expect("Crawl should be found by id");
    assert_eq!(found.id, crawl_id);
    assert_eq!(found.team_id, team_id);
    assert_eq!(found.status, CrawlStatus::Queued);

    cleanup_crawls(&app, team_id).await;
}

/// 测试通过不存在的 ID 查询：应返回 None
#[tokio::test]
async fn test_find_by_id_returns_none_for_unknown() {
    let app = create_test_app_no_worker().await;
    let repo = CrawlRepositoryImpl::new(app.db_pool.clone());

    let unknown_id = Uuid::new_v4();
    let result = repo
        .find_by_id(unknown_id)
        .await
        .expect("Failed to query unknown crawl id");
    assert!(result.is_none(), "Should return None for unknown crawl id");
}

/// 测试更新 Crawl
#[tokio::test]
async fn test_update_crawl() {
    let app = create_test_app_no_worker().await;
    let repo = CrawlRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let unique_prefix = Uuid::new_v4().to_string();
    let crawl_id = Uuid::new_v4();
    let mut crawl = Crawl::new(
        crawl_id,
        team_id,
        format!("Update Crawl {}", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        serde_json::json!({}),
    );

    repo.create(&crawl).await.expect("Failed to create crawl");

    // 修改并更新
    crawl.start();
    repo.update(&crawl).await.expect("Failed to update crawl");

    let found = repo
        .find_by_id(crawl_id)
        .await
        .expect("Failed to find updated crawl");
    let found = found.expect("Updated crawl should be found");
    assert_eq!(found.status, CrawlStatus::Processing);

    cleanup_crawls(&app, team_id).await;
}

/// 测试增加总任务计数
#[tokio::test]
async fn test_increment_total_tasks() {
    let app = create_test_app_no_worker().await;
    let repo = CrawlRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let unique_prefix = Uuid::new_v4().to_string();
    let crawl_id = Uuid::new_v4();
    let crawl = Crawl::new(
        crawl_id,
        team_id,
        format!("Inc Total {}", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        serde_json::json!({}),
    );

    repo.create(&crawl).await.expect("Failed to create crawl");

    repo.increment_total_tasks(crawl_id)
        .await
        .expect("Failed to increment total tasks");
    repo.increment_total_tasks(crawl_id)
        .await
        .expect("Failed to increment total tasks second time");

    let found = repo
        .find_by_id(crawl_id)
        .await
        .expect("Failed to find crawl");
    let found = found.expect("Crawl should be found");
    assert_eq!(found.total_tasks(), 2, "Total tasks should be 2");

    cleanup_crawls(&app, team_id).await;
}

/// 测试增加已完成任务计数
#[tokio::test]
async fn test_increment_completed_tasks() {
    let app = create_test_app_no_worker().await;
    let repo = CrawlRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let unique_prefix = Uuid::new_v4().to_string();
    let crawl_id = Uuid::new_v4();
    let crawl = Crawl::new(
        crawl_id,
        team_id,
        format!("Inc Completed {}", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        serde_json::json!({}),
    );

    repo.create(&crawl).await.expect("Failed to create crawl");

    repo.increment_completed_tasks(crawl_id)
        .await
        .expect("Failed to increment completed tasks");

    let found = repo
        .find_by_id(crawl_id)
        .await
        .expect("Failed to find crawl");
    let found = found.expect("Crawl should be found");
    assert_eq!(found.completed_tasks(), 1, "Completed tasks should be 1");

    cleanup_crawls(&app, team_id).await;
}

/// 测试增加失败任务计数
#[tokio::test]
async fn test_increment_failed_tasks() {
    let app = create_test_app_no_worker().await;
    let repo = CrawlRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let unique_prefix = Uuid::new_v4().to_string();
    let crawl_id = Uuid::new_v4();
    let crawl = Crawl::new(
        crawl_id,
        team_id,
        format!("Inc Failed {}", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        serde_json::json!({}),
    );

    repo.create(&crawl).await.expect("Failed to create crawl");

    repo.increment_failed_tasks(crawl_id)
        .await
        .expect("Failed to increment failed tasks");
    repo.increment_failed_tasks(crawl_id)
        .await
        .expect("Failed to increment failed tasks second time");

    let found = repo
        .find_by_id(crawl_id)
        .await
        .expect("Failed to find crawl");
    let found = found.expect("Crawl should be found");
    assert_eq!(found.failed_tasks(), 2, "Failed tasks should be 2");

    cleanup_crawls(&app, team_id).await;
}

/// 测试更新状态
#[tokio::test]
async fn test_update_status() {
    let app = create_test_app_no_worker().await;
    let repo = CrawlRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;

    let unique_prefix = Uuid::new_v4().to_string();
    let crawl_id = Uuid::new_v4();
    let crawl = Crawl::new(
        crawl_id,
        team_id,
        format!("Status Crawl {}", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        serde_json::json!({}),
    );

    repo.create(&crawl).await.expect("Failed to create crawl");

    repo.update_status(crawl_id, CrawlStatus::Processing)
        .await
        .expect("Failed to update status to Processing");
    let found = repo
        .find_by_id(crawl_id)
        .await
        .expect("Failed to find crawl");
    assert_eq!(
        found.expect("crawl").status,
        CrawlStatus::Processing,
        "Status should be Processing"
    );

    repo.update_status(crawl_id, CrawlStatus::Completed)
        .await
        .expect("Failed to update status to Completed");
    let found = repo
        .find_by_id(crawl_id)
        .await
        .expect("Failed to find crawl");
    assert_eq!(
        found.expect("crawl").status,
        CrawlStatus::Completed,
        "Status should be Completed"
    );

    cleanup_crawls(&app, team_id).await;
}

/// 测试通过 team_id 分页查询
#[tokio::test]
async fn test_find_by_team_id_paginated() {
    let app = create_test_app_no_worker().await;
    let repo = CrawlRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;
    let unique_prefix = Uuid::new_v4().to_string();

    // 创建 3 个 crawl
    for i in 0..3 {
        let crawl = Crawl::new(
            Uuid::new_v4(),
            team_id,
            format!("Page Crawl {}-{}", unique_prefix, i),
            format!("https://{}-{}.example.com", unique_prefix, i),
            format!("https://{}-{}.example.com", unique_prefix, i),
            serde_json::json!({}),
        );
        repo.create(&crawl).await.expect("Failed to create crawl");
    }

    // 分页查询第一页（limit=2, offset=0）
    let page1 = repo
        .find_by_team_id_paginated(team_id, 2, 0)
        .await
        .expect("Failed to find paginated page 1");
    assert!(page1.len() <= 2, "Page 1 should have at most 2 items");

    // 分页查询第二页（limit=2, offset=2）
    let page2 = repo
        .find_by_team_id_paginated(team_id, 2, 2)
        .await
        .expect("Failed to find paginated page 2");

    // 验证分页不重复
    let page1_ids: Vec<_> = page1.iter().map(|c| c.id).collect();
    let page2_ids: Vec<_> = page2.iter().map(|c| c.id).collect();
    for id in &page2_ids {
        assert!(
            !page1_ids.contains(id),
            "Page 2 should not contain items from Page 1"
        );
    }

    cleanup_crawls(&app, team_id).await;
}

/// 测试统计团队 crawl 数量
#[tokio::test]
async fn test_count_by_team_id() {
    let app = create_test_app_no_worker().await;
    let repo = CrawlRepositoryImpl::new(app.db_pool.clone());
    let team_id = app.team_id;
    let unique_prefix = Uuid::new_v4().to_string();

    let count_before = repo
        .count_by_team_id(team_id)
        .await
        .expect("Failed to count before insert");

    let crawl = Crawl::new(
        Uuid::new_v4(),
        team_id,
        format!("Count Crawl {}", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        format!("https://{}.example.com", unique_prefix),
        serde_json::json!({}),
    );
    repo.create(&crawl).await.expect("Failed to create crawl");

    let count_after = repo
        .count_by_team_id(team_id)
        .await
        .expect("Failed to count after insert");

    assert_eq!(
        count_after,
        count_before + 1,
        "Count should increase by 1 after insert"
    );

    cleanup_crawls(&app, team_id).await;
}

/// 测试对不存在的 crawl 执行 increment 操作：不应报错
#[tokio::test]
async fn test_increment_on_unknown_crawl_is_noop() {
    let app = create_test_app_no_worker().await;
    let repo = CrawlRepositoryImpl::new(app.db_pool.clone());

    let unknown_id = Uuid::new_v4();

    // 对不存在的 crawl 执行 increment，不应报错（实现中检查了 Some）
    repo.increment_total_tasks(unknown_id)
        .await
        .expect("increment_total_tasks on unknown crawl should be noop");
    repo.increment_completed_tasks(unknown_id)
        .await
        .expect("increment_completed_tasks on unknown crawl should be noop");
    repo.increment_failed_tasks(unknown_id)
        .await
        .expect("increment_failed_tasks on unknown crawl should be noop");
    repo.update_status(unknown_id, CrawlStatus::Failed)
        .await
        .expect("update_status on unknown crawl should be noop");
}

/// 辅助函数：清理指定 team_id 的 crawls
async fn cleanup_crawls(app: &super::super::helpers::test_app::TestApp, team_id: Uuid) {
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session for cleanup");
    let conn = session
        .connection()
        .expect("Failed to get connection for cleanup");
    let _ = crawl::Entity::delete_many()
        .filter(crawl::Column::TeamId.eq(team_id))
        .exec(conn)
        .await;
}
