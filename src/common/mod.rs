// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 通用模块
//!
//! 提供应用程序的通用功能，包括错误类型、常量定义等

pub mod constants;
pub mod error;
pub mod time_utils;

pub use constants::*;
pub use error::{CrawlRsError, CrawlRsResult};
pub use time_utils::{
    from_db_datetime, from_db_datetime_opt, to_db_datetime, to_db_datetime_opt, UTC_OFFSET,
};

/// Testcontainers integration test fixtures.
#[cfg(test)]
pub mod test_fixtures;

/// Centralized test helpers shared across `src/` `#[cfg(test)] mod tests` blocks.
#[cfg(test)]
pub mod test_helpers;

/// Test support utilities shared across modules
#[cfg(test)]
pub(crate) mod test_support {
    use once_cell::sync::Lazy;
    use std::sync::Mutex;

    /// Global mutex to serialize tests that manipulate environment variables.
    /// Environment variables are process-global, so all test modules that
    /// set/unset env vars must lock this mutex to prevent race conditions.
    pub static ENV_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    /// Backward-compatible re-export of the testcontainers fixtures module.
    pub use super::test_fixtures as testcontainers_fixtures;
}
