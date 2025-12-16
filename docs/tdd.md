# crawlrs - 技术设计文档 (TDD)

## 版本信息
- **文档版本**: v2.0.0
- **Rust 版本**: 1.75+ (Edition 2021)
- **最近更新**: 2024-12-10

---

## 1. 技术栈选型

### 1.1 核心依赖

| 组件            | 技术选型               | 版本   | 选型理由                                       | 状态 |
|---------------|--------------------|------|--------------------------------------------|----|
| **Web 框架**    | Axum               | 0.7  | 基于 Tower，性能最优，类型安全                         | ✅  |
| **ORM**       | SeaORM             | 1.0  | 异步、迁移管理、编译期类型检查                            | ✅  |
| **HTTP 客户端**  | reqwest            | 0.12 | 生态成熟，支持连接池和 HTTP/2                         | ✅  |
| **异步运行时**     | tokio              | 1.36 | 业界标准，生态完善                                  | ✅  |
| **限流**        | redis              | 0.24 | 使用 Redis INCR/EXPIRE 实现分布式限流 (替代 governor) | ✅ |
| **序列化**       | serde              | 1.0  | Rust 生态标准                                  | ✅  |
| **日志**        | tracing            | 0.1  | 结构化日志，与 tokio 深度集成                         | ✅  |
| **错误处理**      | thiserror + anyhow | -    | 库用 thiserror，应用用 anyhow                    | ✅  |
| **配置管理**      | config             | 0.13 | 多环境配置支持                                    | ✅  |
| **Redis 客户端** | redis              | 0.24 | 异步支持，连接池管理                                 | ✅  |
| **对象存储**      | aws-sdk-s3         | 1.0  | 官方 SDK，兼容 GCS/MinIO                        | ✅  |
| **浏览器引擎**     | chromiumoxide      | 0.5  | Rust 原生 CDP 客户端，无头浏览器支持                    | ✅  |

### 1.2 Cargo.toml（关键依赖）

```toml
[dependencies]
# Web 框架
axum = { version = "0.7", features = ["ws", "macros"] }
tower = "0.4"
tower-http = { version = "0.5", features = ["trace", "cors"] }

# 数据库
sea-orm = { version = "1.0", features = ["sqlx-postgres", "runtime-tokio-rustls", "macros"] }
redis = { version = "0.24", features = ["tokio-comp", "connection-manager"] }

# HTTP 客户端
reqwest = { version = "0.11", features = ["json", "rustls-tls", "cookies"] }

# 异步运行时
tokio = { version = "1.35", features = ["full"] }

# 序列化
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# 日志与追踪
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# 错误处理
thiserror = "1.0"
anyhow = "1.0"

# 配置
config = "0.13"
dotenvy = "0.15"

# 工具库
uuid = { version = "1.6", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
```

---

## 2. 架构分层设计

### 2.1 DDD 分层架构

```
┌───────────────────────────────────────┐
│        Presentation Layer             │  ← API 路由、中间件、请求验证
├───────────────────────────────────────┤
│        Application Layer              │  ← 用例编排、事务管理
├───────────────────────────────────────┤
│        Domain Layer                   │  ← 核心业务逻辑、领域模型
├───────────────────────────────────────┤
│        Infrastructure Layer           │  ← 数据库、缓存、外部服务
└───────────────────────────────────────┘
```

### 2.2 目录结构

