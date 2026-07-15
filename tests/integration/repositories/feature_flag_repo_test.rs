// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Feature flag repository integration tests
//!
//! Integration tests for FeatureFlagRepositoryImpl using a real PostgreSQL database.
//! Covers find_by_name, find_by_id, list_all, find_override, list_overrides,
//! list_overrides_for_key, set_override, and delete_override.

use super::super::helpers::create_test_app_no_worker;
use crawlrs::common::time_utils;
use crawlrs::domain::repositories::feature_flag_repository::FeatureFlagRepository;
use crawlrs::infrastructure::database::entities::auth::feature_flag::{
    ActiveModel as FfActiveModel, Entity as FfEntity,
};
use crawlrs::infrastructure::database::entities::auth::feature_flag_override::{
    Column as FfoColumn, Entity as FfoEntity,
};
use crawlrs::infrastructure::repositories::feature_flag_repo_impl::FeatureFlagRepositoryImpl;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

/// 插入一条 feature_flag 记录到数据库（辅助函数）
async fn insert_feature_flag(
    app: &super::super::helpers::test_app::TestApp,
    flag_id: Uuid,
    name: &str,
    enabled: bool,
    rollout: i32,
) {
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session");
    let conn = session.connection().expect("Failed to get connection");
    let now = chrono::Utc::now().with_timezone(&time_utils::UTC_OFFSET);
    FfEntity::insert(FfActiveModel {
        id: Set(flag_id),
        name: Set(name.to_string()),
        description: Set(Some("Test flag".to_string())),
        enabled: Set(enabled),
        rollout_percentage: Set(rollout),
        metadata: Set(serde_json::json!({})),
        started_at: Set(None),
        stopped_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    })
    .exec(conn)
    .await
    .expect("Failed to insert feature flag");
}

/// 测试通过名称查询 feature flag
#[tokio::test]
async fn test_find_by_name_returns_flag() {
    let app = create_test_app_no_worker().await;
    let repo = FeatureFlagRepositoryImpl::new(app.db_pool.clone());
    let flag_id = Uuid::new_v4();
    let unique_name = format!("test_flag_name_{}", Uuid::new_v4());

    insert_feature_flag(&app, flag_id, &unique_name, true, 100).await;

    let found = repo
        .find_by_name(&unique_name)
        .await
        .expect("Failed to find flag by name");
    let found = found.expect("Flag should be found by name");
    assert_eq!(found.id, flag_id);
    assert_eq!(found.name, unique_name);
    assert!(found.enabled);
    assert_eq!(found.rollout_percentage, 100);

    cleanup_feature_flags(&app, flag_id).await;
}

/// 测试通过不存在的名称查询：应返回 None
#[tokio::test]
async fn test_find_by_name_returns_none_for_unknown() {
    let app = create_test_app_no_worker().await;
    let repo = FeatureFlagRepositoryImpl::new(app.db_pool.clone());

    let unknown_name = format!("nonexistent_flag_{}", Uuid::new_v4());
    let result = repo
        .find_by_name(&unknown_name)
        .await
        .expect("Failed to query unknown flag name");
    assert!(result.is_none(), "Should return None for unknown flag name");
}

/// 测试通过 ID 查询 feature flag
#[tokio::test]
async fn test_find_by_id_returns_flag() {
    let app = create_test_app_no_worker().await;
    let repo = FeatureFlagRepositoryImpl::new(app.db_pool.clone());
    let flag_id = Uuid::new_v4();
    let unique_name = format!("test_flag_id_{}", Uuid::new_v4());

    insert_feature_flag(&app, flag_id, &unique_name, false, 0).await;

    let found = repo
        .find_by_id(flag_id)
        .await
        .expect("Failed to find flag by id");
    let found = found.expect("Flag should be found by id");
    assert_eq!(found.id, flag_id);
    assert_eq!(found.name, unique_name);
    assert!(!found.enabled);
    assert_eq!(found.rollout_percentage, 0);

    cleanup_feature_flags(&app, flag_id).await;
}

