// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 内存缓存容量上限测试（审查 H3/M4）。

use super::*;

fn entry(content: Arc<str>) -> CachedRobots {
    CachedRobots {
        content,
        expires_at: Instant::now() + Duration::from_secs(60),
        fetch_failed: false,
    }
}

/// 条目上限：4096 个空内容也不能无限增长。
#[test]
fn test_memory_cache_entry_cap_is_enforced() {
    let mut cache = MemoryCache::default();
    for i in 0..(MAX_MEMORY_CACHE_ENTRIES + 8) {
        cache.insert(format!("http://h{i}.test/robots.txt"), entry(Arc::from("")));
    }
    assert_eq!(cache.entries.len(), MAX_MEMORY_CACHE_ENTRIES);
    assert_eq!(cache.total_bytes, 0);
}

/// 字节上限：两条 9 MiB 内容只能留一条（16 MiB 上限）。
#[test]
fn test_memory_cache_byte_cap_evicts_oldest() {
    let mut cache = MemoryCache::default();
    let big: Arc<str> = Arc::from("x".repeat(9 * 1024 * 1024));
    cache.insert("http://a.test/robots.txt".to_string(), entry(big.clone()));
    cache.insert("http://b.test/robots.txt".to_string(), entry(big));
    assert_eq!(cache.entries.len(), 1);
    assert_eq!(cache.total_bytes, 9 * 1024 * 1024);
    assert!(cache.entries.contains_key("http://b.test/robots.txt"));
}

/// 覆盖同一 key 必须归还旧内容的字节配额。
#[test]
fn test_memory_cache_overwrite_reclaims_bytes() {
    let mut cache = MemoryCache::default();
    cache.insert(
        "http://a.test/robots.txt".to_string(),
        entry(Arc::from("x".repeat(1024 * 1024))),
    );
    cache.insert(
        "http://a.test/robots.txt".to_string(),
        entry(Arc::from("y".repeat(1024))),
    );
    assert_eq!(cache.entries.len(), 1);
    assert_eq!(cache.total_bytes, 1024);
}

/// 单条超过总预算的内容直接丢弃。
#[test]
fn test_memory_cache_rejects_oversized_entry() {
    let mut cache = MemoryCache::default();
    let oversized: Arc<str> = Arc::from("x".repeat(MAX_MEMORY_CACHE_BYTES + 1));
    cache.insert("http://big.test/robots.txt".to_string(), entry(oversized));
    assert!(cache.entries.is_empty());
    assert_eq!(cache.total_bytes, 0);
}
