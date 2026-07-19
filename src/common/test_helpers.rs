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

/// Build a real `DbPool` against the URL provided by `TEST_DATABASE_URL`.
///
/// # Panics
///
/// Panics if `TEST_DATABASE_URL` is unset or if pool construction fails.
/// No hardcoded fallback is provided to avoid credential leaks.
pub fn create_test_db_pool() -> Arc<DbPool> {
    std::thread::scope(|s| {
        let handle = s.spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for DbPool construction");
            let _guard = rt.enter();
            let url = std::env::var("TEST_DATABASE_URL")
                .expect("TEST_DATABASE_URL must be set; no hardcoded fallback");
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
