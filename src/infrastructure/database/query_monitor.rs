// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Query performance monitoring
//!
//! Provides utilities to monitor and log database query performance.

use std::time::Instant;
use log::{debug, warn};

/// Query performance monitor
///
/// Tracks query execution time and logs slow queries.
pub struct QueryMonitor {
    /// Threshold in milliseconds for logging slow queries
    slow_query_threshold_ms: u64,
}

impl QueryMonitor {
    /// Create a new query monitor with default threshold (1000ms)
    pub fn new() -> Self {
        Self {
            slow_query_threshold_ms: 1000,
        }
    }

    /// Create a new query monitor with custom threshold
    pub fn with_threshold(slow_query_threshold_ms: u64) -> Self {
        Self {
            slow_query_threshold_ms,
        }
    }

    /// Monitor a query execution
    ///
    /// Returns a guard that logs the query duration when dropped.
    pub fn monitor(&self, query_name: &str) -> QueryGuard {
        QueryGuard {
            query_name: query_name.to_string(),
            start_time: Instant::now(),
            slow_query_threshold_ms: self.slow_query_threshold_ms,
        }
    }
}

impl Default for QueryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Query guard that logs execution time on drop
pub struct QueryGuard {
    query_name: String,
    start_time: Instant,
    slow_query_threshold_ms: u64,
}

impl QueryGuard {
    /// Get the elapsed time in milliseconds
    pub fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }
}

impl Drop for QueryGuard {
    fn drop(&mut self) {
        let elapsed_ms = self.elapsed_ms();
        debug!("Query '{}' executed in {}ms", self.query_name, elapsed_ms);

        if elapsed_ms > self.slow_query_threshold_ms {
            warn!(
                "Slow query detected: '{}' took {}ms (threshold: {}ms)",
                self.query_name, elapsed_ms, self.slow_query_threshold_ms
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_monitor() {
        let monitor = QueryMonitor::with_threshold(100);
        let _guard = monitor.monitor("test_query");
        // Guard will log when dropped
    }

    #[test]
    fn test_query_monitor_default() {
        let monitor = QueryMonitor::new();
        assert_eq!(monitor.slow_query_threshold_ms, 1000);
    }
}
