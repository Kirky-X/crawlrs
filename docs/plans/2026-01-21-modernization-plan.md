# Crawlrs 项目现代化改造实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**目标：** 对 crawlrs 企业级网页数据采集平台进行全面现代化改造，包括 API 规范化、性能优化、架构升级和可观测性完善，深度重构，长期收益优先，使用纯 ORM 框架。

**架构方案：** 演进式六边形架构 + 领域驱动设计（DDD），保持现有分层架构（presentation → application → domain → infrastructure），引入现代 Rust 生态最佳实践，弃用 sqlx 原始查询，完全使用 Sea-ORM。

**技术栈：** Axum 0.8+、Sea-ORM 1.1、utoipa、Prometheus metrics、Redis 1.0、tracing

---

## 阶段概览

| 阶段 | 内容 | 预计时间 | 优先级 |
|------|------|----------|--------|
| Phase 1 | 基础设施优化（依赖、日志、指标、健康检查） | 1-2 周 | P0 |
| Phase 2 | API 规范化（统一响应、错误处理、OpenAPI 文档） | 2-3 周 | P0 |
| Phase 3 | 架构重构（模块化、DDD 强化、事件驱动、DI 优化） | 3-4 周 | P1 |
| Phase 4 | 性能优化（连接池、缓存策略、批处理） | 2-3 周 | P1 |
| Phase 5 | 可观测性完善（tracing、metrics、日志） | 1-2 周 | P2 |

---

## 阶段 1：基础设施优化

### 任务 1.1：升级依赖版本

**文件：**
- 修改：`/home/dev/crawlrs/.worktrees/modernization/Cargo.toml`

**步骤 1：添加 utoipa 依赖**

```toml
# Cargo.toml 新增依赖
utoipa = "0.23"
utoipa-swagger-ui = "0.23"

# 修改 sea-orm 特征
sea-orm = { version = "1.1", default-features = false, features = ["runtime-tokio-rustls", "macros", "with-chrono", "with-uuid", "with-json"] }

# 更新 prometheus 依赖
metrics-exporter-prometheus = { version = "0.18.1", default-features = false, features = ["http-listener"] }
```

**步骤 2：运行 cargo update 更新锁文件**

```bash
cd /home/dev/crawlrs/.worktrees/modernization
cargo update
```

**步骤 3：验证编译**

```bash
cargo build --release 2>&1 | tail -20
```

**预期：** 编译成功，无重大警告。

---

### 任务 1.2：统一日志格式配置

**文件：**
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/common/tracing.rs`
- 修改：`/home/dev/crawlrs/.worktrees/modernization/src/bootstrap/services.rs`

**步骤 1：创建 tracing 配置模块**

```rust
// src/common/tracing.rs
use tracing_subscriber::{EnvFilter, fmt};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn init_tracing(service_name: &str, log_level: &str) {
    let env_filter = EnvFilter::new(log_level)
        .add_directive(tracing::Level::DEBUG.into());
    
    let format = fmt::layer()
        .with_timer(tracing_subscriber::fmt::time::iso8601())
        .with_threadNames(true)
        .with_line_number(true)
        .with_file(true);
    
    tracing_subscriber::registry()
        .with(env_filter)
        .with(format)
        .init();
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        tracing::info!($($arg)*)
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        tracing::error!($($arg)*)
    };
}
```

**步骤 2：在 bootstrap 中初始化**

```rust
// src/bootstrap/services.rs
use crate::common::tracing::init_tracing;

pub async fn init_tracing_service(config: &Config) {
    let log_level = config.log_level.as_str().unwrap_or("info");
    init_tracing("crawlrs", log_level);
}
```

**步骤 3：验证日志输出**

```bash
cargo run --bin crawlrs 2>&1 | head -50
```

**预期：** 日志以 JSON 格式输出，包含时间戳、线程名、行号等信息。

---

### 任务 1.3：配置 Prometheus 指标

**文件：**
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/common/metrics.rs`

