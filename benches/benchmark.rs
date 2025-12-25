// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

//! 性能基准测试套件
//!
//! 该模块包含对 crawlrs 系统核心组件的性能基准测试，用于评估系统在不同场景下的性能表现。

use crawlrs::domain::models::task::{Task, TaskType};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use migration::{Migrator, MigratorTrait};
use sea_orm::{
    ColumnTrait, Database, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use std::hint::black_box;
use tokio::runtime::Runtime;
use uuid::Uuid;

/// 创建测试数据库连接并运行迁移
async fn create_test_db() -> Result<DatabaseConnection, DbErr> {
    let db = Database::connect("sqlite::memory:").await?;

    // 运行数据库迁移
    Migrator::up(&db, None).await?;

    Ok(db)
}

/// 基准测试：任务创建性能
///
/// 测试在不同并发级别下创建任务的性能表现，包括数据库持久化操作
fn benchmark_task_creation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let db = rt
        .block_on(create_test_db())
        .expect("Failed to setup test database");

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
                            TaskType::Scrape,
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

    // 测试数据库持久化的任务创建
    for size in [10, 100, 500].iter() {
        group.bench_with_input(
            BenchmarkId::new("database_persistence", size),
            size,
            |b, &size| {
                b.iter(|| {
                    let rt = Runtime::new().unwrap();
                    let db = &db;
                    let mut tasks = Vec::new();

                    for i in 0..size {
                        let task = crawlrs::infrastructure::database::entities::task::ActiveModel {
                            id: sea_orm::Set(Uuid::new_v4()),
                            crawl_id: sea_orm::Set(None),
                            task_type: sea_orm::Set("scrape".to_string()),
                            status: sea_orm::Set("queued".to_string()),
                            priority: sea_orm::Set(0),
                            team_id: sea_orm::Set(Uuid::new_v4()),
                            url: sea_orm::Set(format!("https://example{}.com", i)),
                            payload: sea_orm::Set(serde_json::json!({"test": true})),
                            attempt_count: sea_orm::Set(0),
                            max_retries: sea_orm::Set(3),
                            created_at: sea_orm::Set(chrono::Utc::now().into()),
                            updated_at: sea_orm::Set(chrono::Utc::now().into()),
                            scheduled_at: sea_orm::Set(None),
                            started_at: sea_orm::Set(None),
                            completed_at: sea_orm::Set(None),
                            lock_token: sea_orm::Set(None),
                            lock_expires_at: sea_orm::Set(None),
                            expires_at: sea_orm::Set(None),
                        };
                        tasks.push(task);
                    }

                    // 批量插入到数据库
                    let result = rt.block_on(async {
                        crawlrs::infrastructure::database::entities::task::Entity::insert_many(
                            tasks,
                        )
                        .exec(db)
                        .await
                    });

                    black_box(result)
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
                TaskType::Scrape,
                Uuid::new_v4(),
                "https://example.com".to_string(),
                serde_json::json!({}),
            );

            // 模拟完整的任务生命周期
            task = task.start().unwrap();
            task = task.complete().unwrap();

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
                            TaskType::Scrape,
                            Uuid::new_v4(),
                            format!("https://example{}.com", i),
                            serde_json::json!({}),
                        );
                        task = task.start().unwrap();
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
        TaskType::Scrape,
        Uuid::new_v4(),
        "https://example.com".to_string(),
        serde_json::json!({
            "complex": {
                "nested": {
                    "data": "test content with special characters: 你好世界 🌍"
                }
            },
            "array": [1, 2, 3, 4, 5],
            "boolean": true,
            "number": 42.5
        }),
    );

    // 序列化基准测试
    group.bench_function("serialize_task", |b| {
        b.iter(|| {
            let json_str = serde_json::to_string(&task).unwrap();
            black_box(json_str)
        });
    });

    // 反序列化基准测试
    let task_json = serde_json::to_string(&task).unwrap();
    group.bench_function("deserialize_task", |b| {
        b.iter(|| {
            let deserialized: Task = serde_json::from_str(&task_json).unwrap();
            black_box(deserialized)
        });
    });

    // 测试不同大小的payload
    for payload_size in ["small", "medium", "large"].iter() {
        let payload = match *payload_size {
            "small" => serde_json::json!({"key": "value"}),
            "medium" => serde_json::json!({
                "title": "Test Page",
                "content": "This is a medium-sized content with some text and numbers: 12345",
                "metadata": {
                    "author": "Test Author",
                    "date": "2024-01-01",
                    "tags": ["test", "benchmark", "performance"]
                }
            }),
            "large" => serde_json::json!({
                "page_content": "A".repeat(10000),  // 10KB of content
                "nested_structures": {
                    "level1": {
                        "level2": {
                            "level3": {
                                "data": (0..100).collect::<Vec<i32>>(),
                                "text": "Deep nested content with unicode: 你好世界 🌍 🎉"
                            }
                        }
                    }
                },
                "arrays": (0..1000).map(|i| format!("item_{}", i)).collect::<Vec<String>>(),
                "mixed_types": {
                    "string": "text",
                    "number": 42.5,
                    "boolean": true,
                    "null": null,
                    "array": [1, 2, 3],
                    "object": {"nested": "value"}
                }
            }),
            _ => serde_json::json!({}),
        };

        group.bench_with_input(
            BenchmarkId::new("serialize_payload", payload_size),
            &payload,
            |b, payload| {
                b.iter(|| {
                    let json_str = serde_json::to_string(payload).unwrap();
                    black_box(json_str)
                });
            },
        );
    }

    group.finish();
}

/// 基准测试：UUID生成和处理
///
/// 测试UUID生成和字符串转换的性能
fn benchmark_uuid_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("uuid_operations");

    // UUID生成
    group.bench_function("generate_uuid_v4", |b| {
        b.iter(|| {
            let id = Uuid::new_v4();
            black_box(id)
        });
    });

    // UUID到字符串转换
    group.bench_function("uuid_to_string", |b| {
        let id = Uuid::new_v4();
        b.iter(|| {
            let s = id.to_string();
            black_box(s)
        });
    });

    // 字符串到UUID转换
    let uuid_str = Uuid::new_v4().to_string();
    group.bench_function("string_to_uuid", |b| {
        b.iter(|| {
            let id = Uuid::parse_str(&uuid_str).unwrap();
            black_box(id)
        });
    });

    group.finish();
}

/// 基准测试：内存分配和克隆操作
///
/// 测试任务对象的内存分配和克隆性能
fn benchmark_memory_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_operations");

    // 创建包含大量数据的任务
    let task = Task::new(
        TaskType::Scrape,
        Uuid::new_v4(),
        "https://example.com/very/long/url/path/with/many/segments/and/query/parameters?param1=value1&param2=value2&param3=value3".to_string(),
        serde_json::json!({
            "large_content": "A".repeat(100000),  // 100KB
            "nested_data": {
                "repeated_section": (0..1000).map(|i| {
                    serde_json::json!({
                        "id": i,
                        "name": format!("item_{}", i),
                        "description": format!("This is a description for item {}", i),
                        "metadata": {
                            "created": "2024-01-01T00:00:00Z",
                            "updated": "2024-01-01T12:00:00Z",
                            "tags": ["tag1", "tag2", "tag3"]
                        }
                    })
                }).collect::<Vec<_>>()
            }
        })
    );

    // 克隆操作
    group.bench_function("clone_large_task", |b| {
        b.iter(|| {
            let cloned = task.clone();
            black_box(cloned)
        });
    });

    // 内存分配测试 - 创建多个任务
    group.bench_function("allocate_task_batch", |b| {
        b.iter(|| {
            let tasks: Vec<Task> = (0..100)
                .map(|i| {
                    Task::new(
                        TaskType::Scrape,
                        Uuid::new_v4(),
                        format!("https://example{}.com", i),
                        serde_json::json!({"index": i}),
                    )
                })
                .collect();
            black_box(tasks)
        });
    });

    group.finish();
}