/// 测试通过不存在的 ID 查询：应返回 None
#[tokio::test]
async fn test_find_by_id_returns_none_for_unknown() {
    let app = create_test_app_no_worker().await;
    let repo = FeatureFlagRepositoryImpl::new(app.db_pool.clone());

    let unknown_id = Uuid::new_v4();
    let result = repo
        .find_by_id(unknown_id)
        .await
        .expect("Failed to query unknown flag id");
    assert!(result.is_none(), "Should return None for unknown flag id");
}

/// 测试列出所有 feature flags
#[tokio::test]
async fn test_list_all_returns_flags() {
    let app = create_test_app_no_worker().await;
    let repo = FeatureFlagRepositoryImpl::new(app.db_pool.clone());

    let flag1_id = Uuid::new_v4();
    let flag2_id = Uuid::new_v4();
    let name1 = format!("list_all_1_{}", Uuid::new_v4());
    let name2 = format!("list_all_2_{}", Uuid::new_v4());

    insert_feature_flag(&app, flag1_id, &name1, true, 100).await;
    insert_feature_flag(&app, flag2_id, &name2, false, 50).await;

    let all = repo
        .list_all()
        .await
        .expect("Failed to list all feature flags");

    assert!(
        all.iter().any(|f| f.id == flag1_id),
        "Flag 1 should be in list_all"
    );
    assert!(
        all.iter().any(|f| f.id == flag2_id),
        "Flag 2 should be in list_all"
    );

    cleanup_feature_flags(&app, flag1_id).await;
    cleanup_feature_flags(&app, flag2_id).await;
}

/// 测试 set_override 和 find_override：创建 override 并查询
#[tokio::test]
async fn test_set_and_find_override() {
    let app = create_test_app_no_worker().await;
    let repo = FeatureFlagRepositoryImpl::new(app.db_pool.clone());
    let flag_id = Uuid::new_v4();
    let api_key_id = app.api_key_id;
    let unique_name = format!("override_flag_{}", Uuid::new_v4());

    insert_feature_flag(&app, flag_id, &unique_name, true, 100).await;

    // 设置 override
    let override_ = repo
        .set_override(flag_id, api_key_id, false)
        .await
        .expect("Failed to set override");
    assert_eq!(override_.feature_flag_id, flag_id);
    assert_eq!(override_.api_key_id, api_key_id);
    assert!(!override_.enabled);

    // 查询 override
    let found = repo
        .find_override(flag_id, api_key_id)
        .await
        .expect("Failed to find override");
    let found = found.expect("Override should be found");
    assert_eq!(found.feature_flag_id, flag_id);
    assert_eq!(found.api_key_id, api_key_id);
    assert!(!found.enabled);

    cleanup_feature_flags(&app, flag_id).await;
}

/// 测试 set_override 的更新逻辑：已存在的 override 被更新
#[tokio::test]
async fn test_set_override_updates_existing() {
    let app = create_test_app_no_worker().await;
    let repo = FeatureFlagRepositoryImpl::new(app.db_pool.clone());
    let flag_id = Uuid::new_v4();
    let api_key_id = app.api_key_id;
    let unique_name = format!("update_override_{}", Uuid::new_v4());

    insert_feature_flag(&app, flag_id, &unique_name, true, 100).await;

    // 第一次设置 override 为 false
    repo.set_override(flag_id, api_key_id, false)
        .await
        .expect("Failed to set override to false");

    // 第二次设置 override 为 true —— 应更新而不是创建新的
    repo.set_override(flag_id, api_key_id, true)
        .await
        .expect("Failed to update override to true");

    // 验证只有一条 override 且值为 true
    let overrides = repo
        .list_overrides(flag_id)
        .await
        .expect("Failed to list overrides");

    let matching: Vec<_> = overrides
        .iter()
        .filter(|o| o.api_key_id == api_key_id)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "Should have exactly 1 override for this api_key"
    );
    assert!(matching[0].enabled, "Override should be updated to true");

    cleanup_feature_flags(&app, flag_id).await;
}

