# crawlrs 项目上下文文档

## 项目概述

**crawlrs** 是一个用 Rust 开发的高性能企业级网页数据采集平台，提供搜索、抓取、爬取、映射与结构化提取能力。相比传统 Node.js 方案，性能提升 3-5 倍，P99 延迟降低 50%。

### 核心优势
- 🚀 **高性能**: 单机 10000+ RPS，P99 延迟 < 200ms
- 🛡️ **类型安全**: 利用 Rust 编译期检查，消除 90% 运行时错误
- 🔄 **弹性扩展**: 支持单机和集群部署，按需水平扩展
- 📊 **可观测性**: 内置分布式追踪和 Prometheus 指标
- 🔐 **企业级**: SSRF 防护、速率限制、多租户隔离

### 核心功能
- **搜索 (Search)**: 多引擎并发聚合（Google/Bing/Baidu/Sogou），智能去重排序，支持异步回填
- **抓取 (Scrape)**: 单页面内容获取，支持多格式输出（Markdown/HTML/截图/JSON）
- **爬取 (Crawl)**: 全站递归爬取，支持深度控制和路径过滤
- **提取 (Extract)**: 基于 CSS 选择器和 LLM 的结构化数据提取

### 技术栈
| 组件 | 技术 | 版本 |
|------|------|------|
| **Web 框架** | Axum | 0.8 |
| **ORM** | SeaORM | 1.0 |
| **异步运行时** | Tokio | 1.48 |
| **数据库** | PostgreSQL | 15+ |
| **缓存** | Redis | 7+ |
| **HTTP 客户端** | reqwest | 0.12 |
| **浏览器自动化** | chromiumoxide | 0.8 |
| **限流** | governor | 0.10 |
| **日志** | tracing | 0.1 |
| **AWS SDK** | aws-sdk-s3 | 1.118 |

## 项目结构

```
crawlrs/
├── src/
│   ├── main.rs              # 应用入口，服务启动逻辑
│   ├── lib.rs               # 库导出
│   ├── application/         # 应用层（用例）
│   │   ├── dto/            # 数据传输对象
│   │   └── use_cases/      # 业务用例
│   ├── config/             # 配置管理
│   ├── domain/             # 领域层（核心业务逻辑）
│   │   ├── models/         # 领域模型
│   │   ├── repositories/   # 仓储接口
│   │   ├── search/         # 搜索引擎抽象
│   │   ├── services/       # 领域服务
│   │   └── use_cases/      # 领域用例
│   ├── search/             # 搜索引擎实现 (新架构)
│   ├── engines/            # 抓取引擎实现
│   │   ├── reqwest_engine.rs      # HTTP 引擎
│   │   ├── playwright_engine.rs   # 浏览器引擎
│   │   ├── fire_engine_tls.rs     # TLS 指纹绕过
│   │   ├── fire_engine_cdp.rs     # CDP 协议引擎
│   │   ├── router.rs              # 智能引擎路由器
│   │   ├── circuit_breaker.rs     # 熔断器
│   │   ├── health_monitor.rs      # 健康监控
│   │   └── validators.rs          # 验证器
│   ├── infrastructure/     # 基础设施层
│   │   ├── cache/          # 缓存实现
│   │   ├── database/       # 数据库连接
│   │   ├── repositories/   # 仓储实现
│   │   ├── services/       # 基础设施服务
│   │   ├── observability/  # 可观测性
│   │   ├── geolocation.rs  # 地理位置服务
│   │   ├── metrics.rs      # 指标收集
│   │   └── storage.rs      # 存储抽象
│   ├── presentation/       # 表现层
│   │   ├── handlers/       # HTTP 处理器
│   │   ├── middleware/     # 中间件
│   │   ├── extractors/     # 请求提取器
│   │   └── routes/         # 路由定义
│   ├── queue/              # 任务队列
│   │   ├── task_queue.rs   # 任务队列接口
│   │   └── scheduler.rs    # 任务调度器
│   ├── utils/              # 工具函数
│   │   ├── telemetry.rs    # 遥测初始化
│   │   ├── robots.rs       # Robots.txt 解析
│   │   ├── port_sniffer.rs # 端口嗅探
│   │   ├── retry_policy.rs # 重试策略
│   │   └── url_utils.rs    # URL 工具
│   └── workers/            # 后台工作器
│       ├── manager.rs      # 工作器管理器
│       ├── scrape_worker.rs    # 抓取工作器
│       ├── webhook_worker.rs   # Webhook 工作器
│       ├── backlog_worker.rs   # 积压任务工作器
│       └── expiration_worker.rs # 过期任务工作器
├── migration/              # 数据库迁移
├── config/                 # 配置文件
├── docker/                 # Docker 配置
├── examples/               # 示例代码
│   ├── search/             # 搜索引擎示例
│   └── browser/            # 浏览器引擎示例
├── tests/                  # 测试文件
│   ├── integration/        # 集成测试
│   ├── e2e/               # 端到端测试
│   ├── unit/              # 单元测试
│   └── handlers/          # 处理器测试
├── benches/                # 性能测试
└── docs/                   # 文档
```