**步骤 1：创建指标模块**

```rust
// src/common/metrics.rs
use std::sync::LazyLock;
use metrics::{Counter, Histogram, Gauge};

pub static TASK_COUNTER: LazyLock<Counter> = LazyLock::new(|| {
    Counter::new("crawlrs_tasks_total", "Total number of tasks created")
});

pub static TASK_DURATION: LazyLock<Histogram> = LazyLock::new(|| {
    Histogram::new_with_bounds(
        "crawlrs_task_duration_seconds",
        "Task execution duration in seconds",
        vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0],
    )
});

pub static CACHE_HIT_COUNTER: LazyLock<Counter> = LazyLock::new(|| {
    Counter::new("crawlrs_cache_hits_total", "Total cache hits")
});

pub static CACHE_MISS_COUNTER: LazyLock<Counter> = LazyLock::new(|| {
    Counter::new("crawlrs_cache_misses_total", "Total cache misses")
});

pub static ACTIVE_REQUESTS: LazyLock<Gauge> = LazyLock::new(|| {
    Gauge::new("crawlrs_active_requests", "Number of active requests")
});
```

**步骤 2：在 handlers 中使用指标**

```rust
// 示例：在任务创建时增加指标
use crate::common::metrics::TASK_COUNTER;

pub async fn create_task(/* ... */) -> Result<..., AppError> {
    TASK_COUNTER.increment(1);
    // ...
}
```

**步骤 3：验证指标端点**

```bash
curl http://localhost:8899/metrics 2>/dev/null | grep crawlrs
```

**预期：** 返回 Prometheus 格式的指标数据。

---

### 任务 1.4：实现 Health Check 端点

**文件：**
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/presentation/handlers/health_handler.rs`
- 修改：`/home/dev/crawlrs/.worktrees/modernization/src/presentation/mod.rs`

**步骤 1：创建 Health Check Handler**

```rust
// src/presentation/handlers/health_handler.rs
use axum::{Json, Extension};
use serde::Serialize;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

#[derive(Serialize)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
    pub timestamp: String,
    pub checks: Vec<HealthCheck>,
}

#[derive(Serialize)]
pub struct HealthCheck {
    pub name: String,
    pub status: String,
    pub latency_ms: u64,
    pub message: Option<String>,
}

pub async fn health_check(
    Extension(db): Extension<Arc<DatabaseConnection>>,
) -> Json<HealthStatus> {
    let checks = vec![
        check_database(&db).await,
    ];
    
    let overall_status = checks
        .iter()
        .map(|c| c.status.clone())
        .max()
        .unwrap_or("healthy".to_string());
    
    Json(HealthStatus {
        status: overall_status,
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        checks,
    })
}

async fn check_database(db: &Arc<DatabaseConnection>) -> HealthCheck {
    let start = std::time::Instant::now();
    let result = db.ping().await;
    
    HealthCheck {
        name: "database".to_string(),
        status: match result {
            Ok(_) => "healthy".to_string(),
            Err(_) => "unhealthy".to_string(),
        },
        latency_ms: start.elapsed().as_millis() as u64,
        message: result.err().map(|e| e.to_string()),
    }
}
```

**步骤 2：注册路由**

```rust
// src/presentation/mod.rs
use crate::presentation::handlers::health_handler;

pub fn register_routes() -> Router {
    Router::new()
        .route("/health", get(health_handler::health_check))
        // ... 其他路由
}
```

**步骤 3：测试端点**

```bash
curl -s http://localhost:8899/health | jq .
```

**预期：** 返回 JSON 格式的健康检查结果。

---

## 阶段 2：API 规范化

### 任务 2.1：定义统一响应格式

**文件：**
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/common/response.rs`
- 修改：`/home/dev/crawlrs/.worktrees/modernization/src/common/mod.rs`

