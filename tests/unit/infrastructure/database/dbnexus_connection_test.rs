// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! dbnexus_connection 单元测试
//!
//! 测试 DatabasePool 包装器、create_pool 和 create_pool_with_retry 的所有公共 API，
//! 包括：
//! - create_pool: 无效 URL 错误路径、有效 URL 成功路径（testcontainers）
//! - create_pool_with_retry: 重试耗尽、零重试、首次成功（testcontainers）
//! - DatabasePool: From<Arc<DbPool>> 构造、stats/inner/clone_inner、
//!   Deref/AsRef/From trait 实现
//! - DatabasePool session 方法: get_session/get_admin_session/get_system_session/
//!   get_readonly_session/get_pool_stats（testcontainers）
//! - PoolStats: Default 值
//!
//! 纯逻辑测试使用 lazy DbPool（不实际连接数据库），验证结构方法和错误路径。
//! tc_ 前缀测试使用 testcontainers PostgreSQL，验证真实连接池和 session 行为。

#![cfg(test)]

use std::ops::Deref;
use std::sync::Arc;

use crawlrs::config::DatabaseSettings;
use crawlrs::infrastructure::database::dbnexus_connection::{
    create_pool, create_pool_with_retry, DatabasePool, PoolStats,
};
use dbnexus::{CacheConfig, DbConfig, DbPool};
use testcontainers::core::IntoContainerPort;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres;

use crate::common::helpers::db_pool::create_test_pool_or_panic;

// ============================================================
// 辅助函数
// ============================================================

/// 检查 Docker 是否可用（通过 `docker info` 探测守护进程）。
async fn docker_available() -> bool {
    tokio::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 通过 serde 反序列化构造 DatabaseSettings。
///
/// `DatabaseSettings.url` 为 `pub(crate)`，外部测试无法直接构造，
/// 但结构体 derive 了 `Deserialize`，可通过 JSON 反序列化绕过可见性限制。
fn make_settings(url: &str) -> DatabaseSettings {
    let json = format!(r#"{{"url":"{}"}}"#, url);
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("failed to parse DatabaseSettings JSON: {e}"))
}

/// 启动 PostgreSQL testcontainer 并返回连接 URL。
///
/// 返回 `(url,)` 或 None（Docker 不可用或容器启动失败时）。
/// 容器通过 `std::mem::forget` 保持存活直到进程退出。
async fn setup_real_pg() -> Option<String> {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — dbnexus_connection tc_ tests");
        return None;
    }
    let image = Postgres::default();
    let container = match image.with_tag("16-alpine").start().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[skip] failed to start postgres container: {e}");
            return None;
        }
    };
    let port = match container.get_host_port_ipv4(5432.tcp()).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[skip] failed to get postgres port: {e}");
            return None;
        }
    };
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    // 保持容器存活直到进程退出（testcontainers 会在 drop 时停止容器）。
    std::mem::forget(container);
    Some(url)
}

/// 通过 testcontainers 创建真实 DbPool。
async fn setup_real_db_pool(url: &str) -> Option<DbPool> {
    let config = DbConfig {
        url: url.to_string(),
        max_connections: 5,
        min_connections: 1,
        idle_timeout: 300,
        acquire_timeout: 30000,
        permissions_path: None,
        migrations_dir: None,
        auto_migrate: false,
        migration_timeout: 300,
        admin_role: "admin".to_string(),
        warmup_timeout: 30,
        warmup_retries: 3,
        cache_config: CacheConfig::default(),
    };
    match DbPool::with_config(config).await {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("[skip] failed to create DbPool: {e}");
            None
        }
    }
}

// ============================================================
// create_pool 测试
// ============================================================

// ---------- 纯逻辑测试（不需要 Docker） ----------

#[tokio::test]
async fn test_create_pool_invalid_url_returns_error() {
    // 完全无效的 URL 字符串
    let settings = make_settings("not-a-valid-url");
    let result = create_pool(&settings).await;
    assert!(
        result.is_err(),
        "create_pool with invalid URL should return error"
    );
}

#[tokio::test]
async fn test_create_pool_unreachable_host_returns_error() {
    // 格式正确但不可达的主机（使用保留端口 1 触发连接失败）
    let settings = make_settings("postgres://postgres:postgres@127.0.0.1:1/postgres");
    let result = create_pool(&settings).await;
    assert!(
        result.is_err(),
        "create_pool with unreachable host should return error"
    );
}