## 构建和运行

### 环境要求
- **Rust**: 1.75+ (Edition 2021)
- **PostgreSQL**: 15+
- **Redis**: 7+
- **Docker** (可选): 用于容器化部署

### 开发模式

#### 从源码编译

```bash
# 克隆仓库
git clone https://gitee.com/kirky-x/crawlrs.git
cd crawlrs

# 编译项目
cargo build --release

# 运行测试
cargo test

# 启动 API 服务
cargo run -- api

# 启动 Worker 服务
cargo run -- worker
```

#### 使用配置文件

配置文件位于 `config/default.toml`，包含数据库、Redis、搜索引擎等配置：

```toml
[database]
url = "postgres://crawlrs:password@localhost:5432/crawlrs_test"
max_connections = 20
min_connections = 5
connect_timeout = 30
idle_timeout = 600

[redis]
url = "redis://localhost:6379"

[server]
host = "0.0.0.0"
port = 8899
enable_port_detection = true

[rate_limiting]
enabled = false
default_rpm = 60

[concurrency]
default_team_limit = 10
task_lock_duration_seconds = 300

[search]
ab_test_enabled = false
variant_b_weight = 0.1
default_engine = "baidu"

[search.engines]
google_enabled = false
bing_enabled = true
baidu_enabled = true
sogou_enabled = true

[search.flaresolverr]
enabled = true
url = "http://flaresolverr:8191/v1"
auto_start = true
timeout_seconds = 30
max_retries = 3

[storage]
storage_type = "local"
local_path = "storage"

[webhook]
timeout_seconds = 10
max_retries = 3
retry_interval_seconds = 60
user_agent = "Crawlrs-Webhook/1.0"
secret = "your-webhook-secret"

[llm]
api_key = "ollama"
model = "qwen3:1.7b"
api_base_url = "http://172.24.160.1:11434/v1"
```

### Docker 部署

#### 完整栈启动

```bash
# 启动所有服务（包括基础设施、应用、浏览器、监控）
docker-compose up -d

# 查看日志
docker-compose logs -f crawlrs

# 停止所有服务
docker-compose down
```

#### 分层启动

```bash
# 仅启动基础设施（PostgreSQL, Redis）
docker-compose --profile infrastructure up -d

# 仅启动应用
docker-compose --profile app up -d

# 仅启动浏览器服务
docker-compose --profile browser up -d

# 仅启动监控服务
docker-compose --profile monitoring up -d
```

#### 环境变量配置

创建 `.env` 文件或使用 `docker/.env.example`：

```env
# 数据库配置
DB_USER=crawlrs
DB_PASSWORD=password
DB_NAME=crawlrs
DB_PORT=5432

# Redis 配置
REDIS_HOST=redis
REDIS_PORT=6379

# 应用配置
APP_PORT=3000
RUST_LOG=debug

# 搜索引擎配置
SEARCH_ENGINE_GOOGLE_ENABLED=false
SEARCH_ENGINE_BING_ENABLED=true
SEARCH_ENGINE_BAIDU_ENABLED=true
SEARCH_ENGINE_SOGOU_ENABLED=true

# Google 搜索 API
GOOGLE_SEARCH_API_KEY=your-api-key
GOOGLE_SEARCH_CX=your-cx-id

# LLM 配置
LLM_API_KEY=your-llm-api-key
LLM_MODEL=gpt-3.5-turbo
LLM_API_BASE_URL=https://api.openai.com/v1
```

### 服务类型

项目支持两种服务模式：

