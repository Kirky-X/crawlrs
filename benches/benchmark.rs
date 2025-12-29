// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

//! 性能基准测试套件
//!
//! 该模块包含对 crawlrs 系统核心组件的性能基准测试，用于评估系统在不同场景下的性能表现。

use crawlrs::domain::models::task::{Task, TaskType};
use crawlrs::domain::services::crawl_service::LinkDiscoverer;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use migration::{Migrator, MigratorTrait};
use sea_orm::{
    ColumnTrait, Database, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use std::hint::black_box;
use tokio::runtime::Runtime;
use uuid::Uuid;
use validator::Validate;

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

/// 基准测试：搜索操作性能
///
/// 测试搜索服务的核心操作性能，包括查询处理、结果过滤和响应构建
fn benchmark_search_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("search_operations");

    // 测试搜索请求验证
    group.bench_function("search_request_validation", |b| {
        b.iter(|| {
            let dto = crawlrs::application::dto::search_request::SearchRequestDto {
                query: "test query".to_string(),
                limit: Some(10),
                lang: Some("en".to_string()),
                country: Some("us".to_string()),
                engine: None,
                sources: None,
                crawl_results: Some(false),
                crawl_config: None,
                sync_wait_ms: Some(5000),
            };
            let result = dto.validate();
            black_box(result)
        });
    });

    // 测试搜索结果DTO构建
    group.bench_function("search_result_dto_construction", |b| {
        b.iter(|| {
            let results: Vec<crawlrs::application::dto::search_request::SearchResultDto> = (0..100)
                .map(
                    |i| crawlrs::application::dto::search_request::SearchResultDto {
                        title: format!("Search Result {}", i),
                        url: format!("https://example{}.com", i),
                        description: Some(format!("This is result number {} from search", i)),
                        engine: Some("google".to_string()),
                    },
                )
                .collect();
            black_box(results)
        });
    });

    // 测试搜索响应构建
    group.bench_function("search_response_construction", |b| {
        b.iter(|| {
            let results: Vec<crawlrs::application::dto::search_request::SearchResultDto> = (0..50)
                .map(
                    |i| crawlrs::application::dto::search_request::SearchResultDto {
                        title: format!("Result {}", i),
                        url: format!("https://example{}.com/result/{}", i, i),
                        description: Some(format!("Description for result {}", i)),
                        engine: Some("bing".to_string()),
                    },
                )
                .collect();

            let response = crawlrs::application::dto::search_request::SearchResponseDto {
                query: "benchmark test query".to_string(),
                results,
                crawl_id: Some(Uuid::new_v4()),
                credits_used: 1,
            };
            black_box(response)
        });
    });

    // 测试搜索结果过滤（按引擎名称）
    group.bench_function("search_result_filtering", |b| {
        let all_results: Vec<crawlrs::application::dto::search_request::SearchResultDto> = (0..100)
            .map(
                |i| crawlrs::application::dto::search_request::SearchResultDto {
                    title: format!("Result {}", i),
                    url: format!("https://example{}.com", i),
                    description: Some(format!("Description {}", i)),
                    engine: Some(if i % 3 == 0 {
                        "google".to_string()
                    } else if i % 3 == 1 {
                        "bing".to_string()
                    } else {
                        "duckduckgo".to_string()
                    }),
                },
            )
            .collect();

        b.iter(|| {
            let filtered: Vec<_> = all_results
                .iter()
                .filter(|r| r.engine.as_ref().is_some_and(|e| e == "google"))
                .take(10)
                .cloned()
                .collect();
            black_box(filtered)
        });
    });

    // 测试搜索结果序列化
    group.bench_function("search_result_serialization", |b| {
        let results: Vec<crawlrs::application::dto::search_request::SearchResultDto> = (0..50)
            .map(
                |i| crawlrs::application::dto::search_request::SearchResultDto {
                    title: format!("Search Result with special chars: 你好世界 {}", i),
                    url: format!("https://example{}.com/path?param=value&special=你好", i),
                    description: Some(format!("Description with unicode: 🌍🎉 {}", i)),
                    engine: Some("google".to_string()),
                },
            )
            .collect();

        b.iter(|| {
            let json = serde_json::to_string(&results).unwrap();
            black_box(json)
        });
    });

    group.finish();
}

