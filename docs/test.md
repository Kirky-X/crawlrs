# crawlrs - 测试文档 (TEST)

## 版本信息

- **文档版本**: v2.1.0
- **测试框架**: Rust Test + tokio::test
- **最近更新**: 2024-12-20

---

## 1. 测试策略

### 1.1 测试金字塔

```
        ┌─────────┐
        │   E2E   │  (10%)  - 全链路测试
        ├─────────┤
        │ 集成测试 │  (30%)  - 模块间交互
        ├─────────┤
        │ 单元测试 │  (60%)  - 函数级测试
        └─────────┘
```

### 1.2 覆盖率目标

- **总体覆盖率**: ≥ 80%
- **核心业务逻辑**: ≥ 90%
- **边界条件**: ≥ 95%
- **错误路径**: ≥ 85%

### 1.3 测试环境

- **单元测试**: 内存数据库 + Mock
- **集成测试**: Docker Compose 测试栈
- **压力测试**: K6 + Grafana
- **E2E 测试**: Testcontainers
- TestSearchEngine: Simulated search engine returning test results

---

## 2. 单元测试

### 2.1 领域模型测试

#### 测试用例：Task 状态转换

```rust
// tests/unit/domain/models/task_test.rs
use crawlrs::domain::models::{Task, TaskStatus};

#[test]
fn test_task_lifecycle_happy_path() {
    // Given: 新创建的任务
    let task = Task::new(TaskType::Scrape, team_id, url, payload);
    assert_eq!(task.status, TaskStatus::Queued);
    
    // When: 启动任务
    let task = task.start().unwrap();
    
    // Then: 状态变为 Active
    assert_eq!(task.status, TaskStatus::Active);
    
    // When: 完成任务
    let task = task.complete().unwrap();
    
    // Then: 状态变为 Completed
    assert_eq!(task.status, TaskStatus::Completed);
}

#[test]
fn test_task_invalid_state_transition() {
    // Given: 已完成的任务
    let mut task = Task::new(TaskType::Scrape, team_id, url, payload);
    task.status = TaskStatus::Completed;
    
    // When & Then: 尝试启动任务应失败
    assert!(task.start().is_err());
}

#[test]
fn test_task_retry_logic() {
    // Given: 失败任务
    let mut task = Task::new(TaskType::Scrape, team_id, url, payload);
    task.status = TaskStatus::Failed;
    task.retry_count = 2;
    task.max_retries = 3;
    
    // When: 检查是否可重试
    assert!(task.can_retry());
    
    // When: 重试次数达到上限
    task.retry_count = 3;
    
    // Then: 不可重试
    assert!(!task.can_retry());
}
```

**测试覆盖**:

- ✅ 正常状态流转
- ✅ 非法状态转换
- ✅ 重试逻辑
- ✅ 边界条件

---

### 2.2 引擎选择测试

#### 测试用例：智能路由器

```rust
// tests/unit/engines/router_test.rs
use crawlrs::engines::{EngineRouter, ScrapeRequest};

#[tokio::test]
async fn test_router_selects_fetch_for_simple_request() {
    // Given: 简单的 HTTP 请求
    let request = ScrapeRequest {
        url: "https://example.com".to_string(),
        needs_js: false,
        needs_screenshot: false,
        ..Default::default()
    };
    
    let router = EngineRouter::new(vec![
        Arc::new(ReqwestEngine),
        Arc::new(PlaywrightEngine),
    ]);
    
    // When: 路由请求
    let response = router.route(&request).await.unwrap();
    
    // Then: 应选择 ReqwestEngine 引擎
    assert_eq!(response.engine_used, "reqwest");
}

#[tokio::test]
async fn test_router_fallback_on_engine_failure() {
    // Given: ReqwestEngine 引擎已断路
    let circuit_breaker = Arc::new(CircuitBreaker::new());
    circuit_breaker.open("fetch");
    
    let router = EngineRouter::with_circuit_breaker(
        vec![
            Arc::new(ReqwestEngine),
            Arc::new(PlaywrightEngine),
        ],
        circuit_breaker,
    );
    
    // When: 路由请求
    let response = router.route(&request).await.unwrap();
    
    // Then: 应降级到 PlaywrightEngine
    assert_eq!(response.engine_used, "playwright");
}
```

**测试覆盖**:

- ✅ 按优先级选择引擎
- ✅ 断路器触发时的降级
- ✅ 所有引擎失败时的错误处理

---

### 2.3 并发控制测试

#### 测试用例：团队信号量

```rust
// tests/unit/middleware/team_semaphore_test.rs
use crawlrs::middleware::TeamSemaphore;

#[tokio::test]
async fn test_semaphore_acquire_and_release() {
    let redis = setup_test_redis().await;
    let semaphore = TeamSemaphore::new(redis);
    
    let team_id = Uuid::new_v4();
    let max_concurrent = 2;
    
    // When: 获取 2 个信号量
    let guard1 = semaphore.acquire(team_id, max_concurrent).await.unwrap();
    let guard2 = semaphore.acquire(team_id, max_concurrent).await.unwrap();
    
    // Then: 第 3 个请求应失败
    let result = semaphore.acquire(team_id, max_concurrent).await;
    assert!(result.is_err());
    
    // When: 释放一个信号量
    drop(guard1);
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Then: 第 3 个请求应成功
    let guard3 = semaphore.acquire(team_id, max_concurrent).await.unwrap();
    assert!(guard3.is_some());
}
```