1. **API 服务** (`cargo run -- api`): 提供 HTTP API 接口，包含 Webhook 处理
   - 启动 Webhook Worker 处理事件
   - 启动 Backlog Worker 处理积压任务
   - 提供完整的 REST API

2. **Worker 服务** (`cargo run -- worker`): 后台任务处理，执行抓取任务
   - 启动多个 Scrape Worker 并行处理任务
   - 执行实际的网页抓取和内容提取
   - 处理结果存储和通知

### 数据库迁移

数据库迁移使用 SeaORM，迁移文件位于 `migration/src/`：

```bash
# 运行迁移
cargo run --bin migrate

# 或在启动时自动运行（main.rs 中已配置）
```

### 健康检查

```bash
# 检查服务状态
curl http://localhost:8899/health

# 查看版本信息
curl http://localhost:8899/v1/version

# 查看 Prometheus 指标
curl http://localhost:8899/metrics
```

## 开发约定

### 代码风格

#### Rust 规范

```bash
# 格式化代码
cargo fmt

# Lint 检查
cargo clippy -- -D warnings

# 运行测试
cargo test

# 运行特定测试
cargo test integration_tests

# 运行性能测试
cargo bench
```

#### 架构分层

项目采用六边形架构（Hexagonal Architecture）：

1. **领域层 (Domain)**: 核心业务逻辑，不依赖外部框架
   - `models/`: 领域模型（Task, ScrapeResult, Crawl, Webhook 等）
   - `repositories/`: 仓储接口
   - `search/`: 搜索引擎接口
   - `services/`: 领域服务接口（RateLimitingService, TeamService 等）

2. **应用层 (Application)**: 编排领域对象执行用例
   - `use_cases/`: 具体用例实现（CreateScrapeUseCase 等）
   - `dto/`: 数据传输对象

3. **基础设施层 (Infrastructure)**: 外部依赖实现
   - `repositories/`: 仓储实现（TaskRepositoryImpl, ScrapeResultRepositoryImpl 等）
   - `cache/`: 缓存实现（RedisClient）
   - `services/`: 基础设施服务（RateLimitingServiceImpl, WebhookServiceImpl）
   - `observability/`: 可观测性组件

4. **表现层 (Presentation)**: HTTP 接口
   - `handlers/`: 请求处理器（scrape_handler, crawl_handler, search_handler 等）
   - `middleware/`: 中间件（auth_middleware, rate_limit_middleware, team_semaphore_middleware）
   - `routes/`: 路由定义
   - `extractors/`: 请求提取器

#### 依赖注入

使用 `Arc` 和 trait 实现依赖注入：

```rust
// 仓储实现
let task_repo = Arc::new(TaskRepositoryImpl::new(db.clone()));

// 服务实现
let rate_limiting_service = Arc::new(RateLimitingServiceImpl::new(
    redis_client.clone(),
    task_repo.clone(),
    tasks_backlog_repo.clone(),
    credits_repo.clone(),
    rate_limiting_config,
));

// 注入到处理器
.layer(Extension(task_repo))
.layer(Extension(rate_limiting_service))
```

### 测试规范

#### 测试类型

1. **单元测试**: 位于 `src/` 各模块内，使用 `#[cfg(test)]`
2. **集成测试**: 位于 `tests/integration/`，测试组件交互
3. **端到端测试**: 位于 `tests/e2e/`，测试完整业务流程
4. **性能测试**: 位于 `benches/`，使用 criterion

#### 测试运行

```bash
# 运行所有测试
cargo test

# 运行集成测试
cargo test --test integration_tests

# 运行特定测试
cargo test test_scrape_handler

# 运行测试并显示输出
cargo test -- --nocapture

# 运行性能测试
cargo bench
```

#### 测试状态

根据最新的测试报告（2025-12-31）：
- **总测试数**: 118
- **通过**: 92
- **失败**: 4
- **忽略**: 22
- **通过率**: 78.0%

**仍需修复的测试**:
1. `test_task_status_transitions` - 间歇性失败
2. `test_uat016_sync_wait_integration` - 任务状态转换问题
3. `test_extract_css_only_no_credit_deduction` - 间歇性超时
4. `test_extract_with_rules_credit_deduction` - 间歇性超时

### 错误处理

使用 `thiserror` 和 `anyhow` 进行错误处理：

