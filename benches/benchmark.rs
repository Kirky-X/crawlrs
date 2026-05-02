// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in project root for full license information.

//! 性能基准测试套件
//!
//! 该模块包含对 crawlrs 系统核心组件的性能基准测试

use crawlrs::domain::models::task_domain::TaskType;
use crawlrs::domain::models::task_model::Task;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;
use tokio::runtime::Runtime;
use uuid::Uuid;

/// 基准测试：任务创建性能
///
/// 测试在不同并发级别下创建任务的性能表现
fn benchmark_task_creation(c: &mut Criterion) {
    let _rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("task_creation");

    // 测试内存中的任务创建
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("memory_creation", size),
            size,
            |b, &size| {
                b.iter(|| {
                    let mut tasks = Vec::new();
                    for i in 0..size {
                        let task = Task::new(
                            Uuid::new_v4(),
                            TaskType::Scrape,
                            Uuid::new_v4(),
                            Uuid::new_v4(),
                            format!("https://example{}.com", i),
                            serde_json::json!({"test": true}),
                        );
                        tasks.push(task);
                    }
                    black_box(tasks)
                });
            },
        );
    }

    group.finish();
}

/// 基准测试：任务状态转换
///
/// 测试任务在不同状态之间转换的性能
fn benchmark_task_status_transitions(c: &mut Criterion) {
    let _rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("task_status_transitions");

    // 测试单个任务的状态转换
    group.bench_function("single_task_lifecycle", |b| {
        b.iter(|| {
            let mut task = Task::new(
                Uuid::new_v4(),
                TaskType::Scrape,
                Uuid::new_v4(),
                Uuid::new_v4(),
                "https://example.com".to_string(),
                serde_json::json!({}),
            );

            // 模拟完整的任务生命周期
            task.start();
            task.complete();

            black_box(task)
        });
    });

    // 测试批量任务状态转换
    for batch_size in [10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            batch_size,
            |b, &batch_size| {
                b.iter(|| {
                    let mut tasks = Vec::new();
                    for i in 0..batch_size {
                        let mut task = Task::new(
                            Uuid::new_v4(),
                            TaskType::Scrape,
                            Uuid::new_v4(),
                            Uuid::new_v4(),
                            format!("https://example{}.com", i),
                            serde_json::json!({}),
                        );
                        task.start();
                        tasks.push(task);
                    }
                    black_box(tasks)
                });
            },
        );
    }

    group.finish();
}

/// 基准测试：JSON序列化/反序列化
///
/// 测试任务对象的JSON序列化和反序列化性能
fn benchmark_json_serialization(c: &mut Criterion) {
    let _rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("json_serialization");

    // 创建测试任务
    let task = Task::new(
        Uuid::new_v4(),
        TaskType::Scrape,
        Uuid::new_v4(),
        Uuid::new_v4(),
        "https://example.com".to_string(),
        serde_json::json!({
            "test": "data",
            "nested": {
                "key": "value"
            }
        }),
    );

    group.bench_function("serialize_task", |b| {
        b.iter(|| {
            let json = serde_json::to_string(&task).unwrap();
            black_box(json)
        });
    });

    group.bench_function("deserialize_task", |b| {
        let json = serde_json::to_string(&task).unwrap();
        b.iter(|| {
            let parsed: Task = serde_json::from_str(&json).unwrap();
            black_box(parsed)
        });
    });

    group.finish();
}

/// 基准测试：URL解析性能
///
/// 测试URL解析的性能
fn benchmark_url_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_parsing");

    let urls = vec![
        "https://example.com/path/to/resource",
        "https://api.example.com/v1/users/123",
        "https://www.example.com/search?q=rust+programming",
        "https://example.com/path?param1=value1&param2=value2",
    ];

    group.bench_function("parse_simple_url", |b| {
        b.iter(|| {
            for url in &urls {
                let parsed = url::Url::parse(url);
                black_box(parsed);
            }
        });
    });

    group.finish();
}

/// 基准测试：UUID生成性能
///
/// 测试UUID生成的性能
fn benchmark_uuid_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("uuid_generation");

    group.bench_function("generate_single_uuid", |b| {
        b.iter(|| {
            let uuid = Uuid::new_v4();
            black_box(uuid);
        });
    });

    group.bench_function("generate_batch_uuids_100", |b| {
        b.iter(|| {
            let mut uuids = Vec::new();
            for _ in 0..100 {
                uuids.push(Uuid::new_v4());
            }
            black_box(uuids);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_task_creation,
    benchmark_task_status_transitions,
    benchmark_json_serialization,
    benchmark_url_parsing,
    benchmark_uuid_generation
);
criterion_main!(benches);
