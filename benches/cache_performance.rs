// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use crawlrs::search::aggregator::enhanced::EnhancedSearchAggregatorBuilder;

/// 缓存性能基准测试
///
/// 测试不同缓存配置下的性能表现
fn benchmark_cache_hit_rate(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_performance");

    for ttl in [300, 600, 1800, 3600].iter() {
        group.bench_with_input(
            BenchmarkId::new("cache_ttl", ttl),
            ttl,
            |b, &ttl| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let aggregator = EnhancedSearchAggregatorBuilder::new()
                            .cache_ttl_seconds(*ttl)
                            .build()
                            .await
                            .unwrap();

                        // 第一次查询（缓存未命中）
                        let _ = aggregator.search("rust programming", 10, None, None).await;

                        // 后续查询（应该命中缓存）
                        for _ in 0..10 {
                            black_box(
                                aggregator.search("rust programming", 10, None, None).await,
                            );
                        }
                    });
            },
        );
    }

    group.finish();
}

/// 缓存容量测试
fn benchmark_cache_capacity(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_capacity");

    for capacity in [100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("max_entries", capacity),
            capacity,
            |b, &capacity| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let aggregator = EnhancedSearchAggregatorBuilder::new()
                            .cache_max_entries(*capacity)
                            .build()
                            .await
                            .unwrap();

                        // 模拟大量不同查询
                        for i in 0..100 {
                            let query = format!("test query {}", i);
                            black_box(aggregator.search(&query, 5, None, None).await);
                        }

                        // 重复查询之前的部分以测试缓存命中
                        for i in 0..10 {
                            let query = format!("test query {}", i);
                            black_box(aggregator.search(&query, 5, None, None).await);
                        }
                    });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_cache_hit_rate, benchmark_cache_capacity);
criterion_main!(benches);