/// 基准测试：抓取操作性能
///
/// 测试爬虫服务的核心操作性能，包括链接提取、URL解析和HTML处理
fn benchmark_crawl_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("crawl_operations");

    // 测试链接提取
    let sample_html = r#"
        <html>
            <head><title>Test Page</title></head>
            <body>
                <a href="https://example.com/page1">Page 1</a>
                <a href="https://example.com/page2">Page 2</a>
                <a href="https://other.com/link">External Link</a>
                <a href="/relative/path">Relative Link</a>
                <div class="content">
                    <p>Some content here</p>
                    <a href="https://example.com/deep/nested/page">Nested Page</a>
                </div>
            </body>
        </html>
    "#;

    group.bench_function("link_extraction", |b| {
        b.iter(|| {
            let links = LinkDiscoverer::extract_links(sample_html, "https://example.com");
            black_box(links)
        });
    });

    // 测试URL解析
    let test_urls = vec![
        "https://example.com",
        "https://sub.example.com/path/to/resource",
        "http://localhost:8080/api/v1",
        "https://example.com/path?query=value&other=123",
        "https://example.com:443/path/to/resource",
    ];

    group.bench_function("url_parsing", |b| {
        b.iter(|| {
            for url in &test_urls {
                let parsed = url::Url::parse(url);
                let _ = black_box(parsed);
            }
        });
    });

    // 测试URL规范化
    group.bench_function("url_normalization", |b| {
        b.iter(|| {
            let urls = vec![
                "https://example.com/path/../other",
                "https://example.com/path/./resource",
                "https://EXAMPLE.com/path",
                "https://example.com/path?param=value&param=value",
            ];
            for url in urls {
                let normalized = url::Url::parse(url).ok().map(Into::<String>::into);
                black_box(normalized);
            }
        });
    });

    // 测试Robots.txt URL检查（模拟）
    group.bench_function("robots_url_check", |b| {
        b.iter(|| {
            let urls = vec![
                "https://example.com/allowed/path",
                "https://example.com/disallowed/admin",
                "https://example.com/allowed/api",
            ];
            for url in urls {
                let is_allowed = url.starts_with("https://example.com/disallowed")
                    || url.starts_with("https://example.com/admin");
                black_box(is_allowed);
            }
        });
    });

    // 测试HTML内容解析
    let large_html = format!(
        r#"<html><head><title>Test</title></head><body>{}</body></html>"#,
        (0..1000)
            .map(|i| format!(
                r#"<div class="item" data-id="{}">Content for item {}</div>"#,
                i, i
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );

    group.bench_function("html_parsing", |b| {
        b.iter(|| {
            let document = scraper::Html::parse_document(&large_html);
            black_box(document)
        });
    });

    // 测试CSS选择器解析
    group.bench_function("css_selector_parsing", |b| {
        let selectors = vec![
            "div.content > p",
            ".container .item[data-id]",
            "a[href^=\"https\"]",
            "div:not(.excluded)",
            ".post .title, .post .content",
        ];

        b.iter(|| {
            for selector in &selectors {
                let parsed = scraper::Selector::parse(selector);
                let _ = black_box(parsed);
            }
        });
    });

    // 测试链接过滤
    let all_links = [
        "https://example.com/page1",
        "https://example.com/page2",
        "https://other.com/external",
        "https://example.com/allowed/api",
        "https://example.com/admin/disallowed",
        "https://example.com/path/file.pdf",
        "https://example.com/path/image.jpg",
    ];

    let include_patterns = ["/page".to_string(), "/api".to_string()];
    let exclude_patterns = ["/admin".to_string(), "\\.(pdf|jpg)$".to_string()];

    group.bench_function("link_filtering", |b| {
        b.iter(|| {
            let filtered: Vec<_> = all_links
                .iter()
                .filter(|link| {
                    let passes_include = include_patterns.is_empty()
                        || include_patterns.iter().any(|p| link.contains(p));
                    let passes_exclude = exclude_patterns
                        .iter()
                        .all(|p| !link.contains(p) && !p.is_empty());
                    passes_include && passes_exclude
                })
                .collect();
            black_box(filtered)
        });
    });

    // 测试任务payload解析
    group.bench_function("crawl_payload_parsing", |b| {
        b.iter(|| {
            let payload = serde_json::json!({
                "depth": 2,
                "max_depth": 5,
                "include_patterns": ["/products", "/categories"],
                "exclude_patterns": ["/admin", "/private"],
                "strategy": "bfs",
                "domain_blacklist": ["malicious.com", "spam.com"]
            });

            let depth = payload.get("depth").and_then(|v| v.as_u64()).unwrap_or(0);
            let max_depth = payload
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(3);
            let strategy = payload
                .get("strategy")
                .and_then(|v| v.as_str())
                .unwrap_or("bfs")
                .to_string();

            black_box((depth, max_depth, strategy))
        });
    });

    group.finish();
}

/// 基准测试：提取操作性能
///
/// 测试数据提取服务的核心操作性能，包括HTML解析、CSS选择器提取和LLM提取
fn benchmark_extract_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("extract_operations");

    // 测试HTML解析
    let sample_html = r#"
        <html>
            <head><title>Product Page</title></head>
            <body>
                <div class="product" data-id="12345">
                    <h1 class="title">Amazing Product</h1>
                    <p class="description">This is a great product with many features.</p>
                    <span class="price">$99.99</span>
                    <div class="reviews">
                        <div class="review"><span class="rating">5 stars</span><p>Great!</p></div>
                        <div class="review"><span class="rating">4 stars</span><p>Good product</p></div>
                    </div>
                </div>
            </body>
        </html>
    "#;

    group.bench_function("html_parsing_for_extraction", |b| {
        b.iter(|| {
            let document = scraper::Html::parse_document(sample_html);
            black_box(document)
        });
    });

    // 测试CSS选择器提取（单个元素）
    group.bench_function("css_selector_single_extraction", |b| {
        let sample_doc = sample_html;

        b.iter(|| {
            let selector = scraper::Selector::parse(".product .title").unwrap();
            let document = scraper::Html::parse_document(sample_doc);
            let element = document.select(&selector).next();
            let text = element.map(|e| e.text().collect::<Vec<_>>().join(" "));
            black_box(text)
        });
    });

    // 测试CSS选择器提取（多个元素）
    group.bench_function("css_selector_multiple_extraction", |b| {
        let sample_doc = sample_html;

        b.iter(|| {
            let selector = scraper::Selector::parse(".review").unwrap();
            let document = scraper::Html::parse_document(sample_doc);
            let elements: Vec<_> = document.select(&selector).map(|e| e.inner_html()).collect();
            black_box(elements)
        });
    });

    // 测试属性提取
    group.bench_function("attribute_extraction", |b| {
        let sample_doc = sample_html;

        b.iter(|| {
            let selector = scraper::Selector::parse(".product").unwrap();
            let document = scraper::Html::parse_document(sample_doc);
            let attr = document
                .select(&selector)
                .next()
                .and_then(|e| e.value().attr("data-id"))
                .map(|s| s.to_string());
            black_box(attr)
        });
    });

    // 测试提取规则构建
    group.bench_function("extraction_rule_construction", |b| {
        b.iter(|| {
            let rules: std::collections::HashMap<
                String,
                crawlrs::domain::services::extraction_service::ExtractionRule,
            > = vec![
                (
                    "title".to_string(),
                    crawlrs::domain::services::extraction_service::ExtractionRule {
                        selector: Some(".product .title".to_string()),
                        attr: None,
                        is_array: false,
                        use_llm: None,
                        llm_prompt: None,
                    },
                ),
                (
                    "price".to_string(),
                    crawlrs::domain::services::extraction_service::ExtractionRule {
                        selector: Some(".product .price".to_string()),
                        attr: None,
                        is_array: false,
                        use_llm: None,
                        llm_prompt: None,
                    },
                ),
                (
                    "reviews".to_string(),
                    crawlrs::domain::services::extraction_service::ExtractionRule {
                        selector: Some(".review".to_string()),
                        attr: None,
                        is_array: true,
                        use_llm: None,
                        llm_prompt: None,
                    },
                ),
            ]
            .into_iter()
            .collect();
            black_box(rules)
        });
    });

    // 测试提取请求DTO构建
    group.bench_function("extract_request_dto_construction", |b| {
        b.iter(|| {
            let dto = crawlrs::application::dto::extract_request::ExtractRequestDto {
                urls: vec![
                    "https://example.com/page1".to_string(),
                    "https://example.com/page2".to_string(),
                ],
                prompt: Some("Extract product information".to_string()),
                schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "price": {"type": "string"},
                        "description": {"type": "string"}
                    }
                })),
                model: Some("gpt-4".to_string()),
                rules: None,
                sync_wait_ms: Some(5000),
            };
            black_box(dto)
        });
    });

    // 测试大型HTML内容处理
    let large_html = format!(
        r#"<!DOCTYPE html><html><head><title>Large Page</title></head><body>{}</body></html>"#,
        (0..500)
            .map(|i| format!(
                r#"<div class="item" data-index="{}"><h2>Item {}</h2><p>Description for item {}</p><span class="meta">Meta {}</span></div>"#,
                i, i, i, i
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );

    group.bench_function("large_html_processing", |b| {
        let large_doc = large_html.as_str();

        b.iter(|| {
            let selector = scraper::Selector::parse(".item").unwrap();
            let document = scraper::Html::parse_document(large_doc);
            let items: Vec<_> = document.select(&selector).map(|e| e.inner_html()).collect();
            black_box(items)
        });
    });

    // 测试提取结果JSON构建
    group.bench_function("extraction_result_construction", |b| {
        b.iter(|| {
            let results: Vec<crawlrs::application::dto::extract_request::ExtractResultDto> = (0
                ..50)
                .map(
                    |i| crawlrs::application::dto::extract_request::ExtractResultDto {
                        url: format!("https://example{}.com", i),
                        data: serde_json::json!({
                            "title": format!("Title {}", i),
                            "content": format!("Content for item {}", i),
                            "metadata": {
                                "index": i,
                                "source": "extraction"
                            }
                        }),
                        error: None,
                    },
                )
                .collect();
            black_box(results)
        });
    });

    // 测试Schema验证
    group.bench_function("schema_validation", |b| {
        b.iter(|| {
            let _schema = serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "age": {"type": "integer", "minimum": 0},
                    "email": {"type": "string", "format": "email"}
                },
                "required": ["name", "email"]
            });

            let data = serde_json::json!({
                "name": "Test User",
                "age": 30,
                "email": "test@example.com"
            });

            let valid = data.is_object();
            black_box(valid)
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
    benchmark_string_operations,
    benchmark_search_operations,
    benchmark_crawl_operations,
    benchmark_extract_operations
);

criterion_main!(benches);
