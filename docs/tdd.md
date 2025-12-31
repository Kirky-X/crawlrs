# crawlrs - 技术设计文档 (TDD)

## 版本信息

- **文档版本**: v2.1.0
- **Rust 版本**: 1.75+ (Edition 2021)
- **最近更新**: 2024-12-20

---

## 1. 技术栈选型 ✅ 已验证

### 1.1 核心依赖

| 组件             | 技术选型           | 版本 | 选型理由                                              | 状态 |
| ---------------- | ------------------ | ---- | ----------------------------------------------------- | ---- |
| **Web 框架**     | Axum               | 0.8  | 基于 Tower，性能最优，类型安全                        | ✅ 已实现 |
| **ORM**          | SeaORM             | 1.0  | 异步、迁移管理、编译期类型检查                        | ✅ 已实现 |
| **HTTP 客户端**  | reqwest            | 0.12 | 生态成熟，支持连接池和 HTTP/2                         | ✅ 已实现 |
| **异步运行时**   | tokio              | 1.48 | 业界标准，生态完善                                    | ✅ 已实现 |
| **限流**         | governor           | 0.10 | 高性能令牌桶/漏桶限流实现                             | ✅ 已实现 |
| **序列化**       | serde              | 1.0  | Rust 生态标准                                         | ✅ 已实现 |
| **日志**         | tracing            | 0.1  | 结构化日志，与 tokio 深度集成                         | ✅ 已实现 |
| **错误处理**     | thiserror + anyhow | -    | 库用 thiserror，应用用 anyhow                         | ✅ 已实现 |
| **断路器**       | 自研 CircuitBreaker | -    | 支持失败阈值、恢复超时、半开状态                      | ✅ 已实现 |
| **健康检查**     | 自研 HealthMonitor  | -    | 定期检查引擎存活状态、响应时间监控                    | ✅ 已实现 |
| **配置管理**     | config             | 0.15 | 多环境配置支持                                        | ✅ 已实现 |
| **Redis 客户端** | redis              | 1.0  | 异步支持，连接池管理                                  | ✅ 已实现 |
| **对象存储**     | aws-sdk-s3         | 1.118.0 | 计划支持，已集成到依赖中但尚未完全实现      | ⚠️ 待完善 |
| **浏览器引擎**   | chromiumoxide      | 0.8  | Rust 原生 CDP 客户端，无头浏览器支持                  | ✅ 已实现 |
| **字符串相似度** | strsim             | 0.10 | Jaro-Winkler 算法，用于去重                           | ✅ 已实现 |
| **HTML 解析**    | scraper            | 0.25 | 基于 selectors 的 HTML 解析                           | ✅ 已实现 |

### 1.2 Cargo.toml（关键依赖） ✅ 已同步

```toml
[dependencies]
# Web 框架
axum = "0.8"
tower = { version = "0.5", features = ["util"] }
tower-http = { version = "0.6", features = ["trace"] }

# 数据库
sea-orm = { version = "1.0", default-features = false, features = ["sqlx-postgres", "sqlx-sqlite", "runtime-tokio-rustls", "macros", "with-chrono", "with-uuid", "with-json"] }
redis = { version = "1.0", features = ["tokio-comp"] }

# HTTP 客户端
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls-native-roots", "cookies", "http2", "charset", "macos-system-configuration"] }

# 异步运行时
tokio = { version = "1.48", features = ["full"] }

# 序列化
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# 日志与追踪
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# 错误处理
thiserror = "2.0.17"
anyhow = "1.0"

# 配置
config = "0.15.19"

# 工具库
uuid = { version = "1.19", features = ["v4", "serde"] }
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
│   │   â"‚   â"œâ"€â"€ search/                    # æœç´¢å¼•æ"Žæ¨¡å—
│   │   â"‚   â"‚   â"œâ"€â"€ mod.rs
│   │   â"‚   â"‚   â"œâ"€â"€ traits.rs              # SearchEngine Trait
│   │   â"‚   â"‚   â"œâ"€â"€ router.rs              # æœç´¢è·¯ç"±å™¨
│   │   â"‚   â"‚   â"œâ"€â"€ google_engine.rs
│   │   â"‚   â"‚   â"œâ"€â"€ bing_engine.rs
│   │   â"‚   â"‚   â"œâ"€â"€ baidu_engine.rs
│   │   â"‚   â"‚   â""â"€â"€ sogou_engine.rs
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

#### 3.1.1 Task 领域模型 ✅ 已实现

```rust
// domain/models/task.rs
pub struct Task {
    pub id: Uuid,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub priority: i32,
    pub team_id: Uuid,
    pub url: String,
    pub payload: serde_json::Value,
    pub attempt_count: i32,
    pub max_retries: i32,
    pub scheduled_at: Option<DateTime<FixedOffset>>,
    pub expires_at: Option<DateTime<FixedOffset>>,
    pub created_at: DateTime<FixedOffset>,
    pub started_at: Option<DateTime<FixedOffset>>,
    pub completed_at: Option<DateTime<FixedOffset>>,
    pub crawl_id: Option<Uuid>,
    pub updated_at: DateTime<FixedOffset>,
    pub lock_token: Option<Uuid>,
    pub lock_expires_at: Option<DateTime<FixedOffset>>,
}
```

**差异说明**: 
- 代码实现中使用了 `attempt_count` 而非文档中的 `retry_count`。
- 代码中增加了 `scheduled_at`, `expires_at`, `started_at`, `completed_at`, `crawl_id`, `updated_at`, `lock_token`, `lock_expires_at` 等更丰富的生命周期与并发控制字段。
- 时间类型使用了 `DateTime<FixedOffset>` 以支持时区。

#### 3.2.4 搜索缓存设计 ✅ 已实现

**状态**: ✅ 新增设计

```rust
// application/traits/search_cache.rs
use async_trait::async_trait;
use std::time::Duration;

#[async_trait]
pub trait SearchCache: Send + Sync {
    /// 获取缓存的搜索结果
    async fn get(&self, key: &str) -> Result<Option<SearchResult>, CacheError>;
    
    /// 设置搜索结果缓存
    async fn set(
        &self, 
        key: &str, 
        result: &SearchResult, 
        ttl: Duration
    ) -> Result<(), CacheError>;
    
    /// 删除缓存
    async fn delete(&self, key: &str) -> Result<(), CacheError>;
    
    /// 检查缓存是否存在
    async fn exists(&self, key: &str) -> Result<bool, CacheError>;
}

// infrastructure/cache/redis_search_cache.rs
use redis::{AsyncCommands, Client};
use serde_json;

