// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware,
    routing::get,
    Router,
};
use crawlrs::presentation::middleware::auth_middleware::{auth_middleware, AuthState};
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

async fn setup_app_with_db() -> (Router, DatabaseConnection) {
    // Create in-memory SQLite database for testing
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("Failed to create in-memory database");

    // Run migrations to create tables
    Migrator::up(&db, None)
        .await
        .expect("Failed to run database migrations");

    // Create test team and API key
    let team_id = Uuid::new_v4();
    let api_key = Uuid::new_v4().to_string();

    // Insert test data using SQLite syntax
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, 'test-team', datetime('now'), datetime('now'))",
        vec![team_id.into()],
    ))
    .await
    .expect("Failed to insert team");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES (?, ?, ?, datetime('now'), datetime('now'))",
        vec![Uuid::new_v4().into(), api_key.clone().into(), team_id.into()],
    ))
    .await
    .expect("Failed to insert API key");

    let auth_state = AuthState {
        db: Arc::new(db.clone()),
        auth_scope_service: None,
        team_id: Uuid::nil(), // Will be set by middleware
        api_key_id: Uuid::nil(),
        scope: crawlrs::domain::auth::ApiKeyScope::default(),
        api_key_cache: None,
    };

    let app = Router::new()
        .route("/", get(|| async { "Hello" }))
        .route("/protected", get(|| async { "Protected" }))
        .layer(middleware::from_fn_with_state(auth_state, auth_middleware));

    (app, db)
}

#[tokio::test]
async fn test_auth_middleware_missing_header() {
    let (app, _db) = setup_app_with_db().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_middleware_invalid_header() {
    let (app, _db) = setup_app_with_db().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", "Bearer invalid-key")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_middleware_valid_header() {
    let (app, db) = setup_app_with_db().await;

    // Get the API key we created
    let api_key: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT key FROM api_keys LIMIT 1",
            vec![],
        ))
        .await
        .expect("Failed to query API key")
        .expect("API key not found")
        .try_get_by::<String, _>(0)
        .expect("Failed to get API key from query result");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", format!("Bearer {}", api_key))
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_auth_middleware_rejects_nil_uuid() {
    // Create in-memory SQLite database for testing
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("Failed to create in-memory database");

    // Run migrations to create tables
    Migrator::up(&db, None)
        .await
        .expect("Failed to run database migrations");

    // Create API key with nil UUID (SECURITY ISSUE)
    let nil_uuid = Uuid::nil();
    let api_key_with_nil = Uuid::new_v4().to_string();

    // Insert test data - api_key associated with nil UUID team
    db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, 'test-team-nil', datetime('now'), datetime('now'))",
            vec![nil_uuid.into()],
        ))
        .await
        .expect("Failed to insert team record");

    db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES (?, ?, ?, datetime('now'), datetime('now'))",
            vec![Uuid::new_v4().into(), api_key_with_nil.clone().into(), nil_uuid.into()],
        ))
        .await
        .expect("Failed to insert API key record");

    let auth_state = AuthState {
        db: Arc::new(db.clone()),
        auth_scope_service: None,
        team_id: Uuid::nil(), // Will be set by middleware
        api_key_id: Uuid::nil(),
        scope: crawlrs::domain::auth::ApiKeyScope::default(),
        api_key_cache: None,
    };

    let app = Router::new()
        .route("/", get(|| async { "Hello" }))
        .route("/protected", get(|| async { "Protected" }))
        .layer(middleware::from_fn_with_state(auth_state, auth_middleware));

    // Request with API key that has nil UUID team_id should be REJECTED
    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", format!("Bearer {}", api_key_with_nil))
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    // Should return UNAUTHORIZED, not OK
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_middleware_extracts_bearer_token() {
    let (app, _db) = setup_app_with_db().await;

    // Test that Bearer prefix is correctly extracted
    let test_key = "test-api-key-12345";

    // We'll test the token extraction by checking behavior with different formats
    // Valid format: "Bearer <key>"
    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", format!("Bearer {}", test_key))
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    // With an unrecognized key, should still get UNAUTHORIZED (not a different error)
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_middleware_empty_bearer_token() {
    let (app, _db) = setup_app_with_db().await;

    // Test empty Bearer token
    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", "Bearer ")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    // Should reject empty token
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_middleware_wrong_auth_type() {
    let (app, _db) = setup_app_with_db().await;

    // Test wrong authentication type (Basic instead of Bearer)
    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", "Basic dXNlcjpwYXNz")
                .body(Body::empty())
                .expect("Failed to build request"),
        )
        .await
        .expect("Failed to get response");

    // Should reject non-Bearer auth
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_api_key_cache_reduces_db_queries() {
    use crawlrs::presentation::middleware::auth_middleware::ApiKeyCache;
    use std::time::Duration;

    // 创建缓存实例 (5分钟TTL, 100个槽位)
    let mut cache = ApiKeyCache::new(100, 300);

    // 创建测试数据
    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();
    let test_key = "test-api-key-for-cache";

    // 首次查询应该返回 None
    assert!(cache.get(test_key).is_none());

    // 插入缓存数据
    let auth_result = crawlrs::presentation::middleware::auth_middleware::CachedAuthResult {
        team_id,
        api_key_id,
        scope: crawlrs::domain::auth::ApiKeyScope::default(),
        cached_at: std::time::Instant::now(),
    };
    cache.insert(test_key.to_string(), auth_result.clone());

    // 第二次查询应该命中缓存
    let cached = cache.get(test_key);
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().team_id, team_id);

    // 测试 LRU 淘汰: 创建超过容量的事件
    for i in 0..150 {
        let key = format!("key-{}", i);
        let result = crawlrs::presentation::middleware::auth_middleware::CachedAuthResult {
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            scope: crawlrs::domain::auth::ApiKeyScope::default(),
            cached_at: std::time::Instant::now(),
        };
        cache.insert(key, result);
    }

    // 最早的 key 应该被淘汰
    assert!(cache.get(test_key).is_none());
}

#[tokio::test]
async fn test_api_key_cache_expires_after_ttl() {
    use crawlrs::presentation::middleware::auth_middleware::ApiKeyCache;

    // 创建短 TTL 的缓存 (1秒)
    let mut cache = ApiKeyCache::new(100, 1);

    let test_key = "expiring-key";
    let auth_result = crawlrs::presentation::middleware::auth_middleware::CachedAuthResult {
        team_id: Uuid::new_v4(),
        api_key_id: Uuid::new_v4(),
        scope: crawlrs::domain::auth::ApiKeyScope::default(),
        cached_at: std::time::Instant::now(),
    };

    cache.insert(test_key.to_string(), auth_result.clone());

    // 应该命中
    assert!(cache.get(test_key).is_some());

    // 等待过期
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 应该过期
    assert!(cache.get(test_key).is_none());
}