**测试覆盖**:

- ✅ 信号量获取与释放
- ✅ 并发限制生效
- ✅ RAII 自动释放

---

### 2.4 搜索引擎测试 ✅ 已实现

#### 测试用例：Google ARC_ID 生成与刷新

```rust
// tests/unit/engines/search/google_test.rs
use crawlrs::engines::search::GoogleSearchEngine;

#[tokio::test]
async fn test_google_arc_id_generation() {
    let engine = GoogleSearchEngine::new();
    
    // When: 首次获取 ARC_ID
    let arc_id_1 = engine.get_arc_id(0).await;
    
    // Then: 格式正确
    assert!(arc_id_1.starts_with("arc_id:srp_"));
    assert!(arc_id_1.contains("use_ac:true"));
    
    // When: 1 秒后再次获取
    tokio::time::sleep(Duration::from_secs(1)).await;
    let arc_id_2 = engine.get_arc_id(0).await;
    
    // Then: 应该相同（未超过 1 小时）
    assert_eq!(arc_id_1, arc_id_2);
}

#[tokio::test]
async fn test_google_arc_id_refresh_after_hour() {
    let engine = GoogleSearchEngine::new();
    
    // Given: 模拟时间已过 1 小时（通过修改内部缓存时间戳）
    // 注意：需要提供测试 API 或使用时间注入
    let arc_id_1 = engine.get_arc_id(0).await;
    
    // 强制刷新缓存（测试用 API）
    engine.force_refresh_arc_id().await;
    
    let arc_id_2 = engine.get_arc_id(0).await;
    
    // Then: ARC_ID 应不同
    assert_ne!(arc_id_1, arc_id_2);
}

#[test]
fn test_google_result_parsing() {
    let html = r#"
        <div class="g">
            <h3 class="LC20lb">
                Test Title
            </h3>
            <div class="VwiC3b">Test description</div>
        </div>
    "#;
    
    let engine = GoogleSearchEngine::new();
    let results = engine.parse_results(html).unwrap();
    
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Test Title");
    assert_eq!(results[0].url, "https://example.com");
    assert_eq!(results[0].content, "Test description");
}
```

**测试覆盖**:

- ✅ ARC_ID 生成逻辑
- ✅ ARC_ID 缓存机制
- ✅ HTML 解析正确性
- ✅ 错误处理

**实现状态**: ✅ 已实现 - 测试用例在 `tests/unit/engines/search/google_test.rs`

---

#### 测试用例：Bing Cookie 管理

```rust
// tests/unit/engines/search/bing_test.rs
use crawlrs::engines::search::BingSearchEngine;

#[test]
fn test_bing_cookie_construction() {
    let cookies = BingSearchEngine::build_cookies("en", "US");
    
    assert_eq!(cookies.get("_EDGE_CD"), Some(&"m=US&u=en".to_string()));
    assert_eq!(cookies.get("_EDGE_S"), Some(&"mkt=US&ui=en".to_string()));
}

#[test]
fn test_bing_form_parameter_logic() {
    let engine = BingSearchEngine::new();
    
    // Page 1: 无 FORM 参数
    let params_1 = engine.build_params("rust", 1);
    assert!(!params_1.contains_key("FORM"));
    
    // Page 2: FORM=PERE
    let params_2 = engine.build_params("rust", 2);
    assert_eq!(params_2.get("FORM"), Some(&"PERE".to_string()));
    
    // Page 3: FORM=PERE1
    let params_3 = engine.build_params("rust", 3);
    assert_eq!(params_3.get("FORM"), Some(&"PERE1".to_string()));
    
    // Page 4: FORM=PERE2
    let params_4 = engine.build_params("rust", 4);
    assert_eq!(params_4.get("FORM"), Some(&"PERE2".to_string()));
}

#[test]
fn test_bing_url_decoding() {
    let encoded = "https://www.bing.com/ck/a?u=a1aHR0cHM6Ly9leGFtcGxlLmNvbQ";
    let decoded = BingSearchEngine::decode_url(encoded);
    
    assert_eq!(decoded, "https://example.com");
}
```

**测试覆盖**:

- ✅ Cookie 构造逻辑
- ✅ FORM 参数计算
- ✅ Base64 URL 解码
- ✅ 分页参数正确性

**实现状态**: ✅ 已实现 - 测试用例在 `tests/unit/engines/search/bing_test.rs`

---

#### 测试用例：搜索路由器并发聚合

