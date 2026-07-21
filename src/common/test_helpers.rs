// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Centralized test helpers for `src/` internal `#[cfg(test)] mod tests` blocks.
//!
//! This module is only compiled under `#[cfg(test)]` and provides shared
//! utilities that would otherwise be duplicated across 16+ `src/` modules.

#![cfg(test)]

use std::sync::Arc;
use std::sync::OnceLock;

use dbnexus::{DbConfig, DbPool};
use tokio::sync::Mutex;

/// Resolve the test database URL from the environment.
///
/// Tries `TEST_DATABASE_URL` first (preferred), then falls back to
/// `DATABASE_URL` (CI convention) so the same tests run in both local
/// and CI environments without duplicate configuration.
///
/// Returns `None` when neither variable is set, signaling the caller to
/// skip the test rather than panic.
pub fn resolve_test_database_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .ok()
}

/// Returns `true` when no test database is available and the caller should
/// skip execution. Prints a `[skip]` notice to stderr so skipped tests are
/// visible in CI logs.
pub fn skip_if_no_test_db() -> bool {
    if resolve_test_database_url().is_none() {
        eprintln!("[skip] TEST_DATABASE_URL/DATABASE_URL not set — test requires real DbPool");
        return true;
    }
    false
}

/// Build a real `DbPool` against the URL provided by `TEST_DATABASE_URL`
/// (or `DATABASE_URL` as fallback).
///
/// All repositories in this project consistently use the `admin` role
/// (see `src/infrastructure/database/repositories/*.rs`), which dbnexus
/// grants full access without requiring `permissions.yaml`. Tests therefore
/// do not load `permissions.yaml`, avoiding YAML/JSON parsing differences
/// in dbnexus 0.4.0.
///
/// # Panics
///
/// Panics if neither `TEST_DATABASE_URL` nor `DATABASE_URL` is set, or if
/// pool construction fails. No hardcoded fallback is provided to avoid
/// credential leaks.
pub fn create_test_db_pool() -> Arc<DbPool> {
    std::thread::scope(|s| {
        let handle = s.spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for DbPool construction");
            let _guard = rt.enter();
            let url = resolve_test_database_url()
                .expect("TEST_DATABASE_URL or DATABASE_URL must be set; no hardcoded fallback");
            rt.block_on(async {
                let cfg = DbConfig {
                    url,
                    ..Default::default()
                };
                DbPool::with_config(cfg).await
            })
            .expect("failed to create DbPool for test")
        });
        Arc::new(handle.join().expect("DbPool construction thread panicked"))
    })
}

/// 全局 mutex 用于序列化所有 `acquire_next` 相关测试。
///
/// `acquire_next` 获取任何 `queued` task（不按 team_id 过滤），共享测试数据库
/// + 并行测试会导致测试间相互干扰：一个测试创建的 task 可能被另一个测试的
/// `acquire_next` 获取，导致返回 `None`（flaky test）。此 mutex 确保同一时间
/// 只有一个 `acquire_next` 测试在运行，消除竞争条件。
///
/// 用法：
/// ```ignore
/// #[tokio::test]
/// async fn test_acquire_next() {
///     let _guard = acquire_next_test_mutex().lock().await;
///     // 测试逻辑
/// }
/// ```
pub fn acquire_next_test_mutex() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}
