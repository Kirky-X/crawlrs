// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! connection 单元测试
//!
//! 测试 src/infrastructure/database/connection.rs 中的公共 API：
//! - create_pool: sqlite::memory: 成功路径、无效 URL 失败路径
//! - create_pool_with_retry: 首次成功、重试耗尽、零重试边界
//! - DatabasePool: Default 构造、stats() 克隆、Deref/AsRef/Clone trait
//! - PoolStats: Clone/Debug trait

#![cfg(test)]

use std::ops::Deref;

use crawlrs::config::DatabaseSettings;
use crawlrs::infrastructure::database::connection::{
    create_pool, create_pool_with_retry, DatabasePool, PoolStats,
};

// ============================================================
// 辅助函数
// ============================================================

/// 通过 serde 反序列化构造 DatabaseSettings。
///
/// `DatabaseSettings.url` 为 `pub(crate)`，外部测试无法直接构造，
/// 但结构体 derive 了 `Deserialize`，可通过 JSON 反序列化绕过可见性限制。
fn make_settings(url: &str) -> DatabaseSettings {
    let json = format!(r#"{{"url":"{}"}}"#, url);
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("failed to parse DatabaseSettings JSON: {e}"))
}

// ============================================================
// create_pool 测试
// ============================================================

#[tokio::test]
async fn tc_create_pool_sqlite_memory_succeeds() {
    let settings = make_settings("sqlite::memory:");
    let result = create_pool(&settings).await;
    assert!(
        result.is_ok(),
        "create_pool with sqlite::memory: should succeed, got error: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn tc_create_pool_invalid_url_returns_error() {
    let settings = make_settings("not-a-valid-url");
    let result = create_pool(&settings).await;
    assert!(
        result.is_err(),
        "create_pool with invalid URL should return error"
    );
}

#[tokio::test]
async fn tc_create_pool_unreachable_host_returns_error() {
    // 格式正确但不可达的主机（使用保留端口 1 触发连接失败）
    let settings = make_settings("postgres://postgres:postgres@127.0.0.1:1/postgres");
    let result = create_pool(&settings).await;
    assert!(
        result.is_err(),
        "create_pool with unreachable host should return error"
    );
}

// ============================================================
// create_pool_with_retry 测试
// ============================================================

#[tokio::test]
async fn tc_create_pool_with_retry_sqlite_succeeds_first_try() {
    let settings = make_settings("sqlite::memory:");
    // 3 次重试，延迟 0 秒（加速测试），应在第一次就成功
    let result = create_pool_with_retry(&settings, 3, 0).await;
    assert!(
        result.is_ok(),
        "create_pool_with_retry with sqlite::memory: should succeed, got error: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn tc_create_pool_with_retry_invalid_url_fails_all_retries() {
    let settings = make_settings("not-a-valid-url");
    // 3 次重试，延迟 0 秒（加速测试）
    let result = create_pool_with_retry(&settings, 3, 0).await;
    assert!(
        result.is_err(),
        "create_pool_with_retry with invalid URL should fail after all retries"
    );
}

#[tokio::test]
async fn tc_create_pool_with_retry_zero_retries_returns_timeout_error() {
    let settings = make_settings("not-a-valid-url");
    // retry_count=0 时循环不执行，返回默认的 Timeout 错误
    let result = create_pool_with_retry(&settings, 0, 0).await;
    // 验证错误类型为 ConnectionAcquire(Timeout)（因为 last_error 为 None 时的默认值）
    // 使用 match 而非 unwrap_err()，因为 DbErr 实现了 Debug 但需要精确匹配
    match result {
        Err(sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout)) => { /* expected */ }
        Err(e) => panic!("expected ConnectionAcquire(Timeout), got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn tc_create_pool_with_retry_one_retry_invalid_url_fails() {
    let settings = make_settings("not-a-valid-url");
    // 仅 1 次重试，无延迟
    let result = create_pool_with_retry(&settings, 1, 0).await;
    assert!(
        result.is_err(),
        "create_pool_with_retry with 1 retry and invalid URL should fail"
    );
}

// ============================================================
// DatabasePool 构造与结构方法测试
// ============================================================

#[tokio::test]
async fn tc_database_pool_default_succeeds() {
    // DatabasePool::default() 内部使用 futures::executor::block_on 调用
    // create_pool，需要 Tokio 运行时上下文，因此使用 #[tokio::test]
    let pool = DatabasePool::default();
    // 验证 stats 字段被正确初始化
    let stats = pool.stats();
    assert_eq!(stats.active_connections, 1);
    assert_eq!(stats.idle_connections, 1);
    assert_eq!(stats.total_connections, 1);
}

#[tokio::test]
async fn tc_database_pool_stats_returns_clone() {
    let pool = DatabasePool::default();
    let stats1 = pool.stats();
    let stats2 = pool.stats();
    // stats() 返回克隆，两个实例应相等
    assert_eq!(stats1.active_connections, stats2.active_connections);
    assert_eq!(stats1.idle_connections, stats2.idle_connections);
    assert_eq!(stats1.total_connections, stats2.total_connections);
}

#[tokio::test]
async fn tc_database_pool_deref_to_database_connection() {
    let pool = DatabasePool::default();
    // Deref trait: &DatabasePool -> &DatabaseConnection
    let derefed: &sea_orm::DatabaseConnection = pool.deref();
    // inner 是 pub(crate)，外部无法直接访问；
    // 通过与 as_ref 指针比较验证 Deref 返回的是 inner pool 的引用
    let as_ref: &sea_orm::DatabaseConnection = AsRef::as_ref(&pool);
    let deref_ptr = derefed as *const sea_orm::DatabaseConnection;
    let as_ref_ptr = as_ref as *const sea_orm::DatabaseConnection;
    assert_eq!(
        deref_ptr, as_ref_ptr,
        "Deref and AsRef should return reference to the same inner DatabaseConnection"
    );
}

#[tokio::test]
async fn tc_database_pool_as_ref_to_database_connection() {
    let pool = DatabasePool::default();
    // AsRef trait: &DatabasePool -> &DatabaseConnection
    let as_ref: &sea_orm::DatabaseConnection = AsRef::as_ref(&pool);
    // 验证 as_ref 返回的引用可用：调用 DatabaseConnection 的方法
    let backend = as_ref.get_database_backend();
    // sqlite::memory: 后端应为 Sqlite
    assert_eq!(
        backend,
        sea_orm::DatabaseBackend::Sqlite,
        "as_ref should return usable DatabaseConnection (sqlite backend expected)"
    );
}

#[tokio::test]
async fn tc_database_pool_clone_preserves_inner() {
    let pool = DatabasePool::default();
    // DatabasePool derives Clone
    let cloned_pool = pool.clone();
    // 验证 clone 后 inner 指向同一个 DatabaseConnection（通过 Deref 指针比较）
    let original_ptr = pool.deref() as *const sea_orm::DatabaseConnection;
    let cloned_ptr = cloned_pool.deref() as *const sea_orm::DatabaseConnection;
    assert_eq!(
        original_ptr, cloned_ptr,
        "Clone should preserve inner DatabaseConnection reference (Arc semantics)"
    );
    // 验证 stats 也被克隆
    assert_eq!(
        pool.stats().active_connections,
        cloned_pool.stats().active_connections
    );
}

#[tokio::test]
async fn tc_database_pool_clone_arc_semantics() {
    // 验证 inner 是 Arc<DatabaseConnection>：clone 后两个 pool 共享同一连接
    // 通过 Deref 指针相等性验证 Arc 共享语义
    let pool = DatabasePool::default();
    let cloned = pool.clone();
    let p1 = pool.deref() as *const sea_orm::DatabaseConnection;
    let p2 = cloned.deref() as *const sea_orm::DatabaseConnection;
    assert_eq!(p1, p2, "cloned pool should share the same DatabaseConnection");
    // 再次 clone 验证多次 clone 仍共享
    let cloned2 = pool.clone();
    let p3 = cloned2.deref() as *const sea_orm::DatabaseConnection;
    assert_eq!(p1, p3, "second clone should also share the same DatabaseConnection");
}

// ============================================================
// PoolStats 测试
// ============================================================

#[test]
fn tc_pool_stats_clone_preserves_values() {
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
fn tc_pool_stats_debug_format_works() {
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

#[test]
fn tc_pool_stats_zero_values() {
    let stats = PoolStats {
        active_connections: 0,
        idle_connections: 0,
        total_connections: 0,
    };
    assert_eq!(stats.active_connections, 0);
    assert_eq!(stats.idle_connections, 0);
    assert_eq!(stats.total_connections, 0);
    // 验证零值也能正确克隆和调试
    let cloned = stats.clone();
    assert_eq!(cloned.active_connections, 0);
    let debug_str = format!("{:?}", stats);
    assert!(debug_str.contains("active_connections: 0"));
}