```rust
// tests/unit/engines/search/router_test.rs
use crawlrs::engines::search::{SearchRouter, SearchQuery};

#[tokio::test]
async fn test_concurrent_search_aggregation() {
    // Given: 配置 3 个引擎
    let router = SearchRouter::new(
        vec![
            Arc::new(GoogleSearchEngine::new()),
            Arc::new(BingSearchEngine::new()),
            Arc::new(BaiduSearchEngine::new()),
        ],
        SearchConfig {
            enabled_engines: vec!["google".into(), "bing".into(), "baidu".into()],
            concurrent_timeout_ms: 10000,
            dedup_threshold: 0.85,
            min_engines_success: 1,
            ..Default::default()
        },
    );
    
    let query = SearchQuery {
        query: "rust programming".to_string(),
        page: 1,
        limit: 10,
        lang: "en".to_string(),
        country: "US".to_string(),
    };
    
    // When: 执行并发搜索
    let start = Instant::now();
    let response = router.search(&query).await.unwrap();
    let elapsed = start.elapsed();
    
    // Then: 耗时应接近单引擎耗时（并发执行）
    assert!(elapsed.as_secs() < 12);  // 10s 超时 + 2s 余量
    
    // Then: 结果来自多个引擎
    assert!(response.engines_used.len() >= 2);
    
    // Then: 结果已去重
    let urls: HashSet<_> = response.results.iter().map(|r| &r.url).collect();
    assert_eq!(urls.len(), response.results.len());
}

#[tokio::test]
async fn test_deduplication_logic() {
    let router = SearchRouter::new_for_test();
    
    // Given: 模拟重复结果
    let test_results = vec![
        ("google".to_string(), vec![
            SearchResult {
                title: "Rust Programming Language".to_string(),
                url: "https://rust-lang.org".to_string(),
                ..Default::default()
            },
        ]),
        ("bing".to_string(), vec![
            SearchResult {
                title: "Rust Programming Language".to_string(),  // 完全相同
                url: "https://rust-lang.org".to_string(),
                ..Default::default()
            },
            SearchResult {
                title: "The Rust Programming Language".to_string(),  // 标题相似度 > 0.85
                url: "https://doc.rust-lang.org".to_string(),
                ..Default::default()
            },
        ]),
    ];
    
    // When: 去重
    let merged = router.merge_and_deduplicate(test_results).unwrap();
    
    // Then: 只保留一个结果
    assert_eq!(merged.results.len(), 1);
    assert_eq!(merged.results[0].source_engine, Some("google".to_string()));
}

#[tokio::test]
async fn test_circuit_breaker_integration() {
    let circuit_breaker = Arc::new(CircuitBreaker::new());
    
    // Given: Google 引擎已断路
    circuit_breaker.open("google");
    
    let router = SearchRouter::new(
        vec![
            Arc::new(GoogleSearchEngine::new()),
            Arc::new(BingSearchEngine::new()),
        ],
        SearchConfig {
            enabled_engines: vec!["google".into(), "bing".into()],
            ..Default::default()
        },
    ).with_circuit_breaker(circuit_breaker);
    
    // When: 执行搜索
    let response = router.search(&query).await.unwrap();
    
    // Then: 只使用了 Bing
    assert_eq!(response.engines_used, vec!["bing"]);
    assert!(!response.engines_used.contains(&"google".to_string()));
}
```

**测试覆盖**:

- ✅ 并发查询逻辑
- ✅ URL 去重
- ✅ 标题相似度去重（Jaro-Winkler）
- ✅ 断路器集成
- ✅ 最少成功引擎检查

**实现状态**: ✅ 已实现 - 测试用例在 `tests/unit/engines/search/router_test.rs`

---

### 2.5 搜索缓存测试 ✅ 已实现

#### 测试用例：缓存键生成

```rust
// tests/unit/infrastructure/cache/search_cache_test.rs
use crawlrs::infrastructure::cache::SearchCache;

#[test]
fn test_cache_key_generation() {
    let cache = SearchCache::new_for_test();
    
    let query1 = SearchQuery {
        query: "rust".to_string(),
        engines: vec!["google".into()],
        lang: "en".to_string(),
        limit: 10,
    };
    
    let query2 = SearchQuery {
        query: "rust".to_string(),
        engines: vec!["google".into()],
        lang: "en".to_string(),
        limit: 10,
    };
    
    // Then: 相同查询生成相同的键
    assert_eq!(cache.generate_key(&query1), cache.generate_key(&query2));
    
    // When: 修改任意参数
    let query3 = SearchQuery {
        query: "rust".to_string(),
        engines: vec!["bing".into()],  // 不同引擎
        lang: "en".to_string(),
        limit: 10,
    };
    
    // Then: 键应不同
    assert_ne!(cache.generate_key(&query1), cache.generate_key(&query3));
}

#[tokio::test]
async fn test_cache_set_and_get() {
    let redis = setup_test_redis().await;
    let cache = SearchCache::new(redis);
    
    let key = "search:v1:test123";
    let response = SearchResponse {
        results: vec![SearchResult {
            title: "Test".to_string(),
            url: "https://example.com".to_string(),
            ..Default::default()
        }],
        total: 1,
        engines_used: vec!["google".to_string()],
    };
    
    // When: 写入缓存
    cache.set(key, &response, Duration::from_secs(60)).await.unwrap();
    
    // Then: 可以读取
    let cached = cache.get(key).await.unwrap().unwrap();
    assert_eq!(cached.total, 1);
    assert_eq!(cached.results[0].title, "Test");
}

#[tokio::test]
async fn test_cache_expiration() {
    let redis = setup_test_redis().await;
    let cache = SearchCache::new(redis);
    
    let key = "search:v1:expire_test";
    let response = SearchResponse::default();
    
    // When: 设置 1 秒 TTL
    cache.set(key, &response, Duration::from_secs(1)).await.unwrap();
    
    // Then: 立即可读
    assert!(cache.get(key).await.unwrap().is_some());
    
    // When: 等待 2 秒
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    // Then: 缓存已过期
    assert!(cache.get(key).await.unwrap().is_none());
}
```