pub struct RedisSearchCache {
    client: Client,
    prefix: String,
}

impl RedisSearchCache {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            prefix: "search:".to_string(),
        }
    }
    
    fn cache_key(&self, query: &str, engines: &[EngineType]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        query.hash(&mut hasher);
        engines.hash(&mut hasher);
        
        let hash = hasher.finish();
        format!("{}{:x}", self.prefix, hash)
    }
}

#[async_trait]
impl SearchCache for RedisSearchCache {
    async fn get(&self, key: &str) -> Result<Option<SearchResult>, CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let full_key = format!("{}{}", self.prefix, key);
        
        let value: Option<String> = conn.get(&full_key).await?;
        
        match value {
            Some(json_str) => {
                let result: SearchResult = serde_json::from_str(&json_str)?;
                Ok(Some(result))
            }
            None => Ok(None),
        }
    }
    
    async fn set(
        &self, 
        key: &str, 
        result: &SearchResult, 
        ttl: Duration
    ) -> Result<(), CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let full_key = format!("{}{}", self.prefix, key);
        
        let json_str = serde_json::to_string(result)?;
        conn.set_ex(&full_key, json_str, ttl.as_secs() as usize).await?;
        
        Ok(())
    }
    
    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let full_key = format!("{}{}", self.prefix, key);
        
        conn.del(&full_key).await?;
        Ok(())
    }
    
    async fn exists(&self, key: &str) -> Result<bool, CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let full_key = format!("{}{}", self.prefix, key);
        
        Ok(conn.exists(&full_key).await?)
    }
}
```

#### 3.2.5 同步等待机制 ✅ 已实现

**状态**: ✅ 新增设计

```rust
// application/services/sync_wait_service.rs
use tokio::time::{sleep, Duration};
use uuid::Uuid;

pub struct SyncWaitService<R: TaskRepository> {
    task_repo: Arc<R>,
    poll_interval: Duration,
    max_wait: Duration,
}

impl<R> SyncWaitService<R>
where R: TaskRepository + Send + Sync + 'static
{
    pub fn new(task_repo: Arc<R>, poll_interval: Duration, max_wait: Duration) -> Self {
        Self {
            task_repo,
            poll_interval,
            max_wait,
        }
    }
    
    /// 等待任务完成或超时
    pub async fn wait_for_completion(
        &self,
        task_id: Uuid,
        sync_wait_ms: u64,
    ) -> Result<WaitResult, WaitError> {
        let max_wait = Duration::from_millis(sync_wait_ms.min(30_000)); // 最大30秒
        let start = std::time::Instant::now();
        
        loop {
            // 获取任务状态
            match self.task_repo.find_by_id(task_id).await? {
                Some(task) => {
                    match task.status {
                        TaskStatus::Completed => {
                            return Ok(WaitResult::Completed(task));
                        }
                        TaskStatus::Failed => {
                            return Ok(WaitResult::Failed(task.error.unwrap_or_default()));
                        }
                        TaskStatus::Cancelled => {
                            return Ok(WaitResult::Cancelled);
                        }
                        TaskStatus::Queued | TaskStatus::Processing => {
                            // 继续等待
                        }
                    }
                }
                None => {
                    return Err(WaitError::TaskNotFound);
                }
            }
            
            // 检查是否超时
            if start.elapsed() >= max_wait {
                return Ok(WaitResult::Timeout(task_id));
            }
            
            // 等待下一轮轮询
            sleep(self.poll_interval).await;
        }
    }
}

#[derive(Debug)]
pub enum WaitResult {
    Completed(Task),
    Failed(String),
    Cancelled,
    Timeout(Uuid),
}

#[derive(Debug, thiserror::Error)]
pub enum WaitError {
    #[error("Task not found")]
    TaskNotFound,
    
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),
}
```

#### 3.2.2 搜索引擎 Trait 定义 ✅ 已实现

**状态**: ✅ 新增设计

```rust
// application/traits/search_engine.rs
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EngineType {
    Google,
    Bing,
    Baidu,
    Sogou,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchItem {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source_engine: EngineType,
    pub relevance_score: f64,
    pub published_date: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub items: Vec<SearchItem>,
    pub total: usize,
    pub engines_used: Vec<EngineType>,
    pub cache_hit: bool,
}

#[async_trait]
pub trait SearchEngine: Send + Sync {
    /// 执行搜索查询
    async fn search(&self, query: &str) -> Result<Vec<SearchItem>, SearchError>;
    
    /// 获取引擎类型
    fn engine_type(&self) -> EngineType;
    
    /// 获取引擎超时时间
    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }
    
    /// 是否支持高级搜索
    fn supports_advanced(&self) -> bool {
        false
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("Network error: {0}")]
    Network(String),
    
    #[error("Rate limit exceeded")]
    RateLimit,
    
    #[error("Invalid query")]
    InvalidQuery,
    
    #[error("Engine unavailable")]
    Unavailable,
    
    #[error("Timeout after {0}ms")]
    Timeout(u64),
}
```

#### 3.2.3 结果去重算法 ✅ 已实现

**状态**: ✅ 新增设计

```rust
// application/services/result_deduplicator.rs
use strsim::jaro_winkler;

pub struct ResultDeduplicator {
    /// URL 去重阈值（完全匹配）
    url_threshold: f64,
    /// 标题相似度阈值
    title_threshold: f64,
}

impl ResultDeduplicator {
    pub fn new() -> Self {
        Self {
            url_threshold: 1.0,  // 完全匹配
            title_threshold: 0.85,
        }
    }
    
    /// 去重搜索结果
    pub fn deduplicate(&self, items: Vec<SearchItem>) -> Vec<SearchItem> {
        let mut unique_items = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();
        
        for item in items {
            // 1. URL 完全去重
            if seen_urls.contains(&item.url) {
                continue;
            }
            
            // 2. 标题相似度去重
            let is_duplicate = unique_items.iter().any(|existing: &SearchItem| {
                self.is_title_similar(&existing.title, &item.title)
            });
            
            if !is_duplicate {
                seen_urls.insert(item.url.clone());
                unique_items.push(item);
            }
        }
        
        unique_items
    }
    
    /// 计算标题相似度
    fn is_title_similar(&self, title1: &str, title2: &str) -> bool {
        let similarity = jaro_winkler(title1, title2);
        similarity >= self.title_threshold
    }
    
