// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Auth scope repository integration tests
//!
//! Integration tests for AuthScopeRepositoryImpl using a real PostgreSQL database.
//! Covers find_by_api_key_id, find_by_api_key, upsert, and delete_by_api_key_id.

use super::super::helpers::create_test_app_no_worker;
use crawlrs::domain::auth::ApiKeyScope;
use crawlrs::domain::repositories::auth_scope_repository::AuthScopeRepository;
use crawlrs::infrastructure::database::entities::auth::scope::{
    Column as ScopeColumn, Entity as ScopeEntity,
};
use crawlrs::infrastructure::repositories::auth_scope_repo_impl::AuthScopeRepositoryImpl;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

/// 测试 upsert 创建新 scope 并通过 api_key_id 查询
#[tokio::test]
async fn test_upsert_creates_scope_and_find_by_api_key_id() {
    let app = create_test_app_no_worker().await;
    let repo = AuthScopeRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;

    // upsert 创建新 scope（默认 api_key 没有 scope）
    let scope = ApiKeyScope::full_access();
    let result = repo
        .upsert(api_key_id, scope.clone())
        .await
        .expect("Failed to upsert scope");
    assert_eq!(result, scope, "Upserted scope should match input");

    // 通过 api_key_id 查询
    let found = repo
        .find_by_api_key_id(api_key_id)
        .await
        .expect("Failed to find scope by api_key_id");
    let found = found.expect("Scope should be found after upsert");
    assert_eq!(found, scope, "Found scope should match upserted scope");
    assert!(found.read, "read should be true for full_access");
    assert!(found.write, "write should be true for full_access");
    assert!(found.admin, "admin should be true for full_access");

    cleanup_scopes(&app, api_key_id).await;
}

/// 测试 upsert 更新已存在的 scope
#[tokio::test]
async fn test_upsert_updates_existing_scope() {
    let app = create_test_app_no_worker().await;
    let repo = AuthScopeRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;

    // 第一次 upsert：full_access
    let full = ApiKeyScope::full_access();
    repo.upsert(api_key_id, full.clone())
        .await
        .expect("Failed to upsert full_access");

    // 第二次 upsert：read_only（应更新，不创建新记录）
    let read_only = ApiKeyScope::read_only();
    repo.upsert(api_key_id, read_only.clone())
        .await
        .expect("Failed to upsert read_only");

    // 验证 scope 已更新为 read_only
    let found = repo
        .find_by_api_key_id(api_key_id)
        .await
        .expect("Failed to find scope after update");
    let found = found.expect("Scope should be found");
    assert_eq!(found, read_only, "Scope should be updated to read_only");
    assert!(found.read, "read should be true");
    assert!(!found.write, "write should be false for read_only");
    assert!(!found.admin, "admin should be false for read_only");

    cleanup_scopes(&app, api_key_id).await;
}

/// 测试通过不存在的 api_key_id 查询：应返回 None
#[tokio::test]
async fn test_find_by_api_key_id_returns_none_for_unknown() {
    let app = create_test_app_no_worker().await;
    let repo = AuthScopeRepositoryImpl::new(app.db_pool.clone());

    let unknown_id = Uuid::new_v4();
    let result = repo
        .find_by_api_key_id(unknown_id)
        .await
        .expect("Failed to query unknown api_key_id");
    assert!(
        result.is_none(),
        "Should return None for unknown api_key_id"
    );
}

/// 测试通过 api_key 字符串查询 scope
#[tokio::test]
async fn test_find_by_api_key() {
    let app = create_test_app_no_worker().await;
    let repo = AuthScopeRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;
    // test_app.rs 创建的 api_key 的 key 字符串
    let key_string = format!("test-api-key-{}", api_key_id);

    let scope = ApiKeyScope::default();
    repo.upsert(api_key_id, scope.clone())
        .await
        .expect("Failed to upsert scope");

    let found = repo
        .find_by_api_key(&key_string)
        .await
        .expect("Failed to find scope by api_key string");
    let found = found.expect("Scope should be found by api_key string");
    assert_eq!(found, scope, "Found scope should match upserted scope");

    cleanup_scopes(&app, api_key_id).await;
}

