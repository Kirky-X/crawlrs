// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

// Minimal test helpers for queue_client_test and task_repository_test
// This file provides just enough functionality to run the basic tests

use sea_orm::{ConnectionTrait, Database, DatabaseConnection};
use std::sync::Arc;
use uuid::Uuid;

/// Minimal TestApp struct for testing
#[allow(dead_code)]
pub struct TestApp {
    pub db_pool: Arc<DatabaseConnection>,
    pub team_id: Uuid,
    pub api_key_id: Uuid,
}

/// Create a minimal test app without starting workers
#[allow(dead_code)]
pub async fn create_test_app_no_worker() -> TestApp {
    let db_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        let db_password =
            std::env::var("TEST_DATABASE_PASSWORD").unwrap_or_else(|_| "password".to_string());
        format!(
            "postgres://crawlrs:{}@localhost:5443/crawlrs_test",
            db_password
        )
    });

    let db = Database::connect(&db_url)
        .await
        .expect("Failed to connect to database");
    let db_pool = Arc::new(db);

    // 生成唯一的 team_id 和 api_key_id
    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();

    // 先在 teams 表中创建团队（因为 tasks 表有 foreign key 约束）
    let _ = db_pool
        .execute_unprepared(&format!(
            "INSERT INTO teams (id, name) VALUES ('{}', 'Test Team {}') ON CONFLICT (id) DO NOTHING",
            team_id,
            team_id
        ))
        .await;

    // 创建 API key（因为 tasks 表有外键约束）
    let _ = db_pool
        .execute_unprepared(&format!(
            "INSERT INTO api_keys (id, key, key_hash, team_id) VALUES ('{}', 'test-api-key-{}', 'hash-{}', '{}') ON CONFLICT (id) DO NOTHING",
            api_key_id,
            api_key_id,
            api_key_id,
            team_id
        ))
        .await;

    // 清理测试数据 - 只清理 tasks，保留 api_keys 和 teams
    let _ = db_pool.execute_unprepared("DELETE FROM tasks").await;
    let _ = db_pool
        .execute_unprepared("DELETE FROM tasks_backlog")
        .await;

    TestApp {
        db_pool,
        team_id,
        api_key_id,
    }
}

/// Create a test app (same as no_worker for now)
#[allow(dead_code)]
pub async fn create_test_app() -> TestApp {
    create_test_app_no_worker().await
}