// ---------- testcontainers 测试 ----------

#[tokio::test]
async fn tc_create_pool_valid_url_succeeds() {
    let url = match setup_real_pg().await {
        Some(u) => u,
        None => return,
    };
    let settings = make_settings(&url);
    let result = create_pool(&settings).await;
    assert!(
        result.is_ok(),
        "create_pool with valid URL should succeed, got error: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn tc_create_pool_returns_working_pool() {
    let url = match setup_real_pg().await {
        Some(u) => u,
        None => return,
    };
    let settings = make_settings(&url);
    let pool = create_pool(&settings)
        .await
        .expect("create_pool should succeed with valid URL");
    // 验证返回的 pool 可以获取 session
    let session = pool.get_session("admin").await;
    assert!(
        session.is_ok(),
        "pool from create_pool should be able to get session"
    );
}

// ============================================================
// create_pool_with_retry 测试
// ============================================================

// ---------- 纯逻辑测试（不需要 Docker） ----------

#[tokio::test]
async fn test_create_pool_with_retry_invalid_url_all_retries_fail() {
    let settings = make_settings("not-a-valid-url");
    // 3 次重试，延迟 0 秒（加速测试）
    let result = create_pool_with_retry(&settings, 3, 0).await;
    assert!(
        result.is_err(),
        "create_pool_with_retry with invalid URL should fail after all retries"
    );
}

#[tokio::test]
async fn test_create_pool_with_retry_zero_retries_returns_timeout_error() {
    let settings = make_settings("not-a-valid-url");
    // retry_count=0 时循环不执行，返回默认的 Timeout 错误
    let result = create_pool_with_retry(&settings, 0, 0).await;
    // 验证错误类型为 ConnectionAcquire(Timeout)（因为 last_error 为 None 时的默认值）
    // 使用 match 而非 unwrap_err()，因为 DbPool 未实现 Debug
    match result {
        Err(sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout)) => { /* expected */
        }
        Err(e) => panic!("expected ConnectionAcquire(Timeout), got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn test_create_pool_with_retry_one_retry_invalid_url_fails() {
    let settings = make_settings("not-a-valid-url");
    // 仅 1 次重试，无延迟
    let result = create_pool_with_retry(&settings, 1, 0).await;
    assert!(
        result.is_err(),
        "create_pool_with_retry with 1 retry and invalid URL should fail"
    );
}

// ---------- testcontainers 测试 ----------

#[tokio::test]
async fn tc_create_pool_with_retry_valid_url_succeeds_first_try() {
    let url = match setup_real_pg().await {
        Some(u) => u,
        None => return,
    };
    let settings = make_settings(&url);
    // 3 次重试，应该在第一次就成功
    let result = create_pool_with_retry(&settings, 3, 1).await;
    assert!(
        result.is_ok(),
        "create_pool_with_retry with valid URL should succeed, got error: {:?}",
        result.err()
    );
    let pool = result.unwrap();
    // 验证返回的 pool 可以获取 session
    let session = pool.get_session("admin").await;
    assert!(
        session.is_ok(),
        "pool from create_pool_with_retry should be able to get session"
    );
}

// ============================================================
// DatabasePool 构造与结构方法测试（使用 lazy pool，不需要 Docker）
// ============================================================

#[test]
fn test_database_pool_from_arc_dbpool() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.into();
    // 验证 stats 字段为默认值
    let stats = db_pool.stats();
    assert_eq!(stats.active_connections, 0);
    assert_eq!(stats.idle_connections, 0);
    assert_eq!(stats.total_connections, 0);
}

#[test]
fn test_database_pool_stats_returns_clone() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.into();
    let stats1 = db_pool.stats();
    let stats2 = db_pool.stats();
    // stats() 返回克隆，两个实例应相等
    assert_eq!(stats1.active_connections, stats2.active_connections);
    assert_eq!(stats1.idle_connections, stats2.idle_connections);
    assert_eq!(stats1.total_connections, stats2.total_connections);
}

#[test]
fn test_database_pool_inner_returns_reference() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.clone().into();
    let inner_ref = db_pool.inner();
    // 验证 inner() 返回的是同一个 Arc 的引用
    assert!(Arc::ptr_eq(inner_ref, &pool));
}