```
crawlrs/
├── Cargo.toml
├── .env.example
├── migrations/                      # SeaORM 迁移文件
│   ├── 001_create_tasks.sql
│   ├── 002_create_crawls.sql
│   └── 003_create_webhook_events.sql
│
├── src/
│   ├── main.rs                     # 程序入口
│   ├── lib.rs                      # 库导出
│   │
│   ├── config/                     # 配置管理
│   │   ├── mod.rs
│   │   └── settings.rs             # 环境变量 + config 文件
│   │
│   ├── domain/                     # 领域层（核心业务逻辑）
│   │   ├── mod.rs
│   │   ├── models/                 # 领域模型
│   │   │   ├── task.rs
│   │   │   ├── crawl.rs
│   │   │   └── webhook.rs
│   │   ├── services/               # 领域服务
│   │   │   ├── scrape_service.rs
│   │   │   ├── crawl_service.rs
│   │   │   └── extract_service.rs
│   │   └── repositories/           # 仓储接口（Trait）
│   │       ├── task_repository.rs
│   │       └── crawl_repository.rs
│   │
│   ├── application/                # 应用层（用例编排）
│   │   ├── mod.rs
│   │   ├── usecases/
│   │   │   ├── create_scrape.rs
│   │   │   ├── create_crawl.rs
│   │   │   └── query_status.rs
│   │   └── dto/                    # 数据传输对象
│   │       ├── scrape_request.rs
│   │       └── scrape_response.rs
│   │
│   ├── infrastructure/             # 基础设施层
│   │   ├── mod.rs
│   │   ├── database/
│   │   │   ├── mod.rs
│   │   │   ├── connection.rs       # SeaORM 连接池
│   │   │   └── entities/           # SeaORM 生成的实体
│   │   ├── cache/
│   │   │   └── redis_client.rs
│   │   ├── storage/
│   │   │   └── s3_client.rs
│   │   └── repositories/           # 仓储实现
│   │       ├── task_repo_impl.rs
│   │       └── crawl_repo_impl.rs
│   │
│   ├── presentation/               # 表现层（API）
│   │   ├── mod.rs
│   │   ├── routes/
│   │   │   ├── mod.rs
│   │   │   ├── scrape.rs
│   │   │   ├── crawl.rs
│   │   │   └── extract.rs
│   │   ├── middleware/
│   │   │   ├── auth.rs
│   │   │   ├── rate_limit.rs
│   │   │   └── team_semaphore.rs
│   │   └── handlers/               # 控制器
│   │       └── scrape_handler.rs
│   │
│   ├── workers/                    # Worker 进程
│   │   ├── mod.rs
│   │   ├── manager.rs              # Worker 管理器
│   │   ├── scrape_worker.rs
│   │   └── webhook_worker.rs
│   │
│   ├── engines/                    # 抓取引擎
│   │   ├── mod.rs
│   │   ├── traits.rs               # 引擎 Trait 定义
│   │   ├── router.rs               # 智能路由
│   │   ├── fetch_engine.rs
│   │   ├── playwright_engine.rs
│   │   ├── fire_engine.rs
│   │   ├── circuit_breaker.rs
│   │   └── health_monitor.rs
│   │
│   ├── queue/                      # 队列系统
│   │   ├── mod.rs
│   │   ├── task_queue.rs
│   │   └── scheduler.rs
│   │
│   └── utils/                      # 工具函数
│       ├── mod.rs
│       ├── errors.rs
│       ├── telemetry.rs
│       └── validators.rs
│
└── tests/
    ├── integration/
    │   ├── api_tests.rs
    │   └── worker_tests.rs
    └── load/
        └── stress_test.rs
```

---

## 3. 核心模块设计

### 3.1 领域模型（Domain Models）

#### 3.1.1 Task 领域模型

```rust
// domain/models/task.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub priority: i32,
    pub team_id: Uuid,
    pub url: String,
    pub payload: serde_json::Value,
    pub retry_count: i32,
    pub max_retries: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskType {
    Scrape,
    Crawl,
    Extract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Queued,
    Active,
    Completed,
    Failed,
    Cancelled,
}

// 状态机转换（编译期保证合法性）
impl Task {
    pub fn start(mut self) -> Result<Self, DomainError> {
        match self.status {
            TaskStatus::Queued => {
                self.status = TaskStatus::Active;
                Ok(self)
            }
            _ => Err(DomainError::InvalidStateTransition),
        }
    }

    pub fn complete(mut self) -> Result<Self, DomainError> {
        match self.status {
            TaskStatus::Active => {
                self.status = TaskStatus::Completed;
                Ok(self)
            }
            _ => Err(DomainError::InvalidStateTransition),
        }
    }
}
```

#### 3.1.2 仓储接口（Repository Trait）

