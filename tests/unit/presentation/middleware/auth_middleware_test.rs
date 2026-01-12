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
    let db = Database::connect("sqlite::memory:").await.unwrap();

    // Run migrations to create tables
    Migrator::up(&db, None).await.unwrap();

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
    .unwrap();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES (?, ?, ?, datetime('now'), datetime('now'))",
        vec![Uuid::new_v4().into(), api_key.clone().into(), team_id.into()],
    ))
    .await
    .unwrap();

    let auth_state = AuthState {
        db: Arc::new(db.clone()),
        team_id: Uuid::nil(), // Will be set by middleware
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
                .unwrap(),
        )
        .await
        .unwrap();

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
                .unwrap(),
        )
        .await
        .unwrap();

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
        .unwrap()
        .unwrap()
        .try_get_by::<String, _>(0)
        .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", format!("Bearer {}", api_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_auth_middleware_rejects_nil_uuid() {
    // Create in-memory SQLite database for testing
    let db = Database::connect("sqlite::memory:").await.unwrap();

    // Run migrations to create tables
    Migrator::up(&db, None).await.unwrap();

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
        .unwrap();

    db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES (?, ?, ?, datetime('now'), datetime('now'))",
            vec![Uuid::new_v4().into(), api_key_with_nil.clone().into(), nil_uuid.into()],
        ))
        .await
        .unwrap();

    let auth_state = AuthState {
        db: Arc::new(db.clone()),
        team_id: Uuid::nil(), // Will be set by middleware
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
                .unwrap(),
        )
        .await
        .unwrap();

    // Should return UNAUTHORIZED, not OK
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
