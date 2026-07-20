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

use dbnexus::{DbConfig, DbPool};

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