    /// 按相关性排序
    pub fn sort_by_relevance(&self, mut items: Vec<SearchItem>) -> Vec<SearchItem> {
        items.sort_by(|a, b| {
            // 1. 相关性分数降序
            b.relevance_score.partial_cmp(&a.relevance_score).unwrap()
                // 2. 发布时间降序（如果有）
                .then_with(|| {
                    match (b.published_date, a.published_date) {
                        (Some(b_date), Some(a_date)) => b_date.cmp(&a_date),
                        (Some(_), None) => std::cmp::Ordering::Greater,
                        (None, Some(_)) => std::cmp::Ordering::Less,
                        (None, None) => std::cmp::Ordering::Equal,
                    }
                })
        });
        items
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

#### 3.2.6 访问控制逻辑 ✅ 已实现

**状态**: ✅ 新增设计

```rust
// domain/services/team_service.rs
use crate::engines::validators::validate_domain_blacklist;

impl TeamService {
    /// 验证任务是否符合团队访问控制规则
    pub async fn validate_task(&self, team_id: Uuid, url: &str) -> Result<(), ValidationError> {
        let team_restrictions = self.repo.get_restrictions(team_id).await?;
        
        // 1. 地理位置限制 (GeoIP)
        if let Some(allowed_countries) = &team_restrictions.allowed_countries {
            let ip = resolve_dns(url).await?;
            let country = self.geoip_service.lookup(ip)?;
            if !allowed_countries.contains(&country) {
                return Err(ValidationError::GeoRestricted(country));
            }
        }
        
        // 2. 域名黑名单
        if let Some(blacklist) = &team_restrictions.domain_blacklist {
            validate_domain_blacklist(url, blacklist)?;
        }
        
        // 3. 域名白名单
        if let Some(whitelist) = &team_restrictions.domain_whitelist {
            // ... whitelist logic
        }
        
        Ok(())
    }
}
```

#### 3.2.1 搜索引擎聚合服务 ✅

**状态**: ✅ 新增设计

```rust
// application/services/search_aggregator.rs
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use futures::future::join_all;

pub struct SearchAggregator<S: SearchEngine + Send + Sync> {
    engines: Vec<Arc<S>>,
    cache: Arc<dyn SearchCache>,
    circuit_breaker: Arc<CircuitBreaker>,
    config: SearchConfig,
}

impl<S> SearchAggregator<S> 
where S: SearchEngine + Send + Sync + 'static
{
    pub async fn aggregate_search(
        &self,
        query: &str,
        engines: &[EngineType],
        sync_wait_ms: u64,
    ) -> Result<SearchResult, SearchError> {
        // 1. 缓存检查
        let cache_key = self.cache_key(query, engines);
        if let Some(cached) = self.cache.get(&cache_key).await? {
            return Ok(cached);
        }
        
        // 2. 并发查询所有引擎
        let futures = engines.iter()
            .filter(|engine| self.circuit_breaker.is_closed(engine))
            .map(|engine| self.query_engine(engine, query))
            .collect::<Vec<_>>();
        
        // 3. 设置超时
        let timeout_duration = Duration::from_millis(sync_wait_ms);
        let results = timeout(timeout_duration, join_all(futures)).await
            .map_err(|_| SearchError::Timeout)?;
        
        // 4. 聚合结果
        let aggregated = self.merge_results(results);
        
        // 5. 缓存结果
        self.cache.set(cache_key, &aggregated, Duration::from_secs(3600)).await?;
        
        Ok(aggregated)
    }
    
    async fn query_engine(
        &self,
        engine_type: &EngineType,
        query: &str,
    ) -> Result<Vec<SearchItem>, SearchError> {
        let engine = self.get_engine(engine_type);
        let result = engine.search(query).await;
        
        // 更新断路器状态
        match &result {
            Ok(_) => self.circuit_breaker.record_success(engine_type),
            Err(_) => self.circuit_breaker.record_failure(engine_type),
        }
        
        result
    }
    
    fn merge_results(&self, results: Vec<Result<Vec<SearchItem>, SearchError>>) -> SearchResult {
        let mut all_items = Vec::new();
        let mut engines_used = Vec::new();
        
        for (idx, result) in results.into_iter().enumerate() {
            match result {
                Ok(items) => {
                    all_items.extend(items);
                    engines_used.push(EngineType::from_index(idx));
                }
                Err(_) => continue, // 忽略失败的引擎
            }
        }
        
        // 去重和排序
        let deduped = self.deduplicate(all_items);
        let sorted = self.sort_by_relevance(deduped);
        
        SearchResult {
            items: sorted,
            total: sorted.len(),
            engines_used,
            cache_hit: false,
        }
    }
}
```

---

### 3.3 基础设施层（Infrastructure）

#### 3.3.1 SeaORM 实体定义 ✅ 已实现

```rust
// infrastructure/database/entities/task.rs
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tasks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub task_type: String,
    pub status: String,
    pub priority: i32,
    pub team_id: Uuid,
    pub url: String,
    pub payload: Json,
    pub attempt_count: i32,
    pub max_retries: i32,
    pub scheduled_at: Option<ChronoDateTimeWithTimeZone>,
    pub expires_at: Option<ChronoDateTimeWithTimeZone>,
    pub created_at: ChronoDateTimeWithTimeZone,
    pub started_at: Option<ChronoDateTimeWithTimeZone>,
    pub completed_at: Option<ChronoDateTimeWithTimeZone>,
    pub crawl_id: Option<Uuid>,
    pub updated_at: ChronoDateTimeWithTimeZone,
    pub lock_token: Option<Uuid>,
    pub lock_expires_at: Option<ChronoDateTimeWithTimeZone>,
}

// infrastructure/database/entities/credits.rs
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "credits")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_id: Uuid,
    pub balance: i64,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

// infrastructure/database/entities/credits_transactions.rs
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "credits_transactions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_id: Uuid,
    pub amount: i64,
    pub transaction_type: String,
    pub description: String,
    pub reference_id: Option<Uuid>,
    pub created_at: DateTimeWithTimeZone,
}
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

#### 3.4.2 ReqwestEngine 引擎实现

```rust
// engines/reqwest_engine.rs
pub struct ReqwestEngine;

#[async_trait]
impl ScraperEngine for ReqwestEngine {
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

### 3.5 搜索引擎层（Search Engines） ✅

#### 3.5.1 搜索引擎 Trait 定义

```rust
// engines/search/traits.rs
use async_trait::async_trait;

#[async_trait]
pub trait SearchEngine: Send + Sync {
    /// 执行搜索
    async fn search(&self, query: &SearchQuery) -> Result<Vec, EngineError>;
    
    /// 引擎名称
    fn name(&self) -> &'static str;
    
    /// 计算对请求的支持分数（0-100）
    fn support_score(&self, query: &SearchQuery) -> u8 {
        100  // 默认全支持
    }
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub page: usize,
    pub limit: usize,
    pub lang: String,
    pub country: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
    pub source_engine: Option,
    pub relevance_score: f32,
    pub published_date: Option<DateTime>,
}
```

---

#### 3.5.2 Google 引擎实现（核心逻辑）

```rust
// engines/search/google_engine.rs
use std::sync::Arc;
use tokio::sync::RwLock;
use scraper::{Html, Selector};

pub struct GoogleSearchEngine {
    arc_id_cache: Arc<RwLock>,
    client: reqwest::Client,
}

struct ArcIdCache {
    arc_id: String,
    generated_at: i64,
}

impl GoogleSearchEngine {
    pub fn new() -> Self {
        Self {
            arc_id_cache: Arc::new(RwLock::new(ArcIdCache {
                arc_id: Self::generate_random_id(),
                generated_at: Utc::now().timestamp(),
            })),
            client: reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
        }
    }
    
    /// 生成 23 位随机 ARC_ID
    fn generate_random_id() -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
        
        let mut rng = rand::thread_rng();
        (0..23)
            .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
            .collect()
    }
    
    /// 获取 ARC_ID（每小时自动刷新）
    async fn get_arc_id(&self, start_offset: usize) -> String {
        let mut cache = self.arc_id_cache.write().await;
        let now = Utc::now().timestamp();
        
        // 超过 1 小时重新生成
        if now - cache.generated_at > 3600 {
            cache.arc_id = Self::generate_random_id();
            cache.generated_at = now;
            info!("Google ARC_ID refreshed: {}", cache.arc_id);
        }
        
        format!(
            "arc_id:srp_{}_1{:02},use_ac:true,_fmt:prog",
            cache.arc_id,
            start_offset
        )
    }
    
    /// 解析 Google HTML 结果
    fn parse_results(&self, html: &str) -> Result<Vec, EngineError> {
        let document = Html::parse_document(html);
        
        // Google 结果容器 selector（可能变化，需监控）
        let result_selector = Selector::parse("div[jscontroller='SC7lYd']")
            .map_err(|_| EngineError::ParseError)?;
        let title_selector = Selector::parse("h3").unwrap();
        let url_selector = Selector::parse("a[href]").unwrap();
        let content_selector = Selector::parse("div[data-sncf='1']").unwrap();
        
        let mut results = Vec::new();
        
        for element in document.select(&result_selector) {
            // 提取标题
            let title = element
                .select(&title_selector)
                .next()
                .map(|e| e.text().collect::())
                .unwrap_or_default();
            
            if title.is_empty() {
                continue;
            }
            
            // 提取 URL
            let url = element
                .select(&url_selector)
                .find_map(|e| e.value().attr("href"))
                .unwrap_or("")
                .to_string();
            
            // 提取摘要
            let content = element
                .select(&content_selector)
                .next()
                .map(|e| e.text().collect::())
                .unwrap_or_default();
            
            results.push(SearchResult {
                title,
                url,
                content,
                source_engine: Some("google".to_string()),
                relevance_score: 1.0,  // 后续可优化
                published_date: None,
            });
        }
        
        Ok(results)
    }
}

#[async_trait]
impl SearchEngine for GoogleSearchEngine {
    async fn search(&self, query: &SearchQuery) -> Result<Vec, EngineError> {
        let start = (query.page - 1) * query.limit;
        
        let params = [
            ("q", query.query.as_str()),
            ("hl", &format!("{}-{}", query.lang, query.country)),
            ("lr", &format!("lang_{}", query.lang)),
            ("start", &start.to_string()),
            ("num", &query.limit.to_string()),
            ("asearch", "arc"),
            ("async", &self.get_arc_id(start).await),
            ("filter", "0"),
            ("safe", "medium"),
        ];
        
        let response = self.client
            .get("https://www.google.com/search")
            .query(&params)
            .send()
            .await
            .map_err(|e| EngineError::NetworkError(e.to_string()))?;
        
        let html = response.text().await
            .map_err(|e| EngineError::NetworkError(e.to_string()))?;
        
        self.parse_results(&html)
    }
    
    fn name(&self) -> &'static str {
        "google"
    }
}
```

---

#### 3.5.3 搜索路由器（并发聚合）

```rust
// engines/search/router.rs
use futures::future::join_all;
use tokio::time::timeout;

pub struct SearchRouter {
    engines: HashMap>,
    config: SearchConfig,
    circuit_breaker: Arc,
    cache: Arc,
}

impl SearchRouter {
    /// 并发聚合搜索
    pub async fn search(&self, query: &SearchQuery) -> Result {
        // 1. 检查缓存
        let cache_key = self.cache.generate_key(query);
        if let Some(cached) = self.cache.get(&cache_key).await? {
            return Ok(cached);
        }
        
        // 2. 选择启用的引擎
        let active_engines: Vec = self.config.enabled_engines
            .iter()
            .filter_map(|name| self.engines.get(name))
            .filter(|e| !self.circuit_breaker.is_open(e.name()))
            .collect();
        
        if active_engines.is_empty() {
            return Err(SearchError::NoEnginesAvailable);
        }
        
        // 3. 并发查询
        let engine_timeout = Duration::from_millis(self.config.concurrent_timeout_ms);
        
        let search_futures = active_engines.iter().map(|engine| {
            let query = query.clone();
            async move {
                match timeout(engine_timeout, engine.search(&query)).await {
                    Ok(Ok(results)) => {
                        self.circuit_breaker.record_success(engine.name());
                        Some((engine.name(), results))
                    }
                    Ok(Err(e)) => {
                        warn!("Engine {} failed: {}", engine.name(), e);
                        self.circuit_breaker.record_failure(engine.name());
                        None
                    }
                    Err(_) => {
                        warn!("Engine {} timeout", engine.name());
                        self.circuit_breaker.record_failure(engine.name());
                        None
                    }
                }
            }
        });
        
        let results = join_all(search_futures).await;
        let successful_results: Vec = results.into_iter().flatten().collect();
        
        // 4. 检查最少成功引擎数
        if successful_results.len() < self.config.min_engines_success {
            return Err(SearchError::InsufficientEngines);
        }
        
        // 5. 去重 + 合并
        let merged = self.merge_and_deduplicate(successful_results)?;
        
        // 6. 缓存
        self.cache.set(&cache_key, &merged, Duration::from_secs(3600)).await?;
        
        Ok(merged)
    }
    
    /// 去重算法
    fn merge_and_deduplicate(
        &self,
        results: Vec)>,
    ) -> Result {
        let mut all_results = Vec::new();
        let mut seen_urls = HashSet::new();
        
        for (engine_name, engine_results) in results {
            for mut result in engine_results {
                // URL 完全去重
                if seen_urls.contains(&result.url) {
                    continue;
                }
                
                // 标题相似度去重
                let is_duplicate = all_results.iter().any(|existing: &SearchResult| {
                    strsim::jaro_winkler(&existing.title, &result.title) as f32
                        > self.config.dedup_threshold
                });
                
                if !is_duplicate {
                    result.source_engine = Some(engine_name.to_string());
                    all_results.push(result);
                    seen_urls.insert(result.url.clone());
                }
            }
        }
        
        Ok(SearchResponse {
            results: all_results,
            total: all_results.len(),
            engines_used: results.iter().map(|(n, _)| n.to_string()).collect(),
        })
    }
}
```

---

#### 3.5.4 搜索缓存实现

```rust
// infrastructure/cache/search_cache.rs
use sha2::{Sha256, Digest};

pub struct SearchCache {
    redis: ConnectionManager,
}

impl SearchCache {
    /// 生成缓存键
    pub fn generate_key(&self, query: &SearchQuery) -> String {
        let mut hasher = Sha256::new();
        hasher.update(query.query.as_bytes());
        hasher.update(query.engines.join(",").as_bytes());
        hasher.update(query.lang.as_bytes());
        hasher.update(query.limit.to_string().as_bytes());
        
        format!("search:v1:{}", hex::encode(hasher.finalize()))
    }
    
    pub async fn get(&self, key: &str) -> Result<Option> {
        let cached: Option = self.redis.get(key).await?;
        
        if let Some(json) = cached {
            Ok(Some(serde_json::from_str(&json)?))
        } else {
            Ok(None)
        }
    }
    
    pub async fn set(&self, key: &str, response: &SearchResponse, ttl: Duration) -> Result {
        let json = serde_json::to_string(response)?;
        self.redis.set_ex(key, json, ttl.as_secs() as usize).await?;
        Ok(())
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

**状态**: ✅ 已实现

```rust
// presentation/middleware/team_semaphore.rs
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct TeamSemaphore {
    semaphores: Arc<DashMap<Uuid, Arc<Semaphore>>>,
    default_permits: usize,
}

impl TeamSemaphore {
    pub fn new(default_permits: usize) -> Self {
        Self {
            semaphores: Arc::new(DashMap::new()),
            default_permits,
        }
    }

    pub async fn acquire(&self, team_id: Uuid) -> OwnedSemaphorePermit {
        self.get_or_create(team_id).acquire_owned().await.unwrap()
    }
}```

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

-- æœç´¢åŽ†å²è¡¨
CREATE TABLE search_queries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL,
    query TEXT NOT NULL,
    engines_used TEXT[] NOT NULL,
    results_count INT NOT NULL DEFAULT 0,
    cache_hit BOOLEAN NOT NULL DEFAULT false,
    response_time_ms INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    INDEX idx_team_created (team_id, created_at DESC),
    INDEX idx_query_hash (MD5(query))
);

-- ä»»åŠ¡è¡¨å¢žå¼ºï¼ˆæ"¯æŒæœç´¢ä»»åŠ¡ï¼‰
ALTER TABLE tasks 
    DROP CONSTRAINT IF EXISTS tasks_task_type_check;

ALTER TABLE tasks 
    ADD CONSTRAINT tasks_task_type_check 
    CHECK (task_type IN ('scrape', 'crawl', 'extract', 'search'));

ALTER TABLE tasks 
    ADD COLUMN search_metadata JSONB;
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
      - SEARCH_ENABLED_ENGINES=google,bing  # æ–°å¢žé…ç½®
      - SEARCH_CACHE_ENABLED=true
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

#### 6.2.1 核心指标定义

```rust
// infrastructure/metrics/mod.rs
use prometheus::{
    register_counter, register_gauge, register_histogram, register_histogram_vec,
    register_int_counter, register_int_gauge, Counter, Encoder, Gauge, Histogram,
    HistogramVec, IntCounter, IntGauge, TextEncoder,
};
use lazy_static::lazy_static;

lazy_static! {
    // 任务相关指标
    pub static ref TASK_CREATED_TOTAL: IntCounter = register_int_counter!(
        "crawlrs_tasks_created_total",
        "Total number of tasks created"
    ).unwrap();
    
    pub static ref TASK_COMPLETED_TOTAL: IntCounter = register_int_counter!(
        "crawlrs_tasks_completed_total",
        "Total number of tasks completed"
    ).unwrap();
    
    pub static ref TASK_FAILED_TOTAL: IntCounter = register_int_counter!(
        "crawlrs_tasks_failed_total",
        "Total number of failed tasks"
    ).unwrap();
    
    pub static ref TASK_DURATION_SECONDS: HistogramVec = register_histogram_vec!(
        "crawlrs_task_duration_seconds",
        "Task execution duration in seconds",
        &["task_type", "status"],
        vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0]
    ).unwrap();
    
    // HTTP请求指标
    pub static ref HTTP_REQUESTS_TOTAL: IntCounter = register_int_counter!(
        "crawlrs_http_requests_total",
        "Total number of HTTP requests"
    ).unwrap();
    
    pub static ref HTTP_REQUEST_DURATION_SECONDS: HistogramVec = register_histogram_vec!(
        "crawlrs_http_request_duration_seconds",
        "HTTP request duration in seconds",
        &["method", "endpoint", "status"],
        vec![0.01, 0.05, 0.1, 0.5, 1.0, 2.0, 5.0]
    ).unwrap();
    
    // 抓取相关指标
    pub static ref SCRAPE_REQUESTS_TOTAL: IntCounter = register_int_counter!(
        "crawlrs_scrape_requests_total",
        "Total number of scrape requests"
    ).unwrap();
    
    pub static ref SCRAPE_DURATION_SECONDS: HistogramVec = register_histogram_vec!(
        "crawlrs_scrape_duration_seconds",
        "Scrape request duration in seconds",
        &["engine", "status"],
        vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0]
    ).unwrap();
    
    pub static ref SCRAPE_CONTENT_SIZE_BYTES: HistogramVec = register_histogram_vec!(
        "crawlrs_scrape_content_size_bytes",
        "Size of scraped content in bytes",
        &["engine"],
        vec![1024.0, 10240.0, 102400.0, 1048576.0, 10485760.0]
    ).unwrap();
    
    // 搜索相关指标
    pub static ref SEARCH_REQUESTS_TOTAL: IntCounter = register_int_counter!(
        "crawlrs_search_requests_total",
        "Total number of search requests"
    ).unwrap();
    
    pub static ref SEARCH_RESULTS_TOTAL: HistogramVec = register_histogram_vec!(
        "crawlrs_search_results_total",
        "Number of search results returned",
        &["engine"],
        vec![1.0, 5.0, 10.0, 20.0, 50.0, 100.0]
    ).unwrap();
    
    pub static ref SEARCH_DURATION_SECONDS: HistogramVec = register_histogram_vec!(
        "crawlrs_search_duration_seconds",
        "Search request duration in seconds",
        &["engine"],
        vec![0.5, 1.0, 2.0, 5.0, 10.0]
    ).unwrap();
    
    // 系统资源指标
    pub static ref ACTIVE_CONNECTIONS: IntGauge = register_int_gauge!(
        "crawlrs_active_connections",
        "Number of active connections"
    ).unwrap();
    
    pub static ref DATABASE_CONNECTIONS_ACTIVE: IntGauge = register_int_gauge!(
        "crawlrs_database_connections_active",
        "Number of active database connections"
    ).unwrap();
    
    pub static ref REDIS_CONNECTIONS_ACTIVE: IntGauge = register_int_gauge!(
        "crawlrs_redis_connections_active",
        "Number of active Redis connections"
    ).unwrap();
    
    // 限流指标
    pub static ref RATE_LIMIT_HITS_TOTAL: IntCounter = register_int_counter!(
        "crawlrs_rate_limit_hits_total",
        "Total number of rate limit hits"
    ).unwrap();
    
    pub static ref CONCURRENT_REQUESTS: IntGauge = register_int_gauge!(
        "crawlrs_concurrent_requests",
        "Number of concurrent requests being processed"
    ).unwrap();
    
    // 缓存指标
    pub static ref CACHE_HITS_TOTAL: IntCounter = register_int_counter!(
        "crawlrs_cache_hits_total",
        "Total number of cache hits"
    ).unwrap();
    
    pub static ref CACHE_MISSES_TOTAL: IntCounter = register_int_counter!(
        "crawlrs_cache_misses_total",
        "Total number of cache misses"
    ).unwrap();
    
    pub static ref CACHE_EVICTIONS_TOTAL: IntCounter = register_int_counter!(
        "crawlrs_cache_evictions_total",
        "Total number of cache evictions"
    ).unwrap();
}
```

#### 6.2.2 指标使用示例

```rust
// application/services/task_service.rs
use crate::infrastructure::metrics::*;

impl TaskService {
    pub async fn create_task(&self, request: CreateTaskRequest) -> Result<Task, ServiceError> {
        // 记录任务创建指标
        TASK_CREATED_TOTAL.inc();
        
        let start = Instant::now();
        let result = self.task_repo.create(request).await;
        let duration = start.elapsed();
        
        match &result {
            Ok(task) => {
                // 记录成功任务耗时
                TASK_DURATION_SECONDS
                    .with_label_values(&["create", "success"])
                    .observe(duration.as_secs_f64());
                
                info!(task_id = %task.id, "Task created successfully");
                Ok(task)
            }
            Err(e) => {
                // 记录失败任务耗时
                TASK_DURATION_SECONDS
                    .with_label_values(&["create", "failure"])
                    .observe(duration.as_secs_f64());
                
                error!(error = %e, "Failed to create task");
                Err(e.into())
            }
        }
    }
}

// presentation/middleware/metrics_middleware.rs
use crate::infrastructure::metrics::*;

pub async fn metrics_middleware(
    request: Request,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    let start = Instant::now();
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    
    // 增加并发请求数
    CONCURRENT_REQUESTS.inc();
    
    let response = next.run(request).await;
    let duration = start.elapsed();
    let status = response.status().as_u16().to_string();
    
    // 记录HTTP请求指标
    HTTP_REQUESTS_TOTAL.inc();
    HTTP_REQUEST_DURATION_SECONDS
        .with_label_values(&[&method, &path, &status])
        .observe(duration.as_secs_f64());
    
    // 减少并发请求数
    CONCURRENT_REQUESTS.dec();
    
    Ok(response)
}
```

#### 6.2.3 监控Dashboard配置规范

**Grafana Dashboard配置**（`grafana/dashboards/crawlrs-overview.json`）：

```json
{
  "dashboard": {
    "title": "Crawlrs 系统概览",
    "tags": ["crawlrs", "production"],
    "timezone": "browser",
    "panels": [
      {
        "title": "请求速率",
        "type": "stat",
        "targets": [
          {
            "expr": "rate(crawlrs_http_requests_total[5m])",
            "legendFormat": "{{method}} {{status}}"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "unit": "reqps",
            "thresholds": {
              "steps": [
                {"color": "green", "value": null},
                {"color": "yellow", "value": 100},
                {"color": "red", "value": 500}
              ]
            }
          }
        }
      },
      {
        "title": "P99 响应时间",
        "type": "stat",
        "targets": [
          {
            "expr": "histogram_quantile(0.99, rate(crawlrs_http_request_duration_seconds_bucket[5m]))",
            "legendFormat": "P99"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "unit": "s",
            "thresholds": {
              "steps": [
                {"color": "green", "value": null},
                {"color": "yellow", "value": 1},
                {"color": "red", "value": 5}
              ]
            }
          }
        }
      },
      {
        "title": "任务成功率",
        "type": "stat",
        "targets": [
          {
            "expr": "rate(crawlrs_tasks_completed_total[5m]) / (rate(crawlrs_tasks_completed_total[5m]) + rate(crawlrs_tasks_failed_total[5m])) * 100",
            "legendFormat": "成功率"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "unit": "percent",
            "thresholds": {
              "steps": [
                {"color": "red", "value": null},
                {"color": "yellow", "value": 95},
                {"color": "green", "value": 99}
              ]
            }
          }
        }
      },
      {
        "title": "活跃连接数",
        "type": "graph",
        "targets": [
          {
            "expr": "crawlrs_active_connections",
            "legendFormat": "活跃连接"
          },
          {
            "expr": "crawlrs_database_connections_active",
            "legendFormat": "数据库连接"
          },
          {
            "expr": "crawlrs_redis_connections_active",
            "legendFormat": "Redis连接"
          }
        ]
      },
      {
        "title": "抓取引擎性能",
        "type": "graph",
        "targets": [
          {
            "expr": "histogram_quantile(0.95, rate(crawlrs_scrape_duration_seconds_bucket[5m]))",
            "legendFormat": "P95 - {{engine}}"
          }
        ]
      },
      {
        "title": "搜索性能",
        "type": "graph",
        "targets": [
          {
            "expr": "histogram_quantile(0.95, rate(crawlrs_search_duration_seconds_bucket[5m]))",
            "legendFormat": "P95 - {{engine}}"
          }
        ]
      },
      {
        "title": "限流触发",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(crawlrs_rate_limit_hits_total[5m])",
            "legendFormat": "限流触发速率"
          }
        ]
      },
      {
        "title": "缓存命中率",
        "type": "stat",
        "targets": [
          {
            "expr": "rate(crawlrs_cache_hits_total[5m]) / (rate(crawlrs_cache_hits_total[5m]) + rate(crawlrs_cache_misses_total[5m])) * 100",
            "legendFormat": "缓存命中率"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "unit": "percent",
            "thresholds": {
              "steps": [
                {"color": "red", "value": null},
                {"color": "yellow", "value": 80},
                {"color": "green", "value": 95}
              ]
            }
          }
        }
      }
    ]
  }
}
```

**Prometheus配置**（`prometheus/prometheus.yml`）：

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'crawlrs'
    static_configs:
      - targets: ['crawlrs:8080']
    metrics_path: '/metrics'
    scrape_interval: 10s
    scrape_timeout: 5s
    
  - job_name: 'crawlrs-cluster'
    static_configs:
      - targets: 
        - 'crawlrs-1:8080'
        - 'crawlrs-2:8080'
        - 'crawlrs-3:8080'
    metrics_path: '/metrics'
    scrape_interval: 10s
    
  - job_name: 'redis'
    static_configs:
      - targets: ['redis:6379']
    metrics_path: '/metrics'
    
  - job_name: 'postgres'
    static_configs:
      - targets: ['postgres:5432']
    metrics_path: '/metrics'

rule_files:
  - "alert_rules.yml"

alerting:
  alertmanagers:
    - static_configs:
        - targets:
          - alertmanager:9093
```