```rust
// 自定义错误类型
#[derive(Debug, thiserror::Error)]
pub enum CrawlError {
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,
}

// 在 main 函数中使用 anyhow
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ...
}
```

### 日志规范

使用 `tracing` 进行结构化日志：

```rust
use tracing::{info, warn, error, debug};

// 初始化日志
telemetry::init_telemetry();

// 记录日志
info!("Starting crawlrs...");
debug!("Processing request: {:?}", request);
warn!("Rate limit approaching: {}/{}", current, limit);
error!("Failed to scrape URL: {}", url, error = %err);
```

### 并发控制

#### 速率限制

使用 `governor` 实现令牌桶限流：

```rust
// 配置
[rate_limiting]
enabled = true
default_rpm = 60  // 每分钟 60 次请求
```

#### 并发控制

使用信号量控制并发任务数：

```rust
// 配置
[concurrency]
default_team_limit = 10  // 每个团队最多 10 个并发任务
task_lock_duration_seconds = 300  // 任务锁定时间
```

### 配置管理

#### 配置加载

使用 `config` crate 加载配置，支持环境变量覆盖：

```rust
// 环境变量格式: CRAWLRS__SECTION__KEY
// 示例: CRAWLRS__DATABASE__URL=postgres://...

let settings = Settings::new()?;
```

#### 配置优先级

1. 环境变量（最高优先级）
2. 配置文件 (`config/default.toml`)
3. 默认值

### 数据库操作

#### 使用 SeaORM

```rust
use sea_orm::*;

// 查询
let tasks = Task::find()
    .filter(task::Column::Status.eq(TaskStatus::Pending))
    .limit(10)
    .all(db.as_ref())
    .await?;

// 插入
let task = task::ActiveModel {
    url: Set(url.to_string()),
    status: Set(TaskStatus::Pending),
    ..Default::default()
};
task.insert(db.as_ref()).await?;

// 更新
let task: task::ActiveModel = task.into();
task.status = Set(TaskStatus::Completed);
task.update(db.as_ref()).await?;
```

## 核心功能实现

### 智能搜索引擎

#### SearchAggregator

多搜索引擎并发聚合，支持智能去重和排序：

```rust
use crawlrs::search::aggregator::SearchAggregator;

// 创建搜索引擎
let search_engines: Vec<Arc<dyn SearchEngine>> = vec![
    smart::create_google_smart_search(router.clone()),
    smart::create_bing_smart_search(router.clone()),
    smart::create_baidu_smart_search(router.clone()),
    smart::create_sogou_smart_search(router.clone()),
];

// 创建聚合器
let aggregator = Arc::new(SearchAggregator::new(search_engines, 10000));
```

#### SearchABTestEngine

A/B 测试引擎，支持流量分配和结果对比：

```rust
use crawlrs::search::ab_test::SearchABTestEngine;

// 创建 A/B 测试引擎
let ab_test_engine = Arc::new(SearchABTestEngine::new(
    variant_a,  // 控制组引擎
    variant_b,  // 实验组引擎
    0.1,        // variant_b 的流量权重 (10%)
));
```

#### SmartSearch

智能搜索引擎，根据页面类型自动选择最优抓取引擎：

```rust
use crawlrs::search::smart;

// 创建 Google 智能搜索引擎
let google_search = smart::create_google_smart_search(router.clone());

// 智能搜索引擎会自动选择：
// - ReqwestEngine: 静态页面
// - PlaywrightEngine: 动态页面
// - FireEngineTls: TLS 指纹绕过
// - FireEngineCdp: CDP 协议
```

### 智能抓取引擎

#### EngineRouter

智能引擎路由器，根据页面特征自动选择最优引擎：

```rust
use crawlrs::engines::router::EngineRouter;

// 引擎优先级
// 1. ReqwestEngine - 静态页面（最快）
// 2. PlaywrightEngine - 动态页面（支持 JS）
// 3. FireEngineTls - TLS 指纹绕过（反爬虫）
// 4. FireEngineCdp - CDP 协议（高级场景）

let engines: Vec<Arc<dyn ScraperEngine>> = vec![
    Arc::new(ReqwestEngine),
    Arc::new(PlaywrightEngine),
    Arc::new(FireEngineTls::new()),
    Arc::new(FireEngineCdp::new()),
];

let router = Arc::new(EngineRouter::new(engines));

// 负载均衡策略
// - RoundRobin: 轮询
// - WeightedRoundRobin: 加权轮询（基于成功率）
// - LeastConnections: 最少连接
// - FastestResponse: 最快响应时间
// - Random: 随机
// - SmartMixed: 智能混合（默认）
```