```rust
// domain/repositories/task_repository.rs
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn create(&self, task: &Task) -> Result<Task, RepositoryError>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError>;
    async fn update(&self, task: &Task) -> Result<Task, RepositoryError>;
    async fn acquire_next(&self, worker_id: Uuid) -> Result<Option<Task>, RepositoryError>;
    async fn mark_completed(&self, id: Uuid) -> Result<(), RepositoryError>;
}
```

---

### 3.2 应用层（Application Layer）

#### 3.2.1 用例示例：创建抓取任务

**状态**: ❌ 未完成 (文件存在但为空)

```rust
// application/usecases/create_scrape.rs
// TODO: Implement logic matching the design below
//
// use crate::domain::repositories::TaskRepository;
// use crate::application::dto::{ScrapeRequest, ScrapeResponse};
//
// pub struct CreateScrapeUseCase<R: TaskRepository> {
//     task_repo: Arc<R>,
//     rate_limiter: Arc<RateLimiter>,
//     team_semaphore: Arc<TeamSemaphore>,
// }
// ...
```

---

### 3.3 基础设施层（Infrastructure）

#### 3.3.1 SeaORM 实体定义

```rust
// infrastructure/database/entities/task.rs
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tasks")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub task_type: String,
    pub status: String,
    pub priority: i32,
    pub team_id: Uuid,
    pub url: String,
    pub payload: Json,
    pub retry_count: i32,
    pub max_retries: i32,
    pub created_at: DateTimeUtc,
    pub started_at: Option<DateTimeUtc>,
    pub completed_at: Option<DateTimeUtc>,
    pub lock_token: Option<Uuid>,
    pub lock_expires_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
```

#### 3.3.2 仓储实现

```rust
// infrastructure/repositories/task_repo_impl.rs
use sea_orm::*;

pub struct TaskRepositoryImpl {
    db: DatabaseConnection,
}

#[async_trait]
impl TaskRepository for TaskRepositoryImpl {
    async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
        let model = task_entity::ActiveModel {
            id: Set(task.id),
            task_type: Set(task.task_type.to_string()),
            status: Set(task.status.to_string()),
            team_id: Set(task.team_id),
            url: Set(task.url.clone()),
            payload: Set(task.payload.clone()),
            ..Default::default()
        };
        
        let result = model.insert(&self.db).await?;
        Ok(result.into())
    }
    
    async fn acquire_next(&self, worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
        // 使用悲观锁（SELECT FOR UPDATE SKIP LOCKED）
        let task = task_entity::Entity::find()
            .filter(task_entity::Column::Status.eq("queued"))
            .order_by_desc(task_entity::Column::Priority)
            .lock_with_behavior(LockBehavior::SkipLocked)
            .one(&self.db)
            .await?;
        
        if let Some(mut task) = task {
            // 更新锁信息
            let mut active: task_entity::ActiveModel = task.clone().into();
            active.lock_token = Set(Some(worker_id));
            active.lock_expires_at = Set(Some(Utc::now() + Duration::minutes(5)));
            active.status = Set("active".to_string());
            
            let updated = active.update(&self.db).await?;
            return Ok(Some(updated.into()));
        }
        
        Ok(None)
    }
}
```

---

### 3.4 引擎层（Engines）

#### 3.4.1 引擎 Trait 定义

```rust
// engines/traits.rs
use async_trait::async_trait;

#[async_trait]
pub trait ScraperEngine: Send + Sync {
    /// 执行抓取
    async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError>;
    
    /// 计算对请求的支持分数（0-100）
    fn support_score(&self, request: &ScrapeRequest) -> u8;
    
    /// 引擎名称
    fn name(&self) -> &'static str;
}

pub struct ScrapeRequest {
    pub url: String,
    pub headers: HashMap<String, String>,
    pub timeout: Duration,
    pub needs_js: bool,
    pub needs_screenshot: bool,
    pub mobile: bool,
}

pub struct ScrapeResponse {
    pub status_code: u16,
    pub content: String,
    pub content_type: String,
    pub response_time_ms: u64,
}
```

