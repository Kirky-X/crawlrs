// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Test helpers for constructing `dbnexus::DbPool` in unit and integration tests.
//!
//! Two flavors are provided:
//!
//! - `create_test_pool_or_panic()` — builds a real `DbPool` via `DbPool::with_config(cfg)`
//!   reading URL from `TEST_DATABASE_URL` env var. Panics if env is not set, forcing
//!   CI/local to configure explicitly. The name reflects actual behavior: dbnexus 0.4
//!   with `permission` feature no longer supports sync `DbPool::try_from`, so every
//!   test requiring a `DbPool` must establish a real DB connection.
//!
//! - `create_real_test_pool()` — alias of `create_test_pool_or_panic()` retained for
//!   call sites that explicitly document "I need a real DB connection".
//!
//! **No credential is hardcoded.** Previously (pre-Task9 follow-up) a hardcoded
//! fallback URL with plaintext `USER:PASSWORD` was duplicated across 20 files —
//! source leak == local DB credential leak. This module centralizes the logic
//! and removes the hardcoded credential entirely.

use dbnexus::{DbConfig, DbPool};
use std::sync::Arc;

/// Build a `DbPool` for tests, panicking if `TEST_DATABASE_URL` is not set or
/// the pool cannot be constructed.
///
/// Despite the historical "lazy" naming, dbnexus 0.4 with `permission` feature
/// no longer supports sync `DbPool::try_from` (permission cache requires async
/// initialization). `with_config` is async and establishes a real DB connection.
/// The function name now reflects the actual contract: panic on missing env
/// or unreachable DB — no silent fallback.
///
/// # Panics
///
/// - If `TEST_DATABASE_URL` env var is not set.
/// - If `DbPool::with_config` fails (e.g., URL malformed or DB unreachable).
pub fn create_test_pool_or_panic() -> Arc<DbPool> {
    create_real_test_pool()
}

/// Build an eager `DbPool` for integration tests that require a real DB connection.
///
/// Reads `TEST_DATABASE_URL` env var. **Panics if not set** — no hardcoded fallback.
/// Forcing explicit configuration prevents source-code credential leaks.
///
/// # Panics
///
/// - If `TEST_DATABASE_URL` env var is not set.
/// - If `DbPool::with_config` fails (e.g., URL malformed or DB unreachable).
pub fn create_real_test_pool() -> Arc<DbPool> {
    let url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        panic!(
            "TEST_DATABASE_URL must be set for integration tests requiring a real DB; \
             no hardcoded fallback is provided to avoid credential leaks"
        )
    });
    std::thread::scope(|s| {
        let handle = s.spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for DbPool construction");
            let _guard = rt.enter();
            let cfg = DbConfig {
                url,
                ..Default::default()
            };
            rt.block_on(DbPool::with_config(cfg))
                .expect("failed to create real DbPool for test")
        });
        Arc::new(handle.join().expect("DbPool construction thread panicked"))
    })
}

/// Returns `true` when the test should be skipped because `TEST_DATABASE_URL`
/// is not set. The caller MUST return early when this returns `true`:
///
/// ```rust,ignore
/// #[tokio::test]
/// async fn test_real_db_round_trip() {
///     if crate::common::helpers::db_pool::require_real_db_or_skip() {
///         return;
///     }
///     // ... real DB assertions ...
/// }
/// ```
///
/// Returns `bool` (not `()`) so callers can `return` early; Rust cannot
/// unwind from a helper, and a previous `()`-returning version masked
/// skip-as-failure panics in `create_real_test_pool`.
#[allow(dead_code)]
pub fn require_real_db_or_skip() -> bool {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        eprintln!(
            "skipping: TEST_DATABASE_URL not set (set it to a real PostgreSQL URL to run this test)"
        );
        true
    } else {
        false
    }
}