**告警规则**（`prometheus/alert_rules.yml`）：

```yaml
groups:
  - name: crawlrs_alerts
    rules:
      - alert: HighErrorRate
        expr: rate(crawlrs_tasks_failed_total[5m]) > 0.1
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "Crawlrs任务失败率过高"
          description: "任务失败率超过10%，当前值: {{ $value }}"
          
      - alert: HighResponseTime
        expr: histogram_quantile(0.99, rate(crawlrs_http_request_duration_seconds_bucket[5m])) > 5
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Crawlrs响应时间过长"
          description: "P99响应时间超过5秒，当前值: {{ $value }}s"
          
      - alert: HighRateLimitHits
        expr: rate(crawlrs_rate_limit_hits_total[5m]) > 10
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Crawlrs限流触发频繁"
          description: "限流触发速率超过10次/秒，当前值: {{ $value }}"
          
      - alert: LowCacheHitRate
        expr: rate(crawlrs_cache_hits_total[5m]) / (rate(crawlrs_cache_hits_total[5m]) + rate(crawlrs_cache_misses_total[5m])) < 0.8
        for: 10m
        labels:
          severity: info
        annotations:
          summary: "Crawlrs缓存命中率低"
          description: "缓存命中率低于80%，当前值: {{ $value }}"
          
      - alert: HighActiveConnections
        expr: crawlrs_active_connections > 1000
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Crawlrs活跃连接数过高"
          description: "活跃连接数超过1000，当前值: {{ $value }}"
          
      - alert: DatabaseConnectionsExhausted
        expr: crawlrs_database_connections_active > 90
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "Crawlrs数据库连接即将耗尽"
          description: "数据库连接数超过90，当前值: {{ $value }}"
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

### 7.3 地理访问限制 (Geographic Restrictions) ✅ 已实现

```rust
// domain/services/team_service.rs
use maxminddb::{geoip2, Reader};
use std::net::IpAddr;