#### 3.4.2 Fetch 引擎实现

```rust
// engines/fetch_engine.rs
pub struct FetchEngine;

#[async_trait]
impl ScraperEngine for FetchEngine {
    async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        // 每个请求创建独立 Client（隔离 Cookie）
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 ...")
            .timeout(request.timeout)
            .cookie_store(true)
            .build()?;
        
        let start = Instant::now();
        let response = client.get(&request.url)
            .headers(request.headers.clone())
            .send()
            .await?;
        
        let status_code = response.status().as_u16();
        let content_type = response.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html")
            .to_string();
        
        let content = response.text().await?;
        
        Ok(ScrapeResponse {
            status_code,
            content,
            content_type,
            response_time_ms: start.elapsed().as_millis() as u64,
        })
    }
    
    fn support_score(&self, request: &ScrapeRequest) -> u8 {
        if request.needs_js || request.needs_screenshot {
            return 0;  // 不支持
        }
        100  // 最高优先级（最快）
    }
    
    fn name(&self) -> &'static str {
        "fetch"
    }
}
```

#### 3.4.3 智能路由器

```rust
// engines/router.rs
pub struct EngineRouter {
    engines: Vec<Arc<dyn ScraperEngine>>,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl EngineRouter {
    pub async fn route(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        // 按支持分数排序
        let mut scored_engines: Vec<_> = self.engines
            .iter()
            .map(|e| (e.support_score(request), e))
            .collect();
        scored_engines.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        
        // 依次尝试
        for (score, engine) in scored_engines {
            if score == 0 {
                continue;  // 跳过不支持的引擎
            }
            
            if self.circuit_breaker.is_open(engine.name()) {
                tracing::warn!("Circuit breaker open for {}", engine.name());
                continue;
            }
            
            match engine.scrape(request).await {
                Ok(response) => {
                    self.circuit_breaker.record_success(engine.name());
                    return Ok(response);
                }
                Err(e) if e.is_retryable() => {
                    self.circuit_breaker.record_failure(engine.name());
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        
        Err(EngineError::AllEnginesFailed)
    }
}
```

---

### 3.5 并发控制

#### 3.5.1 速率限制器
**状态**: ✅ 已实现 (使用 Redis INCR/EXPIRE)

```rust
// presentation/middleware/rate_limit_middleware.rs
use crate::infrastructure::cache::redis_client::RedisClient;

pub struct RateLimiter {
    redis_client: RedisClient,
    default_limit_per_minute: u32,
}

impl RateLimiter {
    pub async fn check(&self, api_key: &str) -> Result<(), RateLimitError> {
        let key = format!("rate_limit:{}", api_key);
        // 使用 Redis INCR + EXPIRE 实现
        let current_requests = self.redis_client.incr(&key).await?;
        
        if current_requests == 1 {
            self.redis_client.expire(&key, 60).await?;
        }
        
        let limit = self.get_rate_limit(api_key).await?;
        
        if current_requests > limit.into() {
            return Err(RateLimitError::TooManyRequests);
        }
        
        Ok(())
    }
}
```

#### 3.5.2 团队信号量

**状态**: ❌ 未完成 (文件存在但为空)

```rust
// presentation/middleware/team_semaphore.rs
// TODO: Implement Team Semaphore logic
//
// use redis::AsyncCommands;
//
// pub struct TeamSemaphore {
//     redis: ConnectionManager,
// }
// ...
```

---

### 3.6 Webhook 投递

#### 3.6.1 Outbox 事件表

```rust
// infrastructure/database/entities/webhook_event.rs
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "webhook_events")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub team_id: Uuid,
    pub event_type: String,
    pub payload: Json,
    pub webhook_url: String,
    pub status: String,  // pending/delivered/failed/dead
    pub retry_count: i32,
    pub max_retries: i32,
    pub next_retry_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
}
```

#### 3.6.2 投递 Worker