**步骤 1：创建响应结构**

```rust
// src/common/response.rs
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<ApiError>,
    pub meta: Option<PaginationMeta>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PaginationMeta {
    pub page: u32,
    pub per_page: u32,
    pub total: u64,
    pub total_pages: u32,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            meta: None,
        }
    }
    
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error Some(ApiError {
                code: code.into(),
                message: message.into(),
                details: None,
            }),
            meta: None,
        }
    }
    
    pub fn with_pagination(
        data: T,
        page: u32,
        per_page: u32,
        total: u64,
    ) -> Self {
        let total_pages = (total as f64 / per_page as f64).ceil() as u32;
        Self {
            success: true,
            data: Some(data),
            error: None,
            meta: Some(PaginationMeta {
                page,
                per_page,
                total,
                total_pages,
            }),
        }
    }
}
```

**步骤 2：导出模块**

```rust
// src/common/mod.rs
pub mod response;
pub use response::{ApiResponse, ApiError, PaginationMeta};
```

**步骤 3：测试响应格式**

```rust
// tests/unit/common/response_test.rs
#[test]
fn test_api_response_success() {
    let response = ApiResponse::success("test data");
    assert!(response.success);
    assert_eq!(response.data, Some("test data"));
    assert!(response.error.is_none());
}

#[test]
fn test_api_response_error() {
    let response = ApiResponse::<String>::error("NOT_FOUND", "Resource not found");
    assert!(!response.success);
    assert!(response.data.is_none());
    assert!(response.error.is_some());
    assert_eq!(response.error.unwrap().code, "NOT_FOUND");
}
```

---

### 任务 2.2：定义统一错误类型

**文件：**
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/common/error.rs`
- 修改：`/home/dev/crawlrs/.worktrees/modernization/src/common/mod.rs`

**步骤 1：创建错误类型**

```rust
// src/common/error.rs
use thiserror::Error;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use crate::common::response::ApiResponse;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Validation error: {message}")]
    Validation { message: String, details: Option<serde_json::Value> },
    
    #[error("Not found: {resource}")]
    NotFound { resource: String, id: String },
    
    #[error("Database error: {source}")]
    Database { source: sea_orm::DbErr },
    
    #[error("Cache error: {source}")]
    Cache { source: redis::RedisError },
    
    #[error("Authentication required")]
    Unauthorized,
    
    #[error("Permission denied: {message}")]
    Forbidden { message: String },
    
    #[error("Rate limit exceeded, retry after {retry_after}s")]
    RateLimited { retry_after: u64 },
    
    #[error("Internal server error: {message}")]
    Internal { message: String },
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message, details) = match self {
            AppError::Validation { message, details } => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                message,
                details,
            ),
            AppError::NotFound { resource, id } => (
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                format!("{} not found: {}", resource, id),
                None,
            ),
            AppError::Database { source } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                "Internal database error".to_string(),
                if cfg!(debug_assertions) {
                    Some(json!({ "detail": format!("{}", source) }))
                } else {
                    None
                },
            ),
            AppError::Cache { source } => (
                StatusCode::SERVICE_UNAVAILABLE,
                "CACHE_ERROR",
                "Cache service unavailable".to_string(),
                None,
            ),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Authentication required".to_string(),
                None,
            ),
            AppError::Forbidden { message } => (
                StatusCode::FORBIDDEN,
                "FORBIDDEN",
                message,
                None,
            ),
            AppError::RateLimited { retry_after } => (
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMITED",
                format!("Rate limit exceeded, retry after {}s", retry_after),
                None,
            ),
            AppError::Internal { message } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                message,
                None,
            ),
        };
        
        (status, Json(ApiResponse::error(code, message))).into_response()
    }
}
```

---

### 任务 2.3：集成 utoipa 自动生成 OpenAPI 文档

**文件：**
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/presentation/mod.rs`（OpenAPI 配置）
- 修改：`/home/dev/crawlrs/.worktrees/modernization/src/presentation/handlers/task_handler.rs`

