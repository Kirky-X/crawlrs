// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in project root for full license information.

//! 性能基准测试套件
//!
//! 该模块包含对 crawlrs 系统核心组件的性能基准测试

use crawlrs::domain::models::task_domain::TaskType;
use crawlrs::domain::models::task_model::Task;
use crawlrs::engines::engine_client::{
    HttpMethod, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use crawlrs::engines::router::{EngineRouter, EngineRouterTrait, LoadBalancingStrategy};
use crawlrs::infrastructure::oxcache::RegexCacheType;
use crawlrs::utils::regex_cache::RegexCache;
use async_trait::async_trait;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use oxcache::Cache;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;
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
                let _ = black_box(parsed);
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

// =============================================================================
// T069: URL 验证基准测试
// =============================================================================

/// 生成测试 URL 集合
fn generate_test_urls(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| format!("https://example{}.com/path/to/resource?q=search&id={}", i % 100, i))
        .collect()
}

/// T069: URL 格式验证性能（domain 层纯函数）
fn benchmark_url_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_validation");

    for size in [100, 1000, 10000] {
        let urls = generate_test_urls(size);
        group.bench_with_input(
            BenchmarkId::new("validate_url_format", size),
            &size,
            |b, _| {
                b.iter(|| {
                    for url in &urls {
                        let result = crawlrs::domain::models::validations::validate_url(url);
                        let _ = black_box(result);
                    }
                });
            },
        );
    }

    group.finish();
}

/// T069: SSRF 内部 URL 检测性能（同步快速检查）
fn benchmark_ssrf_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("ssrf_detection");

    // 混合 URL：包含外部 URL、localhost、私有 IP
    let mixed_urls = [
        "https://example.com/page",
        "http://localhost/admin",
        "http://192.168.1.1/internal",
        "http://10.0.0.1/secret",
        "https://google.com/search",
        "http://127.0.0.1/api",
        "http://172.16.0.1/data",
        "https://api.example.com/v1/users",
    ];

    for size in [100, 1000, 10000] {
        let urls: Vec<String> = (0..size)
            .map(|i| {
                if i % 4 == 0 {
                    mixed_urls[i % mixed_urls.len()].to_string()
                } else {
                    format!("https://host{}.example.com/path", i)
                }
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::new("is_internal_url", size),
            &size,
            |b, _| {
                b.iter(|| {
                    for url in &urls {
                        let is_internal =
                            crawlrs::engines::validators::is_internal_url(url);
                        let _ = black_box(is_internal);
                    }
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// T070: 缓存命中/未命中基准测试
// =============================================================================

/// T070: RegexCache get_or_insert 性能（命中 vs 未命中）
fn benchmark_regex_cache(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("regex_cache");

    // 创建 RegexCache
    let cache: Arc<RegexCacheType> = Arc::new(
        rt.block_on(async {
            Cache::builder()
                .capacity(1000)
                .ttl(Duration::from_secs(300))
                .build()
                .await
                .unwrap()
        }),
    );
    let regex_cache = RegexCache::new(cache);

    // 预热缓存
    let patterns: Vec<String> = (0..50).map(|i| format!(r"pattern_{}\d+", i)).collect();
    for pattern in &patterns {
        let _ = regex_cache.get_or_insert(pattern);
    }

    // 命中测试
    group.bench_function("cache_hit", |b| {
        b.iter(|| {
            for pattern in &patterns {
                let result = regex_cache.get_or_insert(pattern);
                let _ = black_box(result);
            }
        });
    });

    // 未命中测试（每次用新 pattern）
    group.bench_function("cache_miss", |b| {
        let mut counter = 0u64;
        b.iter(|| {
            counter += 1;
            let pattern = format!(r"new_pattern_{}_\d+", counter);
            let result = regex_cache.get_or_insert(&pattern);
            let _ = black_box(result);
        });
    });

    group.finish();
}

// =============================================================================
// T071: 引擎路由决策基准测试
// =============================================================================

/// 基准测试用 Mock 引擎
struct BenchMockEngine {
    engine_name: String,
    score: u8,
}

#[async_trait]
impl ScraperEngine for BenchMockEngine {
    async fn scrape(
        &self,
        _request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, crawlrs::engines::engine_client::EngineError> {
        Ok(InternalScrapeResponse {
            status_code: 200,
            content: "<html><body>Mock</body></html>".to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: std::collections::HashMap::new(),
            response_time_ms: 10,
        })
    }

    fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
        self.score
    }

    fn name(&self) -> &'static str {
        // SAFETY: engine_name 生命周期与 engine 相同
        Box::leak(self.engine_name.clone().into_boxed_str())
    }
}

/// 创建 N 个 mock 引擎
fn create_mock_engines(count: usize) -> Vec<Arc<dyn ScraperEngine>> {
    (0..count)
        .map(|i| {
            Arc::new(BenchMockEngine {
                engine_name: format!("bench_engine_{}", i),
                score: (50 + (i % 50)) as u8,
            }) as Arc<dyn ScraperEngine>
        })
        .collect()
}

/// 创建基准测试请求
fn make_bench_request() -> InternalScrapeRequest {
    InternalScrapeRequest {
        url: "https://example.com".to_string(),
        method: HttpMethod::Get,
        headers: std::collections::HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: false,
    }
}

/// T071: 引擎路由选择性能（select_optimal_engines + route）
fn benchmark_engine_routing(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("engine_routing");

    for engine_count in [10, 50, 100] {
        let engines = create_mock_engines(engine_count);
        let router = EngineRouter::new(engines);
        let request = make_bench_request();

        group.bench_with_input(
            BenchmarkId::new("route_selection", engine_count),
            &engine_count,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        let _result = router.route(&request).await;
                    });
                });
            },
        );
    }

    // 不同策略对比
    for strategy in [
        LoadBalancingStrategy::RoundRobin,
        LoadBalancingStrategy::SmartHybrid,
        LoadBalancingStrategy::LeastConnections,
    ] {
        let engines = create_mock_engines(50);
        let mut router = EngineRouter::new(engines);
        router.set_strategy(strategy);
        let request = make_bench_request();

        group.bench_function(
            BenchmarkId::new("strategy_comparison", format!("{:?}", strategy)),
            |b| {
                b.iter(|| {
                    rt.block_on(async {
                        let _result = router.route(&request).await;
                    });
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_task_creation,
    benchmark_task_status_transitions,
    benchmark_json_serialization,
    benchmark_url_parsing,
    benchmark_uuid_generation,
    // T069-T071 新增
    benchmark_url_validation,
    benchmark_ssrf_detection,
    benchmark_regex_cache,
    benchmark_engine_routing,
);
criterion_main!(benches);