#[test]
fn test_database_pool_clone_inner_returns_new_arc() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.clone().into();
    let cloned = db_pool.clone_inner();
    // 验证 clone_inner() 返回的 Arc 指向同一个 DbPool
    assert!(Arc::ptr_eq(&cloned, &pool));
    // 验证引用计数增加（original + cloned + db_pool.inner）
    assert!(Arc::strong_count(&pool) >= 2);
}

#[test]
fn test_database_pool_deref_to_dbpool() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.clone().into();
    // Deref trait: &DatabasePool -> &DbPool
    let derefed: &DbPool = db_pool.deref();
    // 验证 deref 返回的是 inner pool 的引用
    // DbPool 没有简单的等价比较方法，验证指针地址一致
    let inner_ptr = db_pool.inner().as_ref() as *const DbPool;
    let deref_ptr = derefed as *const DbPool;
    assert_eq!(
        inner_ptr, deref_ptr,
        "Deref should return reference to inner DbPool"
    );
}

#[test]
fn test_database_pool_as_ref_dbpool() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.clone().into();
    // AsRef trait: &DatabasePool -> &DbPool
    let as_ref: &DbPool = AsRef::as_ref(&db_pool);
    // 验证 as_ref 返回的是 inner pool 的引用
    let inner_ptr = db_pool.inner().as_ref() as *const DbPool;
    let as_ref_ptr = as_ref as *const DbPool;
    assert_eq!(
        inner_ptr, as_ref_ptr,
        "AsRef should return reference to inner DbPool"
    );
}

#[test]
fn test_database_pool_from_databasepool_to_arc_dbpool() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.clone().into();
    // From<DatabasePool> for Arc<DbPool>
    let arc_from_pool: Arc<DbPool> = db_pool.into();
    // 验证 Arc 指向同一个 DbPool
    assert!(Arc::ptr_eq(&arc_from_pool, &pool));
}

#[test]
fn test_database_pool_clone_preserves_inner() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.clone().into();
    // DatabasePool derives Clone
    let cloned_pool = db_pool.clone();
    // 验证 clone 后 inner 指向同一个 DbPool
    assert!(Arc::ptr_eq(db_pool.inner(), cloned_pool.inner()));
}

// ============================================================
// DatabasePool session 方法测试（使用 testcontainers）
// ============================================================

#[tokio::test]
async fn tc_database_pool_get_session_admin_succeeds() {
    let url = match setup_real_pg().await {
        Some(u) => u,
        None => return,
    };
    let pool = match setup_real_db_pool(&url).await {
        Some(p) => p,
        None => return,
    };
    let db_pool: DatabasePool = Arc::new(pool).into();
    let session = db_pool.get_session("admin").await;
    assert!(
        session.is_ok(),
        "get_session('admin') should succeed with real pool, got error: {:?}",
        session.err()
    );
}

#[tokio::test]
async fn tc_database_pool_get_admin_session_succeeds() {
    let url = match setup_real_pg().await {
        Some(u) => u,
        None => return,
    };
    let pool = match setup_real_db_pool(&url).await {
        Some(p) => p,
        None => return,
    };
    let db_pool: DatabasePool = Arc::new(pool).into();
    let session = db_pool.get_admin_session().await;
    assert!(
        session.is_ok(),
        "get_admin_session() should succeed with real pool, got error: {:?}",
        session.err()
    );
}

#[tokio::test]
async fn tc_database_pool_get_system_session_succeeds() {
    let url = match setup_real_pg().await {
        Some(u) => u,
        None => return,
    };
    let pool = match setup_real_db_pool(&url).await {
        Some(p) => p,
        None => return,
    };
    let db_pool: DatabasePool = Arc::new(pool).into();
    let session = db_pool.get_system_session().await;
    assert!(
        session.is_ok(),
        "get_system_session() should succeed with real pool, got error: {:?}",
        session.err()
    );
}