**步骤 1：创建 OpenAPI 配置**

```rust
// src/presentation/mod.rs
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::presentation::handlers::task_handler::create_task,
        crate::presentation::handlers::task_handler::get_task,
        crate::presentation::handlers::task_handler::list_tasks,
        crate::presentation::handlers::health_handler::health_check,
    ),
    components(
        schemas(
            crate::application::dto::task_dto::CreateTaskRequest,
            crate::application::dto::task_dto::TaskResponse,
            crate::common::response::ApiResponse,
            crate::common::error::AppError,
        )
    ),
    tags(
        (name = "Tasks", description = "Task management endpoints"),
        (name = "Health", description = "Health check endpoints"),
    )
)]
pub struct ApiDoc;
```

**步骤 2：为 Handler 添加注解**

```rust
// src/presentation/handlers/task_handler.rs
use utoipa::OpenApi;
use utoipa::path;

/// 创建任务
///
/// 创建新的爬虫任务，支持配置 URL、提取规则等参数。
#[utoipa::path(
    post,
    path = "/api/v1/tasks",
    tag = "Tasks",
    request_body = CreateTaskRequest,
    responses(
        (status = 201, description = "Task created successfully", body = ApiResponse<TaskResponse>),
        (status = 400, description = "Invalid request", body = ApiResponse<ApiError>),
        (status = 401, description = "Unauthorized", body = ApiResponse<ApiError>),
    )
)]
pub async fn create_task(
    // handler implementation
) -> Result<Json<ApiResponse<TaskResponse>>, AppError> {
    // ...
}
```

**步骤 3：添加 Swagger UI 路由**

```rust
// src/presentation/mod.rs
use utoipa_swagger_ui::SwaggerUi;

pub fn register_routes() -> Router {
    Router::new()
        .route("/health", get(health_handler::health_check))
        .route("/api-docs/openapi.json", get(openapi_json))
        .merge(SwaggerUi::new("/api-docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        // ... 其他路由
}
```

---

## 阶段 3：架构重构

### 任务 3.1：创建 DTO 模块

**文件：**
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/application/dto/mod.rs`
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/application/dto/task_dto.rs`
- 修改：`/home/dev/crawlrs/.worktrees/modernization/src/application/mod.rs`

**步骤 1：创建任务 DTO**

```rust
// src/application/dto/task_dto.rs
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Serialize, Deserialize, Debug, Validate)]
pub struct CreateTaskRequest {
    #[validate(length(min = 1, max = 255))]
    pub name: String,
    
    #[validate(url)]
    pub url: String,
    
    #[validate(length(max = 1000))]
    pub description: Option<String>,
    
    pub extract_rules: Option<ExtractRules>,
    
    pub options: Option<TaskOptions>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExtractRules {
    pub selectors: Vec<SelectorRule>,
    pub use_llm: bool,
    pub prompt: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SelectorRule {
    pub name: String,
    pub selector: String,
    pub attribute: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TaskOptions {
    pub timeout: Option<u64>,
    pub retries: Option<u32>,
    pub priority: Option<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TaskResponse {
    pub id: String,
    pub name: String,
    pub url: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}
```

**步骤 2：创建模块导出**

```rust
// src/application/dto/mod.rs
pub mod task_dto;
pub use task_dto::{CreateTaskRequest, TaskResponse, ExtractRules, SelectorRule, TaskOptions};
```

---

### 任务 3.2：创建 Use Case 模块

**文件：**
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/application/use_cases/mod.rs`
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/application/use_cases/create_task_use_case.rs`
- 修改：`/home/dev/crawlrs/.worktrees/modernization/src/application/mod.rs`

**步骤 1：创建 Use Case 接口**