/// 测试通过不存在的 api_key 字符串查询：应返回 None
#[tokio::test]
async fn test_find_by_api_key_returns_none_for_unknown() {
    let app = create_test_app_no_worker().await;
    let repo = AuthScopeRepositoryImpl::new(app.db_pool.clone());

    let unknown_key = "nonexistent-key-string";
    let result = repo
        .find_by_api_key(unknown_key)
        .await
        .expect("Failed to query unknown api_key");
    assert!(result.is_none(), "Should return None for unknown api_key");
}

/// 测试删除 scope
#[tokio::test]
async fn test_delete_by_api_key_id() {
    let app = create_test_app_no_worker().await;
    let repo = AuthScopeRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;

    // 先创建 scope
    repo.upsert(api_key_id, ApiKeyScope::full_access())
        .await
        .expect("Failed to upsert scope");

    // 删除 scope
    let deleted = repo
        .delete_by_api_key_id(api_key_id)
        .await
        .expect("Failed to delete scope");
    assert!(deleted, "Should return true after deleting existing scope");

    // 验证已删除
    let found = repo
        .find_by_api_key_id(api_key_id)
        .await
        .expect("Failed to find scope after delete");
    assert!(found.is_none(), "Scope should be None after delete");

    // 再次删除：应返回 false
    let deleted_again = repo
        .delete_by_api_key_id(api_key_id)
        .await
        .expect("Failed to delete scope second time");
    assert!(
        !deleted_again,
        "Should return false when deleting non-existent scope"
    );
}

/// 测试 upsert denied scope
#[tokio::test]
async fn test_upsert_denied_scope() {
    let app = create_test_app_no_worker().await;
    let repo = AuthScopeRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;

    let denied = ApiKeyScope::denied();
    repo.upsert(api_key_id, denied.clone())
        .await
        .expect("Failed to upsert denied scope");

    let found = repo
        .find_by_api_key_id(api_key_id)
        .await
        .expect("Failed to find denied scope");
    let found = found.expect("Denied scope should be found");
    assert_eq!(found, denied, "Found scope should match denied");
    assert!(!found.read, "read should be false for denied");
    assert!(!found.write, "write should be false for denied");
    assert!(!found.admin, "admin should be false for denied");

    cleanup_scopes(&app, api_key_id).await;
}

/// tc_upsert_with_custom_limits: upsert 设置自定义 search_limit 和 scrape_limit
#[tokio::test]
async fn tc_upsert_with_custom_limits() {
    let app = create_test_app_no_worker().await;
    let repo = AuthScopeRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;

    let custom_scope = ApiKeyScope::with_custom_limits(true, false, false, 500, 100);

    repo.upsert(api_key_id, custom_scope.clone())
        .await
        .expect("Failed to upsert custom scope");

    let found = repo
        .find_by_api_key_id(api_key_id)
        .await
        .expect("Failed to find custom scope");
    let found = found.expect("Custom scope should be found");
    assert_eq!(found, custom_scope, "Found scope should match custom");

    cleanup_scopes(&app, api_key_id).await;
}

/// tc_find_by_api_key_id_returns_none_after_delete: 删除后查询返回 None
#[tokio::test]
async fn tc_find_by_api_key_id_returns_none_after_delete() {
    let app = create_test_app_no_worker().await;
    let repo = AuthScopeRepositoryImpl::new(app.db_pool.clone());
    let api_key_id = app.api_key_id;

    // 创建 scope
    repo.upsert(api_key_id, ApiKeyScope::full_access())
        .await
        .expect("Failed to upsert scope");

    // 确认存在
    let before = repo
        .find_by_api_key_id(api_key_id)
        .await
        .expect("Failed to find before delete");
    assert!(before.is_some(), "Scope should exist before delete");

    // 删除
    repo.delete_by_api_key_id(api_key_id)
        .await
        .expect("Failed to delete scope");

    // 确认已删除
    let after = repo
        .find_by_api_key_id(api_key_id)
        .await
        .expect("Failed to find after delete");
    assert!(after.is_none(), "Scope should be None after delete");
}

/// 辅助函数：清理指定 api_key_id 的 scopes
async fn cleanup_scopes(app: &super::super::helpers::test_app::TestApp, api_key_id: Uuid) {
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session for cleanup");
    let conn = session
        .connection()
        .expect("Failed to get connection for cleanup");
    let _ = ScopeEntity::delete_many()
        .filter(ScopeColumn::ApiKeyId.eq(api_key_id))
        .exec(conn)
        .await;
}
