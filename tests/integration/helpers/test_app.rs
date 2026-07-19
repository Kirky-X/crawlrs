// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

// Minimal test helpers for queue_client_test and task_repository_test
// This file provides just enough functionality to run the basic tests

use dbnexus::{CacheConfig, DbConfig, DbPool};
use sea_orm::ConnectionTrait;
use std::sync::Arc;
use uuid::Uuid;

/// Minimal TestApp struct for testing
#[allow(dead_code)]
pub struct TestApp {
    pub db_pool: Arc<DbPool>,
    pub team_id: Uuid,
    pub api_key_id: Uuid,
}

/// Create the dbnexus DbPool connecting to the test PostgreSQL instance.
///
/// 强制要求 `TEST_DATABASE_URL` 环境变量；不提供硬编码 fallback 以避免凭据泄露。
async fn create_db_pool() -> Arc<DbPool> {
    let db_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        panic!(
            "TEST_DATABASE_URL must be set for integration tests requiring a real DB; \
             no hardcoded fallback is provided to avoid credential leaks"
        )
    });

    let config = DbConfig {
        url: db_url,
        max_connections: 10,
        min_connections: 1,
        idle_timeout: 300,
        acquire_timeout: 30000,
        permissions_path: Some("tests/integration/helpers/permissions.json".to_string()),
        migrations_dir: None,
        auto_migrate: false,
        migration_timeout: 300,
        admin_role: "admin".to_string(),
        warmup_timeout: 30,
        warmup_retries: 3,
        cache_config: CacheConfig::default(),
    };

    let pool = DbPool::with_config(config)
        .await
        .expect("Failed to create DbPool");

    Arc::new(pool)
}

/// Create a minimal test app without starting workers
#[allow(dead_code)]
pub async fn create_test_app_no_worker() -> TestApp {
    let db_pool = create_db_pool().await;

    // 生成唯一的 team_id 和 api_key_id
    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();

    // 通过 admin session 获取 sea_orm 连接执行 SQL
    let session = db_pool
        .get_session("admin")
        .await
        .expect("Failed to get db session");
    let conn = session.connection().expect("Failed to get db connection");

    // 先在 teams 表中创建团队（因为 tasks 表有 foreign key 约束）
    let _ = conn
        .execute_unprepared(&format!(
            "INSERT INTO teams (id, name) VALUES ('{}', 'Test Team {}') ON CONFLICT (id) DO NOTHING",
            team_id, team_id
        ))
        .await;

    // 创建 API key（因为 tasks 表有外键约束）
    let _ = conn
        .execute_unprepared(&format!(
            "INSERT INTO api_keys (id, key, key_hash, team_id) VALUES ('{}', 'test-api-key-{}', 'hash-{}', '{}') ON CONFLICT (id) DO NOTHING",
            api_key_id, api_key_id, api_key_id, team_id
        ))
        .await;

    // 注意：不执行全局 DELETE FROM tasks / tasks_backlog
    // 原因：并行测试时全局清理会删除其他正在运行的测试的数据，导致隔离问题
    // 各测试应使用唯一前缀（UUID）创建数据，并在测试结束后清理自己的数据
    // 参考 crawl_repo_test.rs / scrape_result_repo_test.rs 中的 cleanup_* 函数

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