```rust
// workers/webhook_worker.rs
pub struct WebhookWorker {
    db: DatabaseConnection,
    client: reqwest::Client,
}

impl WebhookWorker {
    pub async fn run(&self) {
        loop {
            // 查询待投递事件
            let events = webhook_event::Entity::find()
                .filter(webhook_event::Column::Status.eq("pending"))
                .filter(webhook_event::Column::NextRetryAt.lte(Utc::now()))
                .limit(100)
                .all(&self.db)
                .await
                .unwrap_or_default();
            
            for event in events {
                self.deliver_event(event).await;
            }
            
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
    
    async fn deliver_event(&self, event: webhook_event::Model) {
        let signature = self.generate_hmac(&event.payload);
        
        let result = self.client
            .post(&event.webhook_url)
            .header("X-crawlrs-Signature", signature)
            .header("X-crawlrs-Event", &event.event_type)
            .json(&event.payload)
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        
        match result {
            Ok(resp) if resp.status().is_success() => {
                self.mark_delivered(event.id).await;
            }
            _ if event.retry_count < event.max_retries => {
                self.schedule_retry(event.id).await;
            }
            _ => {
                self.mark_dead(event.id).await;
            }
        }
    }
    
    fn generate_hmac(&self, payload: &Json) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        
        let mut mac = Hmac::<Sha256>::new_from_slice(SECRET_KEY.as_bytes()).unwrap();
        mac.update(payload.to_string().as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }
}
```

---

## 4. 数据库设计

### 4.1 核心表 Schema

```sql
-- 任务表
CREATE TABLE tasks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_type VARCHAR(20) NOT NULL CHECK (task_type IN ('scrape', 'crawl', 'extract')),
    status VARCHAR(20) NOT NULL CHECK (status IN ('queued', 'active', 'completed', 'failed', 'cancelled')),
    priority INT NOT NULL DEFAULT 0,
    team_id UUID NOT NULL,
    url VARCHAR(2048) NOT NULL,
    payload JSONB NOT NULL,
    retry_count INT NOT NULL DEFAULT 0,
    max_retries INT NOT NULL DEFAULT 3,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    lock_token UUID,
    lock_expires_at TIMESTAMPTZ,
    INDEX idx_status_priority (status, priority DESC),
    INDEX idx_team_id (team_id),
    INDEX idx_lock_expires (lock_expires_at) WHERE lock_token IS NOT NULL
);

-- 积压表
CREATE TABLE tasks_backlog (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id UUID NOT NULL REFERENCES tasks(id),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '1 hour',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    INDEX idx_expires (expires_at)
);

-- 爬取会话表
CREATE TABLE crawls (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL,
    root_url VARCHAR(2048) NOT NULL,
    status VARCHAR(20) NOT NULL,
    config JSONB NOT NULL,
    total_tasks INT NOT NULL DEFAULT 0,
    completed_tasks INT NOT NULL DEFAULT 0,
    failed_tasks INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    INDEX idx_team_status (team_id, status)
);

-- Webhook 事件表
CREATE TABLE webhook_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    payload JSONB NOT NULL,
    webhook_url VARCHAR(512) NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    retry_count INT NOT NULL DEFAULT 0,
    max_retries INT NOT NULL DEFAULT 5,
    next_retry_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at TIMESTAMPTZ,
    INDEX idx_status_retry (status, next_retry_at) WHERE status = 'pending'
);
```

### 4.2 索引策略
- **复合索引**: `(status, priority)` 支持队列出队查询
- **部分索引**: `WHERE lock_token IS NOT NULL` 减小索引大小
- **覆盖索引**: 包含常用查询的所有列，避免回表

---

## 5. 部署架构

### 5.1 容器化（Docker）

```dockerfile
# Dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/crawlrs /usr/local/bin/
EXPOSE 8080
CMD ["crawlrs"]
```

### 5.2 单机部署（docker-compose.yml）