pub struct TeamService {
    geoip_reader: Arc<Reader<Vec<u8>>>,
}

impl TeamService {
    /// 验证地理访问权限
    pub async fn validate_geographic_restriction(
        &self,
        team_id: Uuid,
        client_ip: IpAddr,
        geo_restriction_repo: &dyn GeoRestrictionRepository,
    ) -> Result<bool, ServiceError> {
        // 获取团队地理限制配置
        let restrictions = geo_restriction_repo
            .find_by_team_id(team_id)
            .await?;
        
        // 如果未启用限制，允许访问
        if !restrictions.enabled {
            return Ok(true);
        }
        
        // 检查 IP 白名单
        if self.is_ip_allowed(client_ip, &restrictions.allowed_ips) {
            return Ok(true);
        }
        
        // 获取地理位置
        let country = self.get_country_code(client_ip)?;
        
        // 检查国家代码
        Ok(restrictions.allowed_countries.contains(&country))
    }
    
    /// 检查 IP 是否在白名单中
    fn is_ip_allowed(&self, ip: IpAddr, allowed_ips: &[String]) -> bool {
        allowed_ips.iter().any(|allowed| {
            if let Ok(parsed_ip) = allowed.parse::<IpAddr>() {
                parsed_ip == ip
            } else if let Some((ip_part, prefix)) = allowed.split_once('/') {
                if let Ok(network_ip) = ip_part.parse::<IpAddr>() {
                    if let Ok(prefix_len) = prefix.parse::<u8>() {
                        return self.is_ip_in_cidr(ip, network_ip, prefix_len);
                    }
                }
            }
            false
        })
    }
}
```

### 7.4 团队白名单管理 (Team Whitelist) ✅ 已实现

```rust
// presentation/handlers/team_handler.rs
use axum::{extract::Extension, response::IntoResponse};
use std::sync::Arc;