```rust
// src/application/use_cases/create_task_use_case.rs
use async_trait::async_trait;
use crate::application::dto::task_dto::{CreateTaskRequest, TaskResponse};
use crate::domain::services::task_service::TaskService;
use crate::common::error::AppError;
use std::sync::Arc;

#[async_trait]
pub trait CreateTaskUseCase: Send + Sync {
    async fn execute(&self, request: CreateTaskRequest) -> Result<TaskResponse, AppError>;
}

pub struct CreateTaskUseCaseImpl {
    task_service: Arc<dyn TaskService>,
}

impl CreateTaskUseCaseImpl {
    pub fn new(task_service: Arc<dyn TaskService>) -> Self {
        Self { task_service }
    }
}

#[async_trait]
impl CreateTaskUseCase for CreateTaskUseCaseImpl {
    async fn execute(&self, request: CreateTaskRequest) -> Result<TaskResponse, AppError> {
        // 验证
        request.validate().map_err(|e| AppError::Validation {
            message: format!("Validation error: {}", e),
            details: None,
        })?;
        
        // 调用领域服务
        let task = self.task_service.create_task(request.into()).await?;
        
        Ok(task.into())
    }
}
```

---

### 任务 3.3：实现领域事件机制

**文件：**
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/domain/events/mod.rs`
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/domain/events/task_events.rs`
- 修改：`/home/dev/crawlrs/.worktrees/modernization/src/domain/mod.rs`

**步骤 1：定义领域事件**

```rust
// src/domain/events/task_events.rs
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskEvent {
    TaskCreated(TaskCreatedEvent),
    TaskStarted(TaskStartedEvent),
    TaskCompleted(TaskCompletedEvent),
    TaskFailed(TaskFailedEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCreatedEvent {
    pub task_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub url: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCompletedEvent {
    pub task_id: Uuid,
    pub duration_ms: u64,
    pub result_count: usize,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFailedEvent {
    pub task_id: Uuid,
    pub reason: String,
    pub retry_count: u32,
    pub failed_at: DateTime<Utc>,
}
```

**步骤 2：创建事件发布 trait**

```rust
// src/domain/events/mod.rs
use async_trait::async_trait;
use crate::domain::events::task_events::TaskEvent;

#[async_trait]
pub trait DomainEventPublisher: Send + Sync {
    async fn publish(&self, event: &TaskEvent);
    async fn publish_batch(&self, events: &[TaskEvent]);
}
```

---

## 阶段 4：性能优化

### 任务 4.1：优化 Sea-ORM 连接池配置

**文件：**
- 修改：`/home/dev/crawlrs/.worktrees/modernization/src/infrastructure/database/mod.rs`

**步骤 1：配置连接池**

```rust
// src/infrastructure/database/mod.rs
use sea_orm::ConnectOptions;
use std::time::Duration;

pub async fn create_database_connection(database_url: &str) -> sea_orm::DatabaseConnection {
    let max_connections = 50;
    let connect_timeout = Duration::from_secs(30);
    let idle_timeout = Duration::from_secs(600);
    
    let options = ConnectOptions::new(database_url)
        .max_connections(max_connections)
        .connect_timeout(connect_timeout)
        .idle_timeout(idle_timeout)
        .sqlx_logging(true)
        .sqlx_logging_level(log::LevelFilter::Debug);
    
    sea_orm::Database::connect(options)
        .await
        .expect("Failed to connect to database")
}
```

---

### 任务 4.2：实现多层缓存策略

**文件：**
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/infrastructure/cache/layered_cache.rs`
- 创建：`/home/dev/crawlrs/.worktrees/modernization/src/infrastructure/cache/mod.rs`

**步骤 1：实现缓存层**

```rust
// src/infrastructure/cache/layered_cache.rs
use dashmap::DashMap;
use redis::AsyncCommands;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

pub struct CacheEntry {
    value: String,
    expires_at: std::time::Instant,
}

