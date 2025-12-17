# crawlrs - 测试文档 (TEST)

## 版本信息
- **文档版本**: v2.0.0
- **测试框架**: Rust Test + tokio::test
- **最近更新**: 2024-12-10

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

## 3. 集成测试

### 3.1 API 端到端测试 (❌ 未完成)

**状态**: `tests/integration/api_tests.rs` 文件为空，测试用例未实现。

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
    let mock_engine = MockEngine::new().with_failure();
    
    let worker = ScrapeWorker::new(db.clone())
        .with_engine(Arc::new(mock_engine));
    
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

### 3.3 数据库交互测试 (❌ 未完成)

**状态**: `tests/integration/repositories/task_repository_test.rs` 文件缺失，测试用例未实现。

#### 测试用例：仓储操作

```rust
// tests/integration/repositories/task_repository_test.rs
#[tokio::test]
async fn test_repository_crud_operations() {
    let db = setup_test_db().await;
    let repo = TaskRepositoryImpl::new(db.clone());
    
    // Create
    let task = Task::new(TaskType::Scrape, team_id, url, payload);
    let created = repo.create(&task).await.unwrap();
    assert_eq!(created.id, task.id);
    
    // Read
    let found = repo.find_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(found.url, task.url);
    
    // Update
    let mut updated_task = found.clone();
    updated_task.status = TaskStatus::Active;
    let updated = repo.update(&updated_task).await.unwrap();
    assert_eq!(updated.status, TaskStatus::Active);
    
    // Delete (标记为 Cancelled)
    repo.mark_cancelled(task.id).await.unwrap();
    let cancelled = repo.find_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(cancelled.status, TaskStatus::Cancelled);
}

#[tokio::test]
async fn test_repository_acquire_next_task() {
    let db = setup_test_db().await;
    let repo = TaskRepositoryImpl::new(db.clone());
    
    // Given: 3 个待处理任务，优先级不同
    create_test_task(&db, TaskStatus::Queued, priority: 1).await;
    create_test_task(&db, TaskStatus::Queued, priority: 10).await;
    create_test_task(&db, TaskStatus::Queued, priority: 5).await;
    
    // When: Worker 获取任务
    let worker_id = Uuid::new_v4();
    let task = repo.acquire_next(worker_id).await.unwrap().unwrap();
    
    // Then: 应获取优先级最高的任务（10）
    assert_eq!(task.priority, 10);
    assert_eq!(task.lock_token, Some(worker_id));
}
```

**测试覆盖**:
- ❌ CRUD 基本操作
- ❌ 事务一致性
- ❌ 并发锁机制
- ❌ 查询性能

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

---

## 变更记录

| 版本 | 日期 | 变更内容 | 作者 |
|------|------|---------|------|
| v2.0.0 | 2024-12-10 | 初始测试文档 | QA 团队 |