/// 测试列出 feature flag 的所有 overrides
#[tokio::test]
async fn test_list_overrides_for_flag() {
    let app = create_test_app_no_worker().await;
    let repo = FeatureFlagRepositoryImpl::new(app.db_pool.clone());
    let flag_id = Uuid::new_v4();
    let api_key_id1 = app.api_key_id;
    let api_key_id2 = Uuid::new_v4();
    let unique_name = format!("list_overrides_{}", Uuid::new_v4());

    insert_feature_flag(&app, flag_id, &unique_name, true, 100).await;

    repo.set_override(flag_id, api_key_id1, true)
        .await
        .expect("Failed to set override 1");
    repo.set_override(flag_id, api_key_id2, false)
        .await
        .expect("Failed to set override 2");

    let overrides = repo
        .list_overrides(flag_id)
        .await
        .expect("Failed to list overrides for flag");

    assert!(
        overrides.iter().any(|o| o.api_key_id == api_key_id1),
        "Override 1 should be in list"
    );
    assert!(
        overrides.iter().any(|o| o.api_key_id == api_key_id2),
        "Override 2 should be in list"
    );

    cleanup_feature_flags(&app, flag_id).await;
}

/// 测试列出指定 api_key 的所有 overrides
#[tokio::test]
async fn test_list_overrides_for_key() {
    let app = create_test_app_no_worker().await;
    let repo = FeatureFlagRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;
    let flag1_id = Uuid::new_v4();
    let flag2_id = Uuid::new_v4();
    let name1 = format!("key_override_1_{}", Uuid::new_v4());
    let name2 = format!("key_override_2_{}", Uuid::new_v4());

    insert_feature_flag(&app, flag1_id, &name1, true, 100).await;
    insert_feature_flag(&app, flag2_id, &name2, true, 100).await;

    repo.set_override(flag1_id, api_key_id, true)
        .await
        .expect("Failed to set override on flag1");
    repo.set_override(flag2_id, api_key_id, false)
        .await
        .expect("Failed to set override on flag2");

    let overrides = repo
        .list_overrides_for_key(api_key_id)
        .await
        .expect("Failed to list overrides for key");

    assert!(
        overrides.iter().any(|o| o.feature_flag_id == flag1_id),
        "Override for flag1 should be in list"
    );
    assert!(
        overrides.iter().any(|o| o.feature_flag_id == flag2_id),
        "Override for flag2 should be in list"
    );

    cleanup_feature_flags(&app, flag1_id).await;
    cleanup_feature_flags(&app, flag2_id).await;
}

/// 测试删除 override
#[tokio::test]
async fn test_delete_override() {
    let app = create_test_app_no_worker().await;
    let repo = FeatureFlagRepositoryImpl::new(app.db_pool.clone());
    let flag_id = Uuid::new_v4();
    let api_key_id = app.api_key_id;
    let unique_name = format!("delete_override_{}", Uuid::new_v4());

    insert_feature_flag(&app, flag_id, &unique_name, true, 100).await;
    repo.set_override(flag_id, api_key_id, true)
        .await
        .expect("Failed to set override");

    // 删除 override
    let deleted = repo
        .delete_override(flag_id, api_key_id)
        .await
        .expect("Failed to delete override");
    assert!(
        deleted,
        "Should return true after deleting existing override"
    );

    // 验证已删除
    let found = repo
        .find_override(flag_id, api_key_id)
        .await
        .expect("Failed to find override after deletion");
    assert!(found.is_none(), "Override should be None after deletion");

    // 再次删除：应返回 false
    let deleted_again = repo
        .delete_override(flag_id, api_key_id)
        .await
        .expect("Failed to delete override second time");
    assert!(
        !deleted_again,
        "Should return false when deleting non-existent override"
    );

    cleanup_feature_flags(&app, flag_id).await;
}

/// 辅助函数：清理指定 feature_flag 及其 overrides
async fn cleanup_feature_flags(app: &super::super::helpers::test_app::TestApp, flag_id: Uuid) {
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session for cleanup");
    let conn = session
        .connection()
        .expect("Failed to get connection for cleanup");
    let _ = FfoEntity::delete_many()
        .filter(FfoColumn::FeatureFlagId.eq(flag_id))
        .exec(conn)
        .await;
    let _ = FfEntity::delete_by_id(flag_id).exec(conn).await;
}