pub struct LayeredCache {
    local_cache: Arc<DashMap<String, CacheEntry>>,
    redis_client: Arc<redis::Client>,
    local_cache_ttl: Duration,
}

impl LayeredCache {
    pub fn new(redis_url: &str, local_ttl_secs: u64) -> Self {
        let client = redis::Client::open(redis_url).expect("Invalid Redis URL");
        
        Self {
            local_cache: Arc::new(DashMap::new()),
            redis_client: Arc::new(client),
            local_cache_ttl: Duration::from_secs(local_ttl_secs),
        }
    }
    
    pub async fn get(&self, key: &str) -> Result<Option<String>, redis::RedisError> {
        // L1: Check local cache
        if let Some(entry) = self.local_cache.get(key) {
            if entry.expires_at > std::time::Instant::now() {
                return Ok(Some(entry.value.clone()));
            }
        }
        
        // L2: Check Redis
        let mut conn = self.redis_client.get_async_connection().await?;
        let value: Option<String> = conn.get(key).await?;
        
        if let Some(ref v) = value {
            // Backfill local cache
            self.local_cache.insert(
                key.to_string(),
                CacheEntry {
                    value: v.clone(),
                    expires_at: std::time::Instant::now() + self.local_cache_ttl,
                },
            );
        }
        
        Ok(value)
    }
    
    pub async fn set(&self, key: &str, value: &str, ttl: Duration) -> Result<(), redis::RedisError> {
        // Update Redis
        let mut conn = self.redis_client.get_async_connection().await?;
        conn.set_ex(key, value, ttl.as_secs()).await?;
        
        // Update local cache
        self.local_cache.insert(
            key.to_string(),
            CacheEntry {
                value: value.to_string(),
                expires_at: std::time::Instant::now() + ttl.min(self.local_cache_ttl),
            },
        );
        
        Ok(())
    }
    
    pub async fn invalidate(&self, key: &str) -> Result<(), redis::RedisError> {
        // Invalidate Redis
        let mut conn = self.redis_client.get_async_connection().await?;
        conn.del(key).await?;
        
        // Invalidate local cache
        self.local_cache.remove(key);
        
        Ok(())
    }
}
```

---

## 阶段 5：可观测性完善

### 任务 5.1：完善分布式追踪

**文件：**
- 修改：`/home/dev/crawlrs/.worktrees/modernization/src/presentation/handlers/task_handler.rs`

**步骤 1：添加 tracing instrumentation**

```rust
// src/presentation/handlers/task_handler.rs
use tracing::{info, warn, error, instrument};

#[instrument(
    skip(state, request),
    fields(task_name = %request.name, task_url = %request.url)
)]
pub async fn create_task(
    State(state): State<Arc<AppModule>>,
    Json(request): Json<CreateTaskRequest>,
) -> Result<Json<ApiResponse<TaskResponse>>, AppError> {
    info!("Creating new task: {} for URL: {}", request.name, request.url);
    
    let start = std::time::Instant::now();
    
    match state.create_task_use_case.execute(request).await {
        Ok(task) => {
            info!(
                task_id = %task.id,
                duration_ms = start.elapsed().as_millis(),
                "Task created successfully"
            );
            Ok(Json(ApiResponse::success(task)))
        }
        Err(e) => {
            error!(error = %e, "Failed to create task");
            Err(e)
        }
    }
}
```

---

## 执行摘要

本计划包含 5 个阶段、15+ 个细粒度任务，预计 8-12 周完成。

**计划已保存至：** `/home/dev/crawlrs/.worktrees/modernization/docs/plans/2026-01-21-modernization-plan.md`

**执行方式：**

**1. 子代理驱动（当前会话）** - 我为每个任务启动新的子代理，任务间进行代码审查，快速迭代

**2. 并行会话（新会话）** - 引导你在 worktree 中打开新会话，使用 executing-plans 批量执行

你希望采用哪种执行方式？
