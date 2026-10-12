// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! URL 去重布隆过滤器（oxcache `BloomFilter` 采用层）
//!
//! 原自研 `MmapBloom`（384 行：mmap(MAP_HUGETLB)→mmap→heap 三级降级链 +
//! 单线程零锁位操作 + `unsafe impl Send/Sync`）已替换为 oxcache
//! [`BloomFilter`]（`bloom` feature，底层 bloomfilter crate 维护）。
//!
//! ## 取舍（显式记录）
//!
//! - 丢失：mmap(MAP_HUGETLB) 2MiB 大页驻留策略与三级降级链、单线程零锁位
//!   操作；大容量下内存驻留由大页 mmap 变为堆分配（1M @1% ≈ 1.2MB 位图）。
//! - 换得：`Arc<RwLock>` 线程安全——`UrlInterner` 的 Send+Sync 由库保证，
//!   不再依赖 `unsafe impl`；假阳性率标准参数化（capacity + FPR 双参）；
//!   上游 bloomfilter crate 维护。
//!
//! 语义不变点：**仅作为 L1 快速负向缓存**——bloom 说"不存在"是权威的
//! （无假阴性），bloom 说"可能存在"需回落 HashSet/DB 精确校验；假阳性
//! 只导致多一次 HashSet 查找，不改变调用方语义。

/// 默认期望装载元素数（对齐迁移前 `MmapBloom::with_default_capacity` 的 1M 容量）。
pub const BLOOM_DEFAULT_CAPACITY: usize = 1_000_000;

/// 目标假阳性率（名义 0.01；实测 <1.5% 由测试钉住，即名义值 +50% 余量）。
pub const BLOOM_TARGET_FPR: f64 = 0.01;

/// URL 去重布隆过滤器（oxcache `BloomFilter<str>`，Arc<RwLock> 线程安全）。
///
/// 注意库语义：`Clone` 只克隆内部 `Arc`（克隆体共享同一份位图状态）；
/// `UrlInterner` 的深拷贝语义改由权威 HashSet 重放实现（见 interner.rs）。
pub type UrlBloom = oxcache::features::BloomFilter;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_constants() {
        // 容量映射钉：with_default_capacity → new(1_000_000, 0.01)
        assert_eq!(BLOOM_DEFAULT_CAPACITY, 1_000_000);
        assert_eq!(BLOOM_TARGET_FPR, 0.01);
    }

    #[test]
    fn test_new_params_recorded() {
        let bloom = UrlBloom::new(BLOOM_DEFAULT_CAPACITY, BLOOM_TARGET_FPR);
        assert_eq!(bloom.capacity(), 1_000_000);
        assert!((bloom.false_positive_rate() - 0.01).abs() < f64::EPSILON);
        assert!(bloom.is_empty());
        assert_eq!(bloom.len(), 0);
    }

    #[test]
    fn test_insert_contains() {
        let bloom = UrlBloom::new(1000, BLOOM_TARGET_FPR);
        bloom.insert("https://example.com");
        assert!(bloom.contains("https://example.com"));
        assert!(!bloom.contains("https://other.com"));
    }

    #[test]
    fn test_empty() {
        let bloom = UrlBloom::new(1000, BLOOM_TARGET_FPR);
        assert!(bloom.is_empty());
        assert_eq!(bloom.len(), 0);
        assert!(!bloom.contains("anything"));
    }

    #[test]
    fn test_clear() {
        let bloom = UrlBloom::new(1000, BLOOM_TARGET_FPR);
        bloom.insert("https://example.com");
        assert!(bloom.contains("https://example.com"));
        bloom.clear();
        assert!(bloom.is_empty());
        assert!(!bloom.contains("https://example.com"));
    }

    /// 假阳性率实测：100k 容量灌 100k 键，10 万非插入键命中率 <1.5%
    /// （名义 0.01 + 50% 余量，对齐规格 R-url-dedup-001）。
    #[test]
    fn test_false_positive_rate_under_1_5_percent() {
        let n = 100_000usize;
        let bloom = UrlBloom::new(n, BLOOM_TARGET_FPR);
        for i in 0..n {
            bloom.insert(&format!("url-{i}"));
        }
        assert_eq!(bloom.len(), n as u64);

        let probe = 100_000usize;
        let mut false_positives = 0;
        for i in n..(n + probe) {
            if bloom.contains(&format!("url-{i}")) {
                false_positives += 1;
            }
        }
        let fp_rate = false_positives as f64 / probe as f64;
        assert!(
            fp_rate < 0.015,
            "false positive rate too high: {:.3}%",
            fp_rate * 100.0
        );
    }

    /// 无假阴性：1M 容量灌满 1M 键，逐一断言 contains 全命中。
    #[test]
    fn test_no_false_negatives_at_full_capacity() {
        let n = 1_000_000usize;
        let bloom = UrlBloom::new(n, BLOOM_TARGET_FPR);
        for i in 0..n {
            bloom.insert(&format!("https://site.com/{i}"));
        }
        for i in 0..n {
            assert!(
                bloom.contains(&format!("https://site.com/{i}")),
                "false negative for key {i}"
            );
        }
    }

    #[test]
    fn test_clone_shares_state() {
        // 库语义钉：BloomFilter Clone 共享同一份位图（非深拷贝）。
        // UrlInterner 的深拷贝语义由 interner 侧权威 HashSet 重放保证。
        let bloom = UrlBloom::new(1000, BLOOM_TARGET_FPR);
        bloom.insert("https://a.com");
        let cloned = bloom.clone();
        assert!(cloned.contains("https://a.com"));
        cloned.insert("https://b.com");
        assert!(bloom.contains("https://b.com"), "clone 共享位图状态");
    }

    #[test]
    fn test_observable_stats_after_insert() {
        let bloom = UrlBloom::new(1000, BLOOM_TARGET_FPR);
        assert_eq!(bloom.set_bits(), 0, "空过滤器无置位");
        bloom.insert("https://a.com");
        assert!(bloom.set_bits() > 0, "插入后应有置位");
        assert!(bloom.hash_count() > 0);
        assert!(bloom.load_factor() > 0.0);
    }

    /// 线程安全由库 Arc<RwLock> 承载：多线程并发 insert/lookup 无数据竞争，
    /// 且全部已插入键最终可查（Send+Sync 不再依赖 unsafe impl）。
    #[test]
    fn test_concurrent_insert_lookup_no_data_race() {
        use std::sync::Arc;
        use std::thread;

        let bloom = Arc::new(UrlBloom::new(50_000, BLOOM_TARGET_FPR));
        let threads = 4usize;
        let per_thread = 10_000usize;

        let mut handles = Vec::new();
        for t in 0..threads {
            let bloom = Arc::clone(&bloom);
            handles.push(thread::spawn(move || {
                for i in 0..per_thread {
                    let key = format!("https://t{t}.com/{i}");
                    bloom.insert(&key);
                    assert!(bloom.contains(&key), "insert 后必须可查: {key}");
                }
            }));
        }
        for handle in handles {
            handle.join().expect("thread must not panic");
        }

        // 全部线程写入的键均可查（无假阴性）
        for t in 0..threads {
            for i in 0..per_thread {
                assert!(
                    bloom.contains(&format!("https://t{t}.com/{i}")),
                    "并发写入后丢失: t{t}/{i}"
                );
            }
        }
        assert_eq!(bloom.len(), (threads * per_thread) as u64);
    }
}