```yaml
version: '3.8'

services:
  api:
    build: .
    ports:
      - "8080:8080"
    environment:
      - DATABASE_URL=postgres://user:password@postgres:5432/crawlrs
      - REDIS_URL=redis://redis:6379
    depends_on:
      - postgres
      - redis
  
  worker:
    build: .
    command: ["crawlrs", "worker"]
    environment:
      - DATABASE_URL=postgres://user:password@postgres:5432/crawlrs
      - REDIS_URL=redis://redis:6379
    depends_on:
      - postgres
      - redis
  
  postgres:
    image: postgres:16
    environment:
      POSTGRES_USER: user
      POSTGRES_PASSWORD: password
      POSTGRES_DB: crawlrs
    volumes:
      - postgres_data:/var/lib/postgresql/data
  
  redis:
    image: redis:7-alpine
    volumes:
      - redis_data:/data

volumes:
  postgres_data:
  redis_data:
```

### 5.3 集群部署（Kubernetes）

```yaml
# k8s/deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: crawlrs-api
spec:
  replicas: 3
  selector:
    matchLabels:
      app: crawlrs-api
  template:
    metadata:
      labels:
        app: crawlrs-api
    spec:
      containers:
      - name: api
        image: crawlrs:latest
        ports:
        - containerPort: 8080
        env:
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: crawlrs-secrets
              key: database-url
        resources:
          requests:
            memory: "256Mi"
            cpu: "500m"
          limits:
            memory: "1Gi"
            cpu: "2000m"
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
---
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: crawlrs-api-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: crawlrs-api
  minReplicas: 3
  maxReplicas: 20
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
```

---

## 6. 监控与可观测性

### 6.1 日志规范

```rust
// utils/telemetry.rs
use tracing::{info, warn, error, instrument};

#[instrument(skip(db))]
pub async fn create_task(db: &DatabaseConnection, task: Task) -> Result<Task, Error> {
    info!(task_id = %task.id, task_type = ?task.task_type, "Creating task");
    
    let result = task_repo.create(&task).await;
    
    match &result {
        Ok(_) => info!(task_id = %task.id, "Task created successfully"),
        Err(e) => error!(task_id = %task.id, error = %e, "Failed to create task"),
    }
    
    result
}
```

**日志格式**（JSON）：
```json
{
  "timestamp": "2024-12-10T10:30:45.123Z",
  "level": "INFO",
  "target": "crawlrs::domain::services",
  "message": "Task created successfully",
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "task_type": "scrape"
}
```

### 6.2 指标采集（Prometheus）

```rust
// utils/metrics.rs
use prometheus::{Registry, Counter, Histogram, opts};

lazy_static! {
    pub static ref TASK_CREATED: Counter = Counter::new(
        "crawlrs_tasks_created_total",
        "Total number of tasks created"
    ).unwrap();
    
    pub static ref SCRAPE_DURATION: Histogram = Histogram::with_opts(
        opts!(
            "crawlrs_scrape_duration_seconds",
            "Scrape request duration in seconds"
        ).buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0])
    ).unwrap();
}
```

**暴露端点**：
```rust
// presentation/routes/metrics.rs
use axum::{response::IntoResponse, routing::get, Router};
use prometheus::{Encoder, TextEncoder};

pub fn metrics_routes() -> Router {
    Router::new().route("/metrics", get(metrics_handler))
}

async fn metrics_handler() -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}
```

---

## 7. 安全设计

### 7.1 SSRF 防护

```rust
// utils/validators.rs
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub fn is_safe_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            !ipv4.is_loopback() &&
            !ipv4.is_private() &&
            !ipv4.is_link_local() &&
            !ipv4.is_documentation() &&
            !ipv4.is_broadcast()
        }
        IpAddr::V6(ipv6) => {
            !ipv6.is_loopback() &&
            !ipv6.is_unspecified()
        }
    }
}

pub async fn validate_url(url: &str) -> Result<(), ValidationError> {
    let parsed = Url::parse(url)?;
    
    // 解析域名到 IP
    let addrs = tokio::net::lookup_host(parsed.host_str().unwrap())
        .await?
        .collect::<Vec<_>>();
    
    // 检查所有 IP
    for addr in addrs {
        if !is_safe_ip(addr.ip()) {
            return Err(ValidationError::SsrfDetected);
        }
    }
    
    Ok(())
}
```

### 7.2 Robots.txt 解析