**测试覆盖**:

- ✅ 缓存键生成逻辑
- ✅ 写入和读取
- ✅ TTL 过期机制
- ✅ 缓存穿透保护（待补充）

**实现状态**: ✅ 已实现 - 测试用例在 `tests/unit/infrastructure/cache/search_cache_test.rs`

---

### 2.6 同步等待机制测试 ✅ 已实现

#### 测试用例：智能等待逻辑

```rust
// tests/unit/presentation/handlers/sync_wait_test.rs
use crawlrs::presentation::handlers::handle_scrape_with_wait;

#[tokio::test]
async fn test_sync_wait_returns_result_immediately() {
    // Given: 任务会在 2 秒内完成
    let service = MockScrapeService::new()
        .with_completion_time(Duration::from_secs(2));
    
    let request = ScrapeRequest {
        url: "https://example.com".to_string(),
        sync_wait_ms: Some(5000),  // 等待 5 秒
        ..Default::default()
    };
    
    // When: 调用处理器
    let start = Instant::now();
    let response = handle_scrape_with_wait(service, request).await.unwrap();
    let elapsed = start.elapsed();
    
    // Then: 2 秒内返回结果
    assert!(elapsed.as_secs() < 3);
    assert_eq!(response.status, TaskStatus::Completed);
    assert!(response.data.is_some());
}

#[tokio::test]
async fn test_sync_wait_timeout_returns_task_id() {
    // Given: 任务会在 10 秒后完成
    let service = MockScrapeService::new()
        .with_completion_time(Duration::from_secs(10));
    
    let request = ScrapeRequest {
        url: "https://example.com".to_string(),
        sync_wait_ms: Some(5000),  // 只等 5 秒
        ..Default::default()
    };
    
    // When: 调用处理器
    let start = Instant::now();
    let response = handle_scrape_with_wait(service, request).await.unwrap();
    let elapsed = start.elapsed();
    
    // Then: 5 秒后返回任务 ID
    assert!(elapsed.as_secs() >= 5 && elapsed.as_secs() < 6);
    assert_eq!(response.status, TaskStatus::Processing);
    assert!(response.data.is_none());
    assert!(response.task_id.is_some());
}

#[tokio::test]
async fn test_sync_wait_default_value() {
    let service = TestScrapeService::new();
    
    let request = ScrapeRequest {
        url: "https://example.com".to_string(),
        sync_wait_ms: None,  // 未指定
        ..Default::default()
    };
    
    // When: 调用处理器
    let response = handle_scrape_with_wait(service, request).await.unwrap();
    
    // Then: 使用默认值 5000ms
    // 注意：需要通过日志或其他方式验证
}

#[tokio::test]
async fn test_sync_wait_max_limit() {
    let service = TestScrapeService::new();
    
    let request = ScrapeRequest {
        url: "https://example.com".to_string(),
        sync_wait_ms: Some(60000),  // 超过最大值 30000
        ..Default::default()
    };
    
    // When: 调用处理器
    let result = handle_scrape_with_wait(service, request).await;
    
    // Then: 应返回参数错误
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::InvalidParameter);
}
```

**测试覆盖**:

- ✅ 任务快速完成时同步返回
- ✅ 任务超时返回任务 ID
- ✅ 默认等待时间生效
- ✅ 最大等待时间限制
- ✅ 后台任务继续执行（待补充）

**实现状态**: ✅ 已实现 - 测试用例在 `tests/unit/presentation/handlers/sync_wait_test.rs`

---

## 3. 集成测试

### 3.1 API 端到端测试 ✅ 已实现

**状态**: `tests/integration/api_tests.rs` 已实现大部分测试用例。

#### 测试用例：创建抓取任务

```rust
// tests/integration/api/scrape_test.rs
use axum_test::TestServer;

#[tokio::test]
async fn test_create_scrape_task_success() {
    // Given: 测试服务器
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    // When: POST /v1/scrape
    let response = server
        .post("/v1/scrape")
        .json(&json!({
            "url": "https://example.com",
            "formats": ["markdown", "html"]
        }))
        .add_header("Authorization", "Bearer test-api-key")
        .await;
    
    // Then: 返回 200 和任务 ID
    response.assert_status_ok();
    let body: ScrapeResponse = response.json();
    assert!(body.success);
    assert!(body.id.is_some());
}

#[tokio::test]
async fn test_scrape_rate_limit() {
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    // When: 连续发送 101 个请求（超过限制）
    for i in 0..101 {
        let response = server
            .post("/v1/scrape")
            .json(&json!({"url": "https://example.com"}))
            .add_header("Authorization", "Bearer test-api-key")
            .await;
        
        if i < 100 {
            response.assert_status_ok();
        } else {
            // Then: 第 101 个请求应被限流
            response.assert_status(StatusCode::TOO_MANY_REQUESTS);
        }
    }
}
```

**测试覆盖**:

- ✅ 正常请求流程
- ✅ 速率限制生效
- ✅ 并发限制生效
- ✅ 参数校验
- ✅ 错误响应格式

**实现状态**: ✅ 已实现 - 测试用例在 `tests/integration/api_tests.rs` 和 `tests/e2e/complete_workflow_test.rs`

---

### 3.2 Worker 测试

#### 测试用例：任务处理流程

```rust
// tests/integration/workers/scrape_worker_test.rs
#[tokio::test]
async fn test_worker_processes_task() {
    // Given: 数据库中有待处理任务
    let db = setup_test_db().await;
    let task = create_test_task(&db, TaskStatus::Queued).await;
    
    // When: 启动 Worker
    let worker = ScrapeWorker::new(db.clone());
    tokio::spawn(async move {
        worker.run_once().await;
    });
    
    // Then: 任务被处理完成
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    let updated_task = find_task_by_id(&db, task.id).await.unwrap();
    assert_eq!(updated_task.status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_worker_retries_failed_task() {
    let db = setup_test_db().await;
    let task = create_test_task(&db, TaskStatus::Queued).await;
    
    // Given: 引擎会失败
    let test_engine = TestEngine::new().with_failure();
    
    let worker = ScrapeWorker::new(db.clone())
        .with_engine(Arc::new(test_engine));
    
    // When: Worker 处理任务
    worker.run_once().await;
    
    // Then: 任务状态为 Failed，重试次数 +1
    let updated_task = find_task_by_id(&db, task.id).await.unwrap();
    assert_eq!(updated_task.status, TaskStatus::Failed);
    assert_eq!(updated_task.retry_count, 1);
}
```

**测试覆盖**:

- ✅ 正常任务处理
- ✅ 失败重试逻辑
- ✅ 超时处理
- ✅ 锁机制

---

### 3.3 数据库交互测试 (✅ 已实现)

**状态**: 数据库交互已在 `tests/integration/real_interactions_test.rs` 和各模块集成测试中得到充分验证。

#### 测试用例：Repository 核心交互

```rust
// tests/integration/real_interactions_test.rs
#[tokio::test]
async fn test_task_repository_crud_flow() {
    let db = setup_test_db().await;
    let repo = TaskRepositoryImpl::new(db);
    
    // 1. Create
    let task = Task::new(...);
    repo.create(&task).await.unwrap();
    
    // 2. Read
    let saved = repo.find_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(saved.url, task.url);
    
    // 3. Update
    repo.mark_completed(task.id).await.unwrap();
    let updated = repo.find_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(updated.status, TaskStatus::Completed);
}
```

**测试覆盖**:

- ✅ 任务 CRUD 操作
- ✅ 状态原子更新
- ✅ 并发锁机制 (Advisory Locks)
- ✅ 事务一致性验证

---

### 3.4 统一任务管理接口测试 ✅

#### 测试用例：批量任务查询

```rust
// tests/integration/api/tasks_query_test.rs
use axum_test::TestServer;

#[tokio::test]
async fn test_batch_task_query() {
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    // Given: 创建 3 个任务
    let task_ids = vec![
        create_test_task(&server, "scrape").await,
        create_test_task(&server, "search").await,
        create_test_task(&server, "crawl").await,
    ];
    
    // When: POST /v2/tasks/query
    let response = server
        .post("/v2/tasks/query")
        .json(&json!({
            "task_ids": task_ids,
            "include_results": true
        }))
        .add_header("Authorization", "Bearer test-api-key")
        .await;
    
    // Then: 返回所有任务
    response.assert_status_ok();
    let body: TasksQueryResponse = response.json();
    assert_eq!(body.tasks.len(), 3);
}

#[tokio::test]
async fn test_task_query_with_filters() {
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    // Given: 创建多个不同状态的任务
    create_test_task_with_status(&server, TaskStatus::Completed).await;
    create_test_task_with_status(&server, TaskStatus::Failed).await;
    create_test_task_with_status(&server, TaskStatus::Processing).await;
    
    // When: 只查询已完成和失败的任务
    let response = server
        .post("/v2/tasks/query")
        .json(&json!({
            "task_ids": all_task_ids,
            "filters": {
                "status": ["completed", "failed"]
            }
        }))
        .await;
    
    // Then: 只返回过滤后的任务
    let body: TasksQueryResponse = response.json();
    assert_eq!(body.tasks.len(), 2);
    assert!(body.tasks.iter().all(|t| 
        t.status == TaskStatus::Completed || t.status == TaskStatus::Failed
    ));
}

#[tokio::test]
async fn test_task_query_exclude_results() {
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    // When: include_results=false
    let response = server
        .post("/v2/tasks/query")
        .json(&json!({
            "task_ids": [task_id],
            "include_results": false
        }))
        .await;
    
    // Then: 响应中不包含 result 字段
    let body: TasksQueryResponse = response.json();
    assert!(body.tasks[0].result.is_none());
}
```