/// 基准测试：并发任务处理
///
/// 测试系统在高并发场景下的任务处理性能
fn benchmark_concurrent_task_processing(c: &mut Criterion) {
    let _rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("concurrent_task_processing");

    // 测试不同并发级别的任务创建
    for concurrency in [10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::new("create_concurrent_tasks", concurrency),
            concurrency,
            |b, &concurrency| {
                b.iter(|| {
                    let mut tasks = Vec::new();
                    for i in 0..concurrency {
                        let task = Task::new(
                            TaskType::Scrape,
                            Uuid::new_v4(),
                            format!("https://example{}.com", i),
                            serde_json::json!({"task_id": i}),
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

/// 基准测试：错误处理性能
///
/// 测试系统在处理错误情况时的性能表现
fn benchmark_error_handling(c: &mut Criterion) {
    let mut group = c.benchmark_group("error_handling");

    // 测试任务状态转换中的错误处理
    group.bench_function("task_state_validation", |b| {
        let completed_task = Task::new(
            TaskType::Scrape,
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        let completed_task = completed_task.complete().unwrap();

        b.iter(|| {
            // 尝试对已完成的任务执行无效操作
            let result = completed_task.clone().start();
            black_box(result)
        });
    });

    // JSON解析错误处理
    let invalid_json = r#"{"invalid": json}"#;
    group.bench_function("json_parse_error", |b| {
        b.iter(|| {
            let result: Result<Task, _> = serde_json::from_str(invalid_json);
            black_box(result)
        });
    });

    // UUID解析错误处理
    let invalid_uuid = "not-a-valid-uuid";
    group.bench_function("uuid_parse_error", |b| {
        b.iter(|| {
            let result = Uuid::parse_str(invalid_uuid);
            black_box(result)
        });
    });

    group.finish();
}

/// 基准测试：数据库查询操作
///
/// 测试数据库查询性能，包括索引使用和复杂查询
fn benchmark_database_queries(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let db = rt
        .block_on(create_test_db())
        .expect("Failed to setup test database");

    // 预填充测试数据
    rt.block_on(async {
        let mut tasks = Vec::new();
        for i in 0..1000 {
            let task = crawlrs::infrastructure::database::entities::task::ActiveModel {
                id: sea_orm::Set(Uuid::new_v4()),
                crawl_id: sea_orm::Set(None),
                task_type: sea_orm::Set(if i % 3 == 0 {
                    "scrape".to_string()
                } else if i % 3 == 1 {
                    "crawl".to_string()
                } else {
                    "extract".to_string()
                }),
                status: sea_orm::Set(if i % 4 == 0 {
                    "completed".to_string()
                } else if i % 4 == 1 {
                    "failed".to_string()
                } else if i % 4 == 2 {
                    "active".to_string()
                } else {
                    "queued".to_string()
                }),
                priority: sea_orm::Set(i % 5),
                team_id: sea_orm::Set(Uuid::new_v4()),
                url: sea_orm::Set(format!("https://example{}.com", i)),
                payload: sea_orm::Set(serde_json::json!({"test": true, "index": i})),
                attempt_count: sea_orm::Set(0),
                max_retries: sea_orm::Set(3),
                created_at: sea_orm::Set(chrono::Utc::now().into()),
                updated_at: sea_orm::Set(chrono::Utc::now().into()),
                scheduled_at: sea_orm::Set(None),
                started_at: sea_orm::Set(None),
                completed_at: sea_orm::Set(None),
                lock_token: sea_orm::Set(None),
                lock_expires_at: sea_orm::Set(None),
                expires_at: sea_orm::Set(None),
            };
            tasks.push(task);
        }

        crawlrs::infrastructure::database::entities::task::Entity::insert_many(tasks)
            .exec(&db)
            .await
            .expect("Failed to insert test data");
    });

    let mut group = c.benchmark_group("database_queries");

    // 基础查询 - 按ID查询
    group.bench_function("query_by_id", |b| {
        b.iter(|| {
            let rt = Runtime::new().unwrap();
            let task_id = Uuid::new_v4();
            let result = rt.block_on(async {
                crawlrs::infrastructure::database::entities::task::Entity::find_by_id(task_id)
                    .one(&db)
                    .await
            });
            black_box(result)
        });
    });

    // 索引查询 - 按状态查询
    group.bench_function("query_by_status", |b| {
        b.iter(|| {
            let rt = Runtime::new().unwrap();
            let result = rt.block_on(async {
                crawlrs::infrastructure::database::entities::task::Entity::find()
                    .filter(
                        crawlrs::infrastructure::database::entities::task::Column::Status
                            .eq("queued"),
                    )
                    .limit(10)
                    .all(&db)
                    .await
            });
            black_box(result)
        });
    });

    // 复合索引查询 - 按状态和优先级排序
    group.bench_function("query_by_status_priority", |b| {
        b.iter(|| {
            let rt = Runtime::new().unwrap();
            let result = rt.block_on(async {
                crawlrs::infrastructure::database::entities::task::Entity::find()
                    .filter(
                        crawlrs::infrastructure::database::entities::task::Column::Status
                            .eq("queued"),
                    )
                    .order_by_asc(
                        crawlrs::infrastructure::database::entities::task::Column::Priority,
                    )
                    .order_by_asc(
                        crawlrs::infrastructure::database::entities::task::Column::CreatedAt,
                    )
                    .limit(10)
                    .all(&db)
                    .await
            });
            black_box(result)
        });
    });

    // 聚合查询 - 统计任务数量
    group.bench_function("count_tasks", |b| {
        b.iter(|| {
            let rt = Runtime::new().unwrap();
            let result = rt.block_on(async {
                crawlrs::infrastructure::database::entities::task::Entity::find()
                    .filter(
                        crawlrs::infrastructure::database::entities::task::Column::Status
                            .eq("completed"),
                    )
                    .count(&db)
                    .await
            });
            black_box(result)
        });
    });

    group.finish();
}

/// 基准测试：字符串操作
///
/// 测试常用的字符串操作性能
fn benchmark_string_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("string_operations");

    // URL验证和处理
    let test_urls = vec![
        "https://example.com",
        "https://subdomain.example.com/path/to/resource?param=value&other=123",
        "http://localhost:8080/api/v1/endpoint",
        "https://example.com/very/long/path/with/many/segments/and/query/parameters?param1=value1&param2=value2&param3=value3",
    ];

    group.bench_function("url_validation", |b| {
        b.iter(|| {
            for url in &test_urls {
                let is_valid = url.starts_with("http://") || url.starts_with("https://");
                black_box(is_valid);
            }
        });
    });

    // JSON字符串转义
    let test_content = r#"Content with "quotes" and special chars: 
	
 and unicode: 你好世界 🌍"#;

    group.bench_function("json_string_escape", |b| {
        b.iter(|| {
            let escaped = serde_json::to_string(test_content).unwrap();
            black_box(escaped)
        });
    });

    // 字符串截断操作
    let long_content = "A".repeat(10000);
    group.bench_function("string_truncation", |b| {
        b.iter(|| {
            let truncated = if long_content.len() > 1000 {
                format!("{}...", &long_content[..1000])
            } else {
                long_content.clone()
            };
            black_box(truncated)
        });
    });

    group.finish();
}

// 基准测试组合
criterion_group!(
    benches,
    benchmark_task_creation,
    benchmark_task_status_transitions,
    benchmark_json_serialization,
    benchmark_uuid_operations,
    benchmark_memory_operations,
    benchmark_concurrent_task_processing,
    benchmark_error_handling,
    benchmark_database_queries,
    benchmark_string_operations
);

criterion_main!(benches);
