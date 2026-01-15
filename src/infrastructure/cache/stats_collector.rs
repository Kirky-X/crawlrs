// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Cache statistics collector for centralized cache metrics management.
//!
//! This module provides a thread-safe statistics collector that eliminates
//! repetitive `.lock().expect()` patterns in cache implementations.

use crate::infrastructure::cache::types::CacheStats;
use std::sync::{Arc, Mutex};

/// Thread-safe cache statistics collector.
///
/// This structure encapsulates cache statistics management and provides
/// ergonomic methods for updating and retrieving statistics without
/// requiring repetitive lock handling.
///
/// # Example
///
/// ```rust
/// use crate::infrastructure::cache::stats_collector::CacheStatsCollector;
///
/// let collector = CacheStatsCollector::new();
/// collector.record_hit();
/// let stats = collector.snapshot();
/// ```
pub struct CacheStatsCollector {
    stats: Arc<Mutex<CacheStats>>,
}

impl CacheStatsCollector {
    /// Creates a new cache statistics collector.
    #[inline]
    pub fn new() -> Self {
        Self {
            stats: Arc::new(Mutex::new(CacheStats::default())),
        }
    }

    /// Creates a new collector from an existing stats reference.
    #[inline]
    pub fn from_stats(stats: Arc<Mutex<CacheStats>>) -> Self {
        Self { stats }
    }

    /// Records a cache hit.
    #[inline]
    pub fn record_hit(&self) {
        if let Ok(mut guard) = self.stats.lock() {
            guard.hits += 1;
        }
    }

    /// Records a cache miss.
    #[inline]
    pub fn record_miss(&self) {
        if let Ok(mut guard) = self.stats.lock() {
            guard.misses += 1;
        }
    }

    /// Records cache evictions.
    #[inline]
    pub fn record_evictions(&self, count: usize) {
        if let Ok(mut guard) = self.stats.lock() {
            guard.evictions += count as u64;
        }
    }

    /// Records a cache store operation.
    #[inline]
    pub fn record_store(&self) {
        if let Ok(mut guard) = self.stats.lock() {
            guard.stores += 1;
        }
    }

    /// Records cache compression savings.
    #[inline]
    pub fn record_compression_saves(&self, bytes_saved: u64) {
        if let Ok(mut guard) = self.stats.lock() {
            guard.compression_saves += bytes_saved;
        }
    }

    /// Records preheat hits.
    #[inline]
    pub fn record_preheat_hit(&self) {
        if let Ok(mut guard) = self.stats.lock() {
            guard.preheat_hits += 1;
        }
    }

    /// Records preheat hits for multiple entries.
    #[inline]
    pub fn record_preheat_hits(&self, count: usize) {
        if let Ok(mut guard) = self.stats.lock() {
            guard.preheat_hits += count as u64;
        }
    }

    /// Executes a closure while holding the stats lock.
    ///
    /// This provides controlled access to the underlying stats for
    /// complex operations that require multiple field updates.
    #[inline]
    pub fn with_stats<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&CacheStats) -> R,
    {
        let guard = self.stats.lock().unwrap();
        f(&guard)
    }

    /// Executes a closure while holding the stats lock mutably.
    ///
    /// This provides controlled access to the underlying stats for
    /// complex operations that require multiple field updates.
    #[inline]
    pub fn with_stats_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut CacheStats) -> R,
    {
        let mut guard = self.stats.lock().unwrap();
        f(&mut guard)
    }

    /// Takes a snapshot of the current statistics.
    #[inline]
    pub fn snapshot(&self) -> CacheStats {
        let guard = self.stats.lock().unwrap();
        guard.clone()
    }

    /// Resets all statistics to zero.
    #[inline]
    pub fn reset(&self) {
        let mut guard = self.stats.lock().unwrap();
        *guard = CacheStats::default();
    }

    /// Returns the number of hits.
    #[inline]
    pub fn hits(&self) -> u64 {
        let guard = self.stats.lock().unwrap();
        guard.hits
    }

    /// Returns the number of misses.
    #[inline]
    pub fn misses(&self) -> u64 {
        let guard = self.stats.lock().unwrap();
        guard.misses
    }

    /// Returns the hit ratio (hits / total accesses).
    #[inline]
    pub fn hit_ratio(&self) -> f64 {
        let guard = self.stats.lock().unwrap();
        let total = guard.hits + guard.misses;
        if total == 0 {
            0.0
        } else {
            guard.hits as f64 / total as f64
        }
    }

    /// Returns the total number of evictions.
    #[inline]
    pub fn evictions(&self) -> u64 {
        let guard = self.stats.lock().unwrap();
        guard.evictions
    }

    /// Returns the total number of stores.
    #[inline]
    pub fn stores(&self) -> u64 {
        let guard = self.stats.lock().unwrap();
        guard.stores
    }

    /// Returns a reference to the underlying stats for advanced usage.
    #[inline]
    pub fn stats(&self) -> &Arc<Mutex<CacheStats>> {
        &self.stats
    }
}

impl Default for CacheStatsCollector {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_hit() {
        let collector = CacheStatsCollector::new();
        collector.record_hit();
        collector.record_hit();
        assert_eq!(collector.hits(), 2);
        assert_eq!(collector.misses(), 0);
    }

    #[test]
    fn test_record_miss() {
        let collector = CacheStatsCollector::new();
        collector.record_miss();
        collector.record_miss();
        collector.record_miss();
        assert_eq!(collector.misses(), 3);
    }

    #[test]
    fn test_record_evictions() {
        let collector = CacheStatsCollector::new();
        collector.record_evictions(5);
        assert_eq!(collector.evictions(), 5);
    }

    #[test]
    fn test_hit_ratio() {
        let collector = CacheStatsCollector::new();
        collector.record_hit();
        collector.record_hit();
        collector.record_hit();
        collector.record_miss();
        assert_eq!(collector.hit_ratio(), 0.75);
    }

    #[test]
    fn test_snapshot() {
        let collector = CacheStatsCollector::new();
        collector.record_hit();
        collector.record_miss();
        let snapshot = collector.snapshot();
        assert_eq!(snapshot.hits, 1);
        assert_eq!(snapshot.misses, 1);
    }

    #[test]
    fn test_reset() {
        let collector = CacheStatsCollector::new();
        collector.record_hit();
        collector.record_miss();
        collector.reset();
        assert_eq!(collector.hits(), 0);
        assert_eq!(collector.misses(), 0);
    }

    #[test]
    fn test_with_stats() {
        let collector = CacheStatsCollector::new();
        collector.record_hit();
        let result = collector.with_stats(|stats| stats.hits + stats.misses);
        assert_eq!(result, 1);
    }

    #[test]
    fn test_with_stats_mut() {
        let collector = CacheStatsCollector::new();
        collector.record_hit();
        collector.with_stats_mut(|stats| {
            stats.hits += 10;
            stats.hits
        });
        assert_eq!(collector.hits(), 11);
    }
}