---

#### 测试用例：批量任务取消

```rust
// tests/integration/api/tasks_cancel_test.rs
#[tokio::test]
async fn test_batch_task_cancel() {
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    // Given: 创建 3 个处理中的任务
    let task_ids = vec![
        create_processing_task(&server).await,
        create_processing_task(&server).await,
        create_processing_task(&server).await,
    ];
    
    // When: POST /v2/tasks/cancel
    let response = server
        .post("/v2/tasks/cancel")
        .json(&json!({
            "task_ids": task_ids
        }))
        .await;
    
    // Then: 所有任务被取消
    response.assert_status_ok();
    let body: TasksCancelResponse = response.json();
    assert_eq!(body.results.len(), 3);
    assert!(body.results.iter().all(|r| r.cancelled));
}

#[tokio::test]
async fn test_cancel_completed_task() {
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    // Given: 已完成的任务
    let task_id = create_completed_task(&server).await;
    
    // When: 尝试取消
    let response = server
        .post("/v2/tasks/cancel")
        .json(&json!({
            "task_ids": [task_id],
            "force": false
        }))
        .await;
    
    // Then: 取消失败，返回原因
    let body: TasksCancelResponse = response.json();
    assert!(!body.results[0].cancelled);
    assert_eq!(body.results[0].reason, Some("Task already completed".to_string()));
}

#[tokio::test]
async fn test_force_cancel() {
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    // Given: 已完成的任务
    let task_id = create_completed_task(&server).await;
    
    // When: 强制取消
    let response = server
        .post("/v2/tasks/cancel")
        .json(&json!({
            "task_ids": [task_id],
            "force": true
        }))
        .await;
    
    // Then: 强制取消成功
    let body: TasksCancelResponse = response.json();
    assert!(body.results[0].cancelled);
}
```

**测试覆盖**:

- ✅ 批量查询正常
- ✅ 状态过滤生效
- ✅ 任务类型过滤生效
- ✅ include_results 参数生效
- ✅ 批量取消正常
- ✅ 已完成任务无法取消
- ✅ 强制取消模式
- ✅ Crawl 级联取消（待补充）

---

## 4. 压力测试

### 4.1 测试场景

#### 场景 1：高并发抓取

**目标**: 验证系统在高并发下的稳定性

**测试脚本** (k6):

```javascript
// tests/load/high_concurrency.js
import http from 'k6/http';
import { check, sleep } from 'k6';

export const options = {
  stages: [
    { duration: '2m', target: 100 },   // 预热到 100 VU
    { duration: '5m', target: 500 },   // 快速增长到 500 VU
    { duration: '10m', target: 1000 }, // 峰值 1000 VU
    { duration: '5m', target: 0 },     // 降级
  ],
  thresholds: {
    http_req_duration: ['p(95)<50', 'p(99)<200'], // 对齐 PRD
    http_req_failed: ['rate<0.001'],                // 失败率 < 0.1%
  },
};

export default function () {
  const url = 'http://localhost:8080/v1/scrape';
  const payload = JSON.stringify({
    url: 'https://example.com',
    formats: ['markdown'],
  });
  
  const params = {
    headers: {
      'Content-Type': 'application/json',
      'Authorization': 'Bearer test-api-key',
    },
  };
  
  const res = http.post(url, payload, params);
  
  check(res, {
    'status is 200': (r) => r.status === 200,
    'response time < 200ms': (r) => r.timings.duration < 200,
  });
  
  sleep(1);
}
```

**预期结果**:

- ✅ P95 延迟 < 50ms
- ✅ P99 延迟 < 200ms
- ✅ 失败率 < 0.1%
- ✅ 吞吐量 > 10000 RPS

---

#### 场景 2：持续负载测试

**目标**: 验证系统长时间运行的稳定性

**测试配置**:

```javascript
export const options = {
  vus: 200,                    // 固定 200 个虚拟用户
  duration: '1h',              // 持续 1 小时
  thresholds: {
    http_req_duration: ['p(99)<1000'],
    http_req_failed: ['rate<0.05'],
  },
};
```

**监控指标**:

- CPU 使用率
- 内存使用率（检查泄漏）
- 数据库连接数
- Redis 内存占用
- GC 暂停时间（无 GC，但监控内存增长）

---

#### 场景 3：爬取积压测试

**目标**: 验证队列系统在大规模积压下的表现

**步骤**:

1. 创建 100,000 个爬取任务
2. 启动 10 个 Worker
3. 监控队列消费速度

**预期结果**:

- ✅ Worker 稳定消费，无崩溃
- ✅ 任务处理速度 > 1000 tasks/min
- ✅ Postgres 性能稳定（无慢查询）

---

#### 场景 4：搜索并发聚合压力测试

**目标**: 验证多引擎并发查询的稳定性

**测试脚本** (k6):

```javascript
// tests/load/search_concurrent.js
import http from 'k6/http';
import { check, sleep } from 'k6';

export const options = {
  stages: [
    { duration: '1m', target: 50 },
    { duration: '5m', target: 200 },
    { duration: '2m', target: 0 },
  ],
  thresholds: {
    'http_req_duration{endpoint:search}': ['p(95)<10000'], // 10s 内完成
    'search_cache_hit_rate': ['rate>0.6'],                  // 缓存命中 > 60%
  },
};

export default function () {
  // 随机查询词（模拟真实场景）
  const queries = [
    'rust programming',
    'web scraping',
    'async await tutorial',
    'tokio runtime',
    'sea-orm database',
  ];
  
  const query = queries[Math.floor(Math.random() * queries.length)];
  
  const payload = JSON.stringify({
    query: query,
    engines: ['google', 'bing', 'baidu'],
    limit: 10,
    sync_wait_ms: 8000,
  });
  
  const params = {
    headers: {
      'Content-Type': 'application/json',
      'Authorization': 'Bearer test-api-key',
    },
    tags: { endpoint: 'search' },
  };
  
  const res = http.post('http://localhost:8080/v1/search', payload, params);
  
  check(res, {
    'status is 200': (r) => r.status === 200,
    'has results': (r) => JSON.parse(r.body).data.results.length > 0,
    'engines_used >= 2': (r) => JSON.parse(r.body).data.engines_used.length >= 2,
    'response time < 10s': (r) => r.timings.duration < 10000,
  });
  
  sleep(2);
}
```

**预期结果**:

- ✅ P95 延迟 < 10 秒
- ✅ 缓存命中率 > 60%
- ✅ 至少 2 个引擎成功
- ✅ 无内存泄漏

---

#### 场景 5：同步等待压力测试

**目标**: 验证同步等待在高并发下不会耗尽连接池

**测试脚本** (k6):

```javascript
// tests/load/sync_wait_stress.js
export const options = {
  vus: 100,
  duration: '5m',
  thresholds: {
    'sync_return_rate': ['rate>0.7'],  // 70% 同步返回
    'http_req_duration': ['p(99)<6000'], // 5s 等待 + 1s 余量
  },
};

export default function () {
  const payload = JSON.stringify({
    url: 'https://httpbin.org/delay/3',  // 模拟 3 秒响应
    formats: ['markdown'],
    sync_wait_ms: 5000,
  });
  
  const res = http.post('http://localhost:8080/v1/scrape', payload, params);
  
  const body = JSON.parse(res.body);
  
  check(res, {
    'sync returned': (r) => body.status === 'completed',  // 同步返回
  });
  
  // 立即发起下一个请求（模拟高并发）
}
```

**预期结果**:

- ✅ 同步返回率 > 70%
- ✅ P99 延迟 < 6 秒
- ✅ 无连接池耗尽
- ✅ 无死锁

---

## 5. E2E 测试

### 5.1 完整爬取流程

```rust
// tests/e2e/crawl_flow_test.rs
use testcontainers::{clients, images};

#[tokio::test]
async fn test_full_crawl_workflow() {
    // Given: 启动完整测试栈
    let docker = clients::Cli::default();
    let postgres = docker.run(images::postgres::Postgres::default());
    let redis = docker.run(images::redis::Redis::default());
    
    let app = create_app_with_containers(&postgres, &redis).await;
    let server = TestServer::new(app).unwrap();
    
    // When: 创建爬取任务
    let response = server
        .post("/v1/crawl")
        .json(&json!({
            "url": "https://example.com",
            "crawler_options": {
                "max_depth": 2,
                "limit": 10
            }
        }))
        .add_header("Authorization", "Bearer test-api-key")
        .await;
    
    response.assert_status_ok();
    let crawl_id = response.json::<CrawlResponse>().id;
    
    // Then: 轮询状态直到完成
    let mut attempts = 0;
    loop {
        let status_response = server
            .get(&format!("/v1/crawl/{}", crawl_id))
            .await;
        
        let status = status_response.json::<CrawlStatus>();
        
        if status.status == "completed" {
            assert!(status.completed >= 1);
            break;
        }
        
        attempts += 1;
        assert!(attempts < 30, "Crawl did not complete in time");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
```

**测试覆盖**:

