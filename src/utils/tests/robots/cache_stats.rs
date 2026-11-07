use super::*;

// CacheStats tests

#[test]
fn test_cache_stats_default_is_zero() {
    let stats = CacheStats::default();
    assert_eq!(stats.hits(), 0, "default hits should be 0");
    assert_eq!(stats.misses(), 0, "default misses should be 0");
}

#[test]
fn test_cache_stats_record_hit_increments() {
    let stats = CacheStats::default();
    stats.record_hit();
    assert_eq!(stats.hits(), 1);
    assert_eq!(stats.misses(), 0);
}

#[test]
fn test_cache_stats_record_miss_increments() {
    let stats = CacheStats::default();
    stats.record_miss();
    assert_eq!(stats.hits(), 0);
    assert_eq!(stats.misses(), 1);
}

#[test]
fn test_cache_stats_multiple_hits_and_misses() {
    let stats = CacheStats::default();
    for _ in 0..5 {
        stats.record_hit();
    }
    for _ in 0..3 {
        stats.record_miss();
    }
    assert_eq!(stats.hits(), 5);
    assert_eq!(stats.misses(), 3);
}

#[test]
fn test_cache_stats_shared_via_arc() {
    // CacheStats uses Arc<AtomicU64>, so cloning the stats shares the same counters.
    let stats = CacheStats::default();
    let clone = stats.clone();

    stats.record_hit();
    clone.record_miss();

    // Both references should see the same counters.
    assert_eq!(stats.hits(), 1);
    assert_eq!(stats.misses(), 1);
    assert_eq!(clone.hits(), 1);
    assert_eq!(clone.misses(), 1);
}

#[test]
fn test_cache_stats_clone_is_shallow() {
    let stats = CacheStats::default();
    let clone = stats.clone();

    // Record on the original; the clone should reflect the change.
    stats.record_hit();
    stats.record_hit();
    assert_eq!(
        clone.hits(),
        2,
        "clone should share the same atomic counter"
    );
}

#[test]
fn test_cache_stats_independent_instances() {
    let stats_a = CacheStats::default();
    let stats_b = CacheStats::default();

    stats_a.record_hit();
    stats_b.record_miss();

    assert_eq!(stats_a.hits(), 1, "stats_a should have 1 hit");
    assert_eq!(stats_a.misses(), 0, "stats_a should have 0 misses");
    assert_eq!(stats_b.hits(), 0, "stats_b should have 0 hits");
    assert_eq!(stats_b.misses(), 1, "stats_b should have 1 miss");
}

// RobotsCheckerError Display tests