pub async fn update_team_geo_restrictions<GR>(
    Extension(geo_restriction_repo): Extension<Arc<GR>>,
    Extension(team_id): Extension<Uuid>,
    Json(request): Json<UpdateTeamGeoRestrictionsRequest>,
) -> impl IntoResponse
where
    GR: GeoRestrictionRepository + 'static,
{
    // 验证 IP 和 CIDR 格式
    for ip in &request.allowed_ips {
        if !is_valid_ip_or_cidr(ip) {
            return Err(ApiError::BadRequest(format!("Invalid IP or CIDR: {}", ip)));
        }
    }
    
    // 验证国家代码格式
    for country in &request.allowed_countries {
        if country.len() != 2 || !country.chars().all(|c| c.is_ascii_uppercase()) {
            return Err(ApiError::BadRequest(format!("Invalid country code: {}", country)));
        }
    }
    
    // 更新配置
    let restrictions = GeoRestriction {
        team_id,
        allowed_countries: request.allowed_countries,
        allowed_ips: request.allowed_ips,
        enabled: request.enabled,
        updated_at: Utc::now(),
    };
    
    geo_restriction_repo.save(restrictions).await?;
    
    Ok(Json(json!({
        "success": true,
        "data": restrictions
    })))
}

/// 验证 IP 地址或 CIDR 格式
fn is_valid_ip_or_cidr(input: &str) -> bool {
    // 检查是否为有效 IP 地址
    if input.parse::<IpAddr>().is_ok() {
        return true;
    }
    
    // 检查是否为有效 CIDR
    if let Some((ip_part, prefix_part)) = input.split_once('/') {
        if let Ok(ip) = ip_part.parse::<IpAddr>() {
            if let Ok(prefix) = prefix_part.parse::<u8>() {
                let max_prefix = match ip {
                    IpAddr::V4(_) => 32,
                    IpAddr::V6(_) => 128,
                };
                return prefix <= max_prefix;
            }
        }
    }
    
    false
}
```

### 7.5 访问控制集成 (Access Control Integration) ✅ 已实现

```rust
// presentation/handlers/crawl_handler.rs
pub async fn crawl<GR>(
    Extension(team_service): Extension<Arc<TeamService>>,
    Extension(geo_restriction_repo): Extension<Arc<GR>>,
    Extension(team_id): Extension<Uuid>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<CrawlRequest>,
) -> impl IntoResponse
where
    GR: GeoRestrictionRepository + 'static,
{
    // 验证地理访问权限
    let client_ip = addr.ip();
    let has_access = team_service
        .validate_geographic_restriction(
            team_id,
            client_ip,
            geo_restriction_repo.as_ref(),
        )
        .await?;
    
    if !has_access {
        warn!(
            "Geographic access denied for team {} from IP {}",
            team_id, client_ip
        );
        return Err(ApiError::Forbidden("Geographic access denied".to_string()));
    }
    
    // 记录访问日志
    info!(
        "Geographic access granted for team {} from IP {} (country: {:?})",
        team_id,
        client_ip,
        team_service.get_country_code(client_ip).ok()
    );
    
    // 继续处理请求...
}
```
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

## 11. 架构实现状态总结

### ✅ DDD分层架构完整实现状态

基于代码交叉检查，TDD文档中定义的DDD分层架构已完整实现：

#### 表示层 (Presentation Layer) ✅

- **Axum Web框架**: 完整实现，提供RESTful API接口
- **中间件系统**: 实现了团队信号量、认证、错误处理等中间件
- **路由配置**: 完整的任务管理、指标收集、健康检查等端点
- **请求/响应处理**: 统一的错误响应格式和状态码处理

#### 应用层 (Application Layer) ✅

- **服务协调**: TaskService、WebhookService等应用服务完整实现
- **DTO转换**: 请求/响应数据的验证和转换逻辑
- **事务边界**: 通过服务方法定义清晰的事务边界
- **用例实现**: 搜索、抓取、爬取、提取等核心用例完整实现

#### 领域层 (Domain Layer) ✅

- **实体定义**: Task、WebhookEvent等核心实体完整定义
- **值对象**: 状态枚举、配置对象等值对象实现
- **领域服务**: 引擎选择、内容提取等领域服务逻辑
- **仓储接口**: 领域层定义的仓储接口清晰分离

#### 基础设施层 (Infrastructure Layer) ✅

- **SeaORM集成**: PostgreSQL数据库访问完整实现
- **Redis缓存**: 限流、机器人协议缓存等功能实现
- **外部API集成**: reqwest客户端集成，支持多种搜索引擎
- **消息队列**: Webhook事件队列和重试机制实现

### ✅ 核心功能实现状态

| 功能模块        | 实现状态 | 说明                                       |
| --------------- | -------- | ------------------------------------------ |
| 统一任务管理    | ✅        | PostgreSQL + Redis双存储，支持任务状态跟踪 |
| 并发控制        | ✅        | 团队级信号量实现，支持细粒度并发控制       |
| 限流策略        | ✅        | 基于Redis的令牌桶算法实现                  |
| 引擎选择与回退  | ✅        | 智能评分算法，支持多引擎故障转移           |
| Webhook可靠投递 | ✅        | 指数退避重试 + 死信队列机制                |
| SSRF安全防护    | ✅        | IP地址白名单验证 + 私有网络过滤            |
| Robots.txt合规  | ✅        | 解析、缓存、Crawl-delay支持                |
| 指标收集        | ✅        | Prometheus指标 + Grafana仪表板             |
| 错误处理        | ✅        | thiserror + anyhow统一错误处理             |
| 日志追踪        | ✅        | tracing + tracing-subscriber集成           |

### ✅ 技术栈验证状态

所有TDD文档中定义的技术栈组件均已正确实现：

- **Web框架**: Axum ✅ - 异步高性能Web框架
- **ORM**: SeaORM ✅ - 异步ORM，支持PostgreSQL
- **运行时**: Tokio ✅ - 异步运行时，支持高并发
- **HTTP客户端**: reqwest ✅ - 异步HTTP客户端
- **缓存**: Redis ✅ - 内存数据库，支持多种数据结构
- **序列化**: serde ✅ - 高性能序列化框架
- **验证**: validator ✅ - 输入验证框架
- **配置**: config ✅ - 分层配置管理
- **依赖注入**: 无 ✅ - Rust通过特质和泛型实现依赖管理
- **测试**: 内置测试框架 ✅ - 单元测试和集成测试支持

### ⚠️ 待完善项

1. **性能优化**: 数据库连接池配置可以进一步优化
2. **监控告警**: 需要完善业务级别的告警规则

---

## 12. 变更记录

| 版本   | 日期       | 变更内容                                                     | 作者     |
| ------ | ---------- | ------------------------------------------------------------ | -------- |
| v2.1.0 | 2024-12-20 | 新增搜索引擎模块架构设计、并发聚合逻辑、搜索缓存、同步等待机制、配置文件结构、数据库 Schema | 技术团队 |
| v2.0.0 | 2024-12-10 | Rust 重构初始版本                                            | 技术团队 |
| v2.0.1 | 2024-12-19 | 添加架构实现状态总结                                         | 技术团队 |