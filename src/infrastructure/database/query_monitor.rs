// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Query performance monitoring
//!
//! Provides utilities to monitor and log database query performance.

use log::{debug, warn};
use std::time::Instant;

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
    use std::thread::sleep;

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

    #[test]
    fn test_query_monitor_default_impl() {
        let monitor = QueryMonitor::default();
        assert_eq!(monitor.slow_query_threshold_ms, 1000);
    }

    #[test]
    fn test_query_monitor_with_threshold() {
        let monitor = QueryMonitor::with_threshold(500);
        assert_eq!(monitor.slow_query_threshold_ms, 500);
    }

    #[test]
    fn test_query_guard_elapsed_ms() {
        let monitor = QueryMonitor::with_threshold(10000);
        let guard = monitor.monitor("elapsed_test");
        sleep(std::time::Duration::from_millis(10));
        let elapsed = guard.elapsed_ms();
        assert!(elapsed >= 8);
    }

    #[test]
    fn test_query_guard_slow_query_triggers_warn() {
        // threshold 0 means any query that takes > 0ms is "slow"
        let monitor = QueryMonitor::with_threshold(0);
        let guard = monitor.monitor("slow_query_test");
        // Sleep to ensure elapsed > 0
        sleep(std::time::Duration::from_millis(2));
        // Dropping should trigger the warn log path since elapsed > 0
        drop(guard);
    }

    #[test]
    fn test_query_guard_fast_query_no_warn() {
        // Large threshold ensures the query is not considered slow
        let monitor = QueryMonitor::with_threshold(u64::MAX);
        let _guard = monitor.monitor("fast_query_test");
        // Dropping immediately should not trigger warn
    }

    #[test]
    fn test_query_monitor_multiple_guards() {
        let monitor = QueryMonitor::with_threshold(50);
        let g1 = monitor.monitor("query1");
        let g2 = monitor.monitor("query2");
        let g3 = monitor.monitor("query3");
        drop(g1);
        drop(g2);
        drop(g3);
    }
}