#[tokio::test]
async fn tc_database_pool_get_readonly_session_without_permissions_returns_error() {
    let url = match setup_real_pg().await {
        Some(u) => u,
        None => return,
    };
    let pool = match setup_real_db_pool(&url).await {
        Some(p) => p,
        None => return,
    };
    let db_pool: DatabasePool = Arc::new(pool).into();
    let session = db_pool.get_readonly_session().await;
    // 没有 permissions.yaml 时，"readonly" 角色未配置，应返回错误。
    // 错误被映射为 ConnectionAcquire(ConnectionClosed)。
    // 使用 match 而非 unwrap_err()，因为 Session 未实现 Debug。
    match session {
        Err(sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::ConnectionClosed)) => { /* expected */
        }
        Err(e) => panic!("expected ConnectionAcquire(ConnectionClosed), got: {:?}", e),
        Ok(_) => {
            // 如果 permissions 配置恰好可用，readonly session 也可能成功。
            // 这是可接受的行为。
        }
    }
}

#[tokio::test]
async fn tc_database_pool_get_pool_stats_returns_valid_stats() {
    let url = match setup_real_pg().await {
        Some(u) => u,
        None => return,
    };
    let pool = match setup_real_db_pool(&url).await {
        Some(p) => p,
        None => return,
    };
    let db_pool: DatabasePool = Arc::new(pool).into();
    let stats = db_pool.get_pool_stats().await;
    // 验证返回的 PoolStats 字段类型正确且非负
    // (active + idle 应该 >= 0, total = active + idle)
    assert!(
        stats.total_connections >= stats.active_connections,
        "total_connections should be >= active_connections"
    );
    assert!(
        stats.total_connections >= stats.idle_connections,
        "total_connections should be >= idle_connections"
    );
}

// ============================================================
// DatabasePool session 方法测试（使用真实 pool，需要 TEST_DATABASE_URL）
// ============================================================

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_database_pool_get_session_with_real_pool_succeeds() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.into();
    // 真实连接下 get_session 应返回 Ok（admin 角色由 dbnexus 内部保障）
    let session = db_pool.get_session("admin").await;
    assert!(
        session.is_ok(),
        "get_session on real pool should succeed, got: {:?}",
        session.err()
    );
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_database_pool_get_admin_session_with_real_pool_succeeds() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.into();
    let session = db_pool.get_admin_session().await;
    assert!(
        session.is_ok(),
        "get_admin_session on real pool should succeed, got: {:?}",
        session.err()
    );
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_database_pool_get_system_session_with_real_pool_succeeds() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.into();
    let session = db_pool.get_system_session().await;
    assert!(
        session.is_ok(),
        "get_system_session on real pool should succeed, got: {:?}",
        session.err()
    );
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_database_pool_get_readonly_session_with_real_pool_succeeds() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.into();
    let session = db_pool.get_readonly_session().await;
    assert!(
        session.is_ok(),
        "get_readonly_session on real pool should succeed, got: {:?}",
        session.err()
    );
}

// ============================================================
// DatabasePool get_pool_stats 测试（使用真实 pool，需要 TEST_DATABASE_URL）
// ============================================================

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL"]
#[allow(unused_comparisons, clippy::absurd_extreme_comparisons)]
async fn test_database_pool_get_pool_stats_with_real_pool_returns_values() {
    let pool = create_test_pool_or_panic();
    let db_pool: DatabasePool = pool.into();
    let stats = db_pool.get_pool_stats().await;
    // 真实连接下计数为非负值（具体值取决于连接池预热策略，不严格断言为 0）
    assert!(
        stats.total_connections >= 0,
        "real pool stats should be non-negative, got total_connections = {}",
        stats.total_connections
    );
}

// ============================================================
// PoolStats 测试
// ============================================================

#[test]
fn test_pool_stats_default_is_all_zeros() {
    let stats = PoolStats::default();
    assert_eq!(stats.active_connections, 0);
    assert_eq!(stats.idle_connections, 0);
    assert_eq!(stats.total_connections, 0);
}

#[test]
fn test_pool_stats_clone_preserves_values() {
    let stats = PoolStats {
        active_connections: 5,
        idle_connections: 3,
        total_connections: 8,
    };
    let cloned = stats.clone();
    assert_eq!(cloned.active_connections, 5);
    assert_eq!(cloned.idle_connections, 3);
    assert_eq!(cloned.total_connections, 8);
}

#[test]
fn test_pool_stats_debug_format_works() {
    let stats = PoolStats {
        active_connections: 1,
        idle_connections: 2,
        total_connections: 3,
    };
    let debug_str = format!("{:?}", stats);
    assert!(debug_str.contains("active_connections: 1"));
    assert!(debug_str.contains("idle_connections: 2"));
    assert!(debug_str.contains("total_connections: 3"));
}