#### CircuitBreaker

熔断器，自动降级保护系统：

```rust
use crawlrs::engines::circuit_breaker::CircuitBreaker;

// 熔断器状态
// - Closed: 正常状态
// - Open: 熔断状态，拒绝请求
// - HalfOpen: 半开状态，尝试恢复

// 熔断器配置
let breaker = CircuitBreaker::new(
    failure_threshold,  // 失败阈值
    success_threshold,  // 成功阈值
    timeout,            // 熔断超时
);
```

### 任务队列系统

#### PostgresTaskQueue

基于 PostgreSQL 的任务队列实现：

```rust
use crawlrs::queue::task_queue::PostgresTaskQueue;

let queue = Arc::new(PostgresTaskQueue::new(task_repo.clone()));

// 任务调度器
let scheduler = Arc::new(Scheduler::new(queue.clone()));

// 支持优先级队列
// 支持任务锁定
// 支持任务超时
```

#### BacklogWorker

积压任务处理 Worker，自动处理因限流而延迟的任务：

```rust
use crawlrs::workers::backlog_worker::BacklogWorker;

let backlog_worker = BacklogWorker::new(
    tasks_backlog_repo.clone(),
    task_repo.clone(),
    rate_limiting_service.clone(),
    Duration::from_secs(30),  // 处理间隔
    10,                       // 每次处理的最大任务数
);

tokio::spawn(async move {
    let _ = backlog_worker.run().await;
});
```

### 限流和并发控制

#### 两层限流机制

1. **API 层限流**: 使用 Redis 实现令牌桶算法
2. **团队并发控制**: 使用分布式信号量控制并发数

```rust
use crawlrs::infrastructure::services::rate_limiting_service_impl::RateLimitingServiceImpl;

let rate_limiting_config = RateLimitingConfig {
    redis_key_prefix: "crawlrs".to_string(),
    rate_limit: RateLimitConfig {
        strategy: RateLimitStrategy::TokenBucket,
        requests_per_second: 1,
        requests_per_minute: 60,
        requests_per_hour: 3600,
        bucket_capacity: Some(60),
        enabled: true,
    },
    concurrency: ConcurrencyConfig {
        strategy: ConcurrencyStrategy::DistributedSemaphore,
        max_concurrent_tasks: 10,
        max_concurrent_per_team: 10,
        lock_timeout_seconds: 300,
        enabled: true,
    },
    backlog_process_interval_seconds: 30,
    rate_limit_ttl_seconds: 3600,
};

let rate_limiting_service = Arc::new(RateLimitingServiceImpl::new(
    redis_client.clone(),
    task_repo.clone(),
    tasks_backlog_repo.clone(),
    credits_repo.clone(),
    rate_limiting_config,
));
```

### 地理位置限制

#### GeoLocationService

地理位置服务，支持基于地理位置的访问控制：

```rust
use crawlrs::infrastructure::geolocation::GeoLocationService;

let geo_service = GeoLocationService::new();

// 检查 IP 地址的地理位置
let location = geo_service.get_location("8.8.8.8").await?;

// 使用 GeoRestrictionRepository 检查限制
let geo_restriction_repo = Arc::new(DatabaseGeoRestrictionRepository::new(db.clone()));

let team_service = Arc::new(TeamService::new(
    geo_service,
    geo_restriction_repo.clone(),
));
```

### Webhook 集成

#### WebhookServiceImpl

Webhook 服务，支持可靠的事件通知：

```rust
use crawlrs::infrastructure::services::webhook_service_impl::WebhookServiceImpl;

let webhook_service = Arc::new(WebhookServiceImpl::new(secret.clone()));

// Outbox 模式确保可靠性
// 指数退避重试
// 支持多种事件类型
```

#### WebhookWorker

Webhook 事件处理 Worker：

```rust
use crawlrs::workers::webhook_worker::WebhookWorker;

let webhook_worker = WebhookWorker::new(
    webhook_event_repository.clone(),
    webhook_service,
    RetryPolicy::default(),
);

tokio::spawn(async move {
    let _ = webhook_worker.run().await;
});
```

### 存储服务

#### 存储抽象