```rust
// engines/robots_parser.rs
use texting_robots::{Robot, get_robots_url};

pub struct RobotsCache {
    cache: Arc<RwLock<HashMap<String, Robot>>>,
    redis: ConnectionManager,
}

impl RobotsCache {
    pub async fn is_allowed(&self, url: &str, user_agent: &str) -> Result<bool, Error> {
        let domain = extract_domain(url)?;
        
        // 先查 Redis
        if let Some(robot) = self.get_cached(&domain).await? {
            return Ok(robot.allowed(url));
        }
        
        // 下载并解析
        let robots_url = get_robots_url(url)?;
        let content = reqwest::get(&robots_url).await?.text().await?;
        let robot = Robot::new(user_agent, content.as_bytes())?;
        
        // 缓存
        self.cache_robots(&domain, &robot).await?;
        
        Ok(robot.allowed(url))
    }
}
```

---

## 8. 性能优化

### 8.1 连接池配置

```rust
// infrastructure/database/connection.rs
use sea_orm::{Database, ConnectOptions};

pub async fn create_pool(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    let mut opt = ConnectOptions::new(database_url.to_owned());
    opt.max_connections(100)
        .min_connections(10)
        .connect_timeout(Duration::from_secs(10))
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(3600))
        .sqlx_logging(true);
    
    Database::connect(opt).await
}
```

### 8.2 批量操作优化

```rust
// domain/repositories/task_repository.rs
impl TaskRepository for TaskRepositoryImpl {
    async fn batch_create(&self, tasks: Vec<Task>) -> Result<Vec<Task>, RepositoryError> {
        // 使用事务批量插入
        let txn = self.db.begin().await?;
        
        let models: Vec<_> = tasks.into_iter()
            .map(|t| task_entity::ActiveModel {
                id: Set(t.id),
                task_type: Set(t.task_type.to_string()),
                // ... 其他字段
            })
            .collect();
        
        // 批量插入（单条 SQL）
        let result = task_entity::Entity::insert_many(models)
            .exec(&txn)
            .await?;
        
        txn.commit().await?;
        Ok(result)
    }
}
```

---

## 9. 错误处理

### 9.1 错误类型定义

```rust
// utils/errors.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),
    
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    
    #[error("Team semaphore exhausted")]
    SemaphoreExhausted,
    
    #[error("Invalid state transition")]
    InvalidStateTransition,
    
    #[error("SSRF detected")]
    SsrfDetected,
    
    #[error("All engines failed")]
    AllEnginesFailed,
}

// Axum 错误转换
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::RateLimitExceeded => (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded"),
            AppError::SemaphoreExhausted => (StatusCode::SERVICE_UNAVAILABLE, "Too many concurrent tasks"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
        };
        
        let body = Json(json!({
            "success": false,
            "error": message,
        }));
        
        (status, body).into_response()
    }
}
```

---

## 10. 测试策略

### 10.1 单元测试示例

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_task_state_transition() {
        let task = Task {
            status: TaskStatus::Queued,
            ..Default::default()
        };
        
        let task = task.start().unwrap();
        assert_eq!(task.status, TaskStatus::Active);
        
        let task = task.complete().unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
    }
    
    #[test]
    fn test_invalid_state_transition() {
        let task = Task {
            status: TaskStatus::Completed,
            ..Default::default()
        };
        
        assert!(task.start().is_err());
    }
}
```

### 10.2 集成测试框架

```rust
// tests/integration/helpers.rs
pub async fn setup_test_db() -> DatabaseConnection {
    let db = Database::connect("postgres://test:test@localhost/crawlrs_test").await.unwrap();
    // 运行迁移
    Migrator::up(&db, None).await.unwrap();
    db
}

pub async fn teardown_test_db(db: &DatabaseConnection) {
    Migrator::down(db, None).await.unwrap();
}
```

---

## 11. 变更记录

| 版本 | 日期 | 变更内容 | 作者 |
|------|------|---------|------|
| v2.0.0 | 2024-12-10 | Rust 重构初始版本 | 技术团队 |
    