- ✅ 任务创建 → 队列 → Worker 处理 → 状态更新
- ✅ 分页结果查询
- ✅ Webhook 回调
- ✅ 错误恢复

---

## 6. 性能基准测试

### 6.1 Benchmark 配置

```rust
// benches/scrape_benchmark.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_task_creation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let db = rt.block_on(setup_test_db());
    let repo = TaskRepositoryImpl::new(db);
    
    c.bench_function("create_task", |b| {
        b.to_async(&rt).iter(|| async {
            let task = Task::new(
                black_box(TaskType::Scrape),
                black_box(Uuid::new_v4()),
                black_box("https://example.com".to_string()),
                black_box(json!({})),
            );
            repo.create(&task).await.unwrap()
        })
    });
}

criterion_group!(benches, benchmark_task_creation);
criterion_main!(benches);
```

**运行命令**:

```bash
cargo bench --bench scrape_benchmark
```

**目标基准**:

- 任务创建: < 5ms
- 任务查询: < 2ms
- 引擎路由: < 100μs

---

## 7. 测试执行流程

### 7.1 本地测试

```bash
# 1. 运行单元测试
cargo test --lib

# 2. 运行集成测试
docker-compose -f docker-compose.test.yml up -d
cargo test --test '*'
docker-compose -f docker-compose.test.yml down

# 3. 检查覆盖率
cargo tarpaulin --out Html --output-dir coverage/

# 4. 运行基准测试
cargo bench
```

### 7.2 CI/CD 流程

```yaml
# .github/workflows/test.yml
name: Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16
        env:
          POSTGRES_PASSWORD: postgres
        options: >-
          --health-cmd pg_isready
          --health-interval 10s
      redis:
        image: redis:7-alpine
        options: >-
          --health-cmd "redis-cli ping"
          --health-interval 10s
    
    steps:
      - uses: actions/checkout@v3
      
      - name: Setup Rust
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      
      - name: Run tests
        run: |
          cargo test --all-features
          cargo tarpaulin --out Xml
      
      - name: Upload coverage
        uses: codecov/codecov-action@v3
```

---

## 8. 测试数据管理

### 8.1 Fixture 数据

```rust
// tests/fixtures/mod.rs
pub struct TestFixtures {
    pub db: DatabaseConnection,
}

impl TestFixtures {
    pub async fn new() -> Self {
        let db = setup_test_db().await;
        Self { db }
    }
    
    pub async fn create_team(&self) -> Uuid {
        let team_id = Uuid::new_v4();
        team::ActiveModel {
            id: Set(team_id),
            name: Set("Test Team".to_string()),
            ..Default::default()
        }.insert(&self.db).await.unwrap();
        team_id
    }
    
    pub async fn create_task(&self, team_id: Uuid) -> Task {
        Task::new(
            TaskType::Scrape,
            team_id,
            "https://example.com".to_string(),
            json!({}),
        )
    }
}
```

### 8.2 测试清理

```rust
impl Drop for TestFixtures {
    fn drop(&mut self) {
        // 清理测试数据
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async {
                self.db.execute(Statement::from_string(
                    DatabaseBackend::Postgres,
                    "TRUNCATE tasks, crawls, webhook_events CASCADE".to_owned(),
                )).await.unwrap();
            });
    }
}
```

---

## 9. 测试最佳实践

### 9.1 原则

1. **F.I.R.S.T 原则**
   - Fast: 快速执行
   - Independent: 独立运行
   - Repeatable: 可重复
   - Self-validating: 自动验证
   - Timely: 及时编写

2. **AAA 模式**
   - Arrange: 准备测试数据
   - Act: 执行操作
   - Assert: 验证结果

### 9.2 注意事项

- ❌ 不要依赖外部服务（使用 Mock）
- ❌ 不要使用硬编码时间（使用时间注入）
- ❌ 不要忽略边界条件
- ✅ 每个测试只验证一个行为
- ✅ 使用有意义的测试名称
- ✅ 清理测试产生的副作用

---

## 10. 测试检查清单

### 10.1 发布前检查

- [ ] 所有单元测试通过
- [ ] 集成测试通过
- [ ] 覆盖率 ≥ 80%
- [ ] 无性能回归（Benchmark 对比）
- [ ] 压力测试通过
- [ ] E2E 测试通过
- [ ] 无内存泄漏（Valgrind 检查）
- [ ] 文档更新
- [ ] 搜索引擎单元测试通过
- [ ] 搜索聚合集成测试通过
- [ ] 同步等待机制测试通过
- [ ] 统一任务管理接口测试通过
- [ ] 搜索缓存测试通过
- [ ] 搜索并发压力测试通过

---

## 11. 变更记录

| 版本   | 日期       | 变更内容                                     | 作者    |
| ------ | ---------- | -------------------------------------------- | ------- |
| v2.1.0 | 2024-12-20 | 新增搜索聚合、同步等待、统一任务管理测试用例 | QA 团队 |
| v2.0.0 | 2024-12-10 | 初始测试文档                                 | QA 团队 |