支持本地存储和 S3 存储：

```rust
use crawlrs::infrastructure::storage;

// 创建存储仓库
let storage_repo: Option<Arc<dyn StorageRepository + Send + Sync>> =
    match storage::create_storage_repository(&settings.storage) {
        Ok(repo) => Some(Arc::from(repo)),
        Err(e) => {
            error!("Failed to initialize storage repository: {}", e);
            return Err(e.into());
        }
    };

// 配置
[storage]
storage_type = "local"  # 或 "s3"
local_path = "storage"
s3_region = "us-east-1"
s3_bucket = "my-bucket"
s3_access_key = "access-key"
s3_secret_key = "secret-key"
```

### LLM 集成

#### LLM API 调用

支持集成 LLM 进行智能提取：

```rust
// 配置
[llm]
api_key = "your-llm-api-key"
model = "gpt-3.5-turbo"
api_base_url = "https://api.openai.com/v1"

# 或使用本地 Ollama
[llm]
api_key = "ollama"
model = "qwen3:1.7b"
api_base_url = "http://localhost:11434/v1"
```

### FlareSolverr 集成

#### 绕过 Cloudflare 保护

使用 FlareSolverr 代理绕过 Cloudflare 等反爬虫保护：

```toml
[search.flaresolverr]
enabled = true
url = "http://flaresolverr:8191/v1"
auto_start = true
timeout_seconds = 30
max_retries = 3
```

## 性能优化

### 缓存策略

- 使用 Redis 缓存 robots.txt
- 使用 LRU 缓存热点数据
- 使用 CDN 缓存静态资源

### 并发优化

- 使用 Tokio 异步运行时
- 使用连接池复用数据库连接
- 使用信号量控制并发数
- 使用任务队列异步处理

### 引擎选择

根据页面类型选择合适的引擎：
- 静态页面 → ReqwestEngine
- SPA 应用 → PlaywrightEngine
- 反爬虫站点 → FireEngineTls/CDP

### 负载均衡

使用 EngineRouter 实现智能负载均衡：
- 轮询（RoundRobin）
- 加权轮询（WeightedRoundRobin）
- 最少连接（LeastConnections）
- 最快响应（FastestResponse）
- 智能混合（SmartMixed）

## 常见问题

### 端口冲突

如果默认端口被占用，系统会自动检测并切换到可用端口：

```bash
# 禁用端口检测
[server]
enable_port_detection = false
```

### 数据库连接失败

检查数据库配置和网络连接：

```bash
# 测试数据库连接
psql postgres://crawlrs:password@localhost:5432/crawlrs_test
```

### Redis 连接失败

检查 Redis 服务状态：

```bash
# 测试 Redis 连接
redis-cli -h localhost -p 6379 ping
```

### 任务队列阻塞

检查并发配置和任务锁定时间：

```toml
[concurrency]
default_team_limit = 10
task_lock_duration_seconds = 300
```

### 搜索引擎失败

检查搜索引擎配置和网络连接：

```bash
# 检查 FlareSolverr 状态
curl http://localhost:8191/v1

# 查看搜索引擎日志
docker-compose logs -f crawlrs | grep search
```

### 限流触发

检查限流配置和团队配额：

```bash
# 查看当前限流状态
redis-cli GET crawlrs:rate_limit:team:{team_id}

# 重置限流
redis-cli DEL crawlrs:rate_limit:team:{team_id}
```

## 贡献指南

### 开发流程

1. Fork 仓库
2. 创建特性分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建 Pull Request

### 代码审查

- 确保所有测试通过
- 运行 `cargo fmt` 格式化代码
- 运行 `cargo clippy` 检查代码质量
- 更新相关文档

### 测试要求

- 单元测试覆盖率 > 80%
- 集成测试必须通过
- 新功能需要添加相应的测试
- 性能测试不能出现退化

## 相关文档

- [README.md](./README.md) - 项目介绍
- [README_zh.md](./README_zh.md) - 中文版项目介绍
- [USER_GUIDE.md](./USER_GUIDE.md) - 用户使用手册
- [TEST_STATUS_REPORT.md](./TEST_STATUS_REPORT.md) - 测试状态报告
- [API 文档](./docs/API.md) - API 参考
- [架构文档](./docs/architecture.md) - 系统架构说明

## 许可证

MIT License - 详见 [LICENSE](./LICENSE) 文件