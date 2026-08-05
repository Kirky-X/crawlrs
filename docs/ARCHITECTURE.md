# System Design & Technical Architecture

<div align="center">

![Documentation](https://img.shields.io/badge/type-architecture-purple)
![Version](https://img.shields.io/badge/version-0.2.0-blue)
![License](https://img.shields.io/badge/license-Apache%202.0-green)

**Version:** 0.2.0 | **Last Updated:** 2025-07-21 | **Author:** Kirky.X

</div>

---

## Table of Contents

- [Overview](#overview)
- [Architectural Principles](#architectural-principles)
- [System Architecture](#system-architecture)
- [Layer Architecture](#layer-architecture)
- [Core Components](#core-components)
  - [Feature Gate Architecture](#feature-gate-architecture)
- [Data Flow](#data-flow)
- [Crawling Engines](#crawling-engines)
- [Queue System](#queue-system)
- [Caching Strategy](#caching-strategy)
- [Rate Limiting](#rate-limiting)
- [Security Model](#security-model)
- [Deployment Architecture](#deployment-architecture)
- [Scalability Considerations](#scalability-considerations)
- [Future Enhancements](#future-enhancements)

---

## Overview

crawlrs is built using **Domain-Driven Design (DDD)** principles with a clean, layered architecture. The system is designed for high performance, scalability, and maintainability.

### Key Design Goals

1. **Performance** - 3-5x higher throughput than Node.js alternatives
2. **Scalability** - Horizontal scaling capabilities via stateless workers
3. **Type Safety** - Leverage Rust's type system for compile-time safety
4. **Flexibility** - Trait-based architecture for engines and extensions
5. **Observability** - Built-in metrics and inklog 0.1 structured logging via the `log::` facade

### Technology Stack

| Component | Technology | Version |
|-----------|-----------|---------|
| Async Runtime | Tokio | 1.53 |
| Web Framework | Axum | 0.8 |
| ORM | Sea-ORM (via dbnexus) | 2.0.1 |
| HTTP Client | Reqwest | 0.13 |
| Database | PostgreSQL | 16+ |
| Cache | oxcache (in-memory) | 0.3 |
| Rate Limiting | limiteron | 0.2 |
| DI Framework | trait-kit | 0.3 |
| Config | confers | 0.4 |
| SDK Generator | sdforge | 0.4 |
| Logging | inklog | 0.1 |
| Browser Engine | chromiumoxide | 0.9 |
| HTML Parser | scraper | 0.27 |

---

## Architectural Principles

### 1. Separation of Concerns

Each layer has a specific responsibility:
- **Presentation** - HTTP request/response handling, middleware
- **Application** - Use cases and business workflows, DTO mapping
- **Domain** - Core business logic and entities, repository interfaces
- **Infrastructure** - External integrations, database, cache, security

### 2. Dependency Inversion

High-level modules depend on abstractions (traits), not concrete implementations:

```rust
// Domain defines interface
trait TaskRepository: Send + Sync {
    async fn create(&self, task: &Task) -> Result<Task>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>>;
    async fn acquire_next(&self, worker_id: Uuid) -> Result<Option<Task>>;
}

// Infrastructure provides implementation
struct TaskRepoImpl {
    db: Arc<DbPool>,
}

impl TaskRepository for TaskRepoImpl { ... }
```

### 3. Single Responsibility

Each component has one reason to change:
- `scrape_worker` - Only handles scrape task execution
- `webhook_worker` - Only handles webhook delivery
- `backlog_worker` - Only handles expired task cleanup
- `LimiteronService` - Only manages rate limits

### 4. Open/Closed Principle

System is open for extension, closed for modification:
- Add new scraping engines by implementing `ScraperEngine` trait
- Add new search engines by implementing `SearchEngine` trait
- Add new middleware by implementing `tower::Layer`

---

## System Architecture

```mermaid
flowchart TD
    subgraph Clients [Client Applications]
        C1[Web Apps]
        C2[Mobile Apps]
        C3[SDKs - sdforge]
        C4[CLI Tools]
    end

    subgraph API [API Gateway - Axum 0.8]
        subgraph Middleware [Middleware Stack]
            M1[Authentication - API Key / Bearer]
            M2[Rate Limiting - 3 variants]
            M3[Team Concurrency - Semaphore]
            M4[Security Headers, CORS, Logging]
        end

        subgraph Handlers [HTTP Handlers & Routes]
            H1[Scrape Handler]
            H2[Crawl Handler]
            H3[Search Handler]
            H4[Extract Handler]
            H5[Task Handler]
            H6[Webhook Handler]
            H7[Team Handler]
            H8[Audit Handler]
            H9[Metrics Handler]
        end
    end

    subgraph App [Application Layer]
        A1[CreateScrape Use Case]
        A2[Crawl Use Case]
        A3[DTOs]
    end

    subgraph Workers [Worker Pool]
        W1[Scrape Worker]
        W2[Webhook Worker]
        W3[Backlog Worker]
        W4[Expiration Worker]
        W5[Task State Machine]
        W6[Manager]
    end

    subgraph Domain [Domain Layer]
        D1[Models / Entities]
        D2[Services]
        D3[Repository Interfaces]
    end

    subgraph Engines [Engine Layer]
        E1[EngineClient - Public API]
        E2[EngineRouter]
        E3[ReqwestEngine]
        E4[PlaywrightEngine]
        E5[FlareSolverrEngine]
    end

    subgraph Infra [Infrastructure Layer]
        DB[(Postgres Database<br/>dbnexus 0.4)]
        RC[(oxcache 0.3<br/>in-memory)]
        EXT[External APIs]
        MON[inklog / Prometheus]
        SEC[SSRF / Geolocation / DNS]
    end

    Clients -->|"HTTPS/REST API"| API
    API --> App
    API --> Workers
    App --> Domain
    Workers --> Domain
    Domain --> DB
    Domain --> RC
    Domain --> SEC
    Domain --> EXT
    Domain --> MON
    App --> Engines
    Workers --> Engines
    Engines --> EXT
```

---

## Layer Architecture

### Presentation Layer

**Location:** `src/presentation/`

**Responsibilities:**
- HTTP request/response handling
- Request validation
- Response formatting
- Middleware implementation (auth, rate limiting, security)

**Components:**

```
presentation/
├── handlers/              # HTTP endpoint handlers
│   ├── scrape_handler.rs
│   ├── scrape_commands.rs
│   ├── scrape_queries.rs
│   ├── crawl_handler.rs
│   ├── crawl_commands.rs
│   ├── crawl_queries.rs
│   ├── search_handler.rs
│   ├── extract_handler.rs
│   ├── task_handler.rs
│   ├── task_commands.rs
│   ├── task_queries.rs
│   ├── webhook_handler.rs
│   ├── team_handler.rs
│   ├── audit_handler.rs
│   ├── metrics_handler.rs
│   ├── api_key_handler.rs
│   ├── response_builder.rs
│   └── mod.rs
├── routes/                # Route definitions
│   ├── mod.rs             # health, version, base routes
│   ├── handlers.rs        # Handler route registration
│   ├── scrape.rs
│   ├── crawl.rs
│   ├── extract.rs
│   └── task.rs
├── middleware/             # HTTP middleware
│   ├── auth_middleware.rs
│   ├── auth_bridge.rs
│   ├── auth_types.rs
│   ├── rate_limit_middleware.rs          # Basic rate limiting
│   ├── distributed_rate_limit_middleware.rs
│   ├── limiteron_rate_limit_middleware.rs
│   ├── security_headers_middleware.rs
│   ├── team_semaphore_middleware.rs
│   ├── scope_validation.rs
│   └── mod.rs
├── sdk/                   # sdforge SDK interface
│   ├── mod.rs
│   ├── mocks.rs
│   └── tests.rs
├── helpers/               # Shared helpers
│   ├── mod.rs
│   ├── rate_limit_helper.rs
│   └── ssrf/
│       ├── mod.rs
│       ├── error.rs
│       ├── redirect.rs
│       ├── static_validator.rs
│       └── types.rs
├── extractors/            # Request extractors
│   ├── mod.rs
│   └── app_deps.rs
├── errors.rs
├── state.rs
└── mod.rs
```

**SDK Interface Layer (sdforge 0.4):**

The presentation layer includes an SDK interface built on **sdforge 0.4**, which wraps domain services as HTTP endpoints via sdforge's `#[service_api]` macro. All SDK handlers extract authentication context from `AuthState` (populated by `auth_middleware`), never from the request body.

**Handler Flow:**

```rust
pub async fn create_scrape(
    Extension(queue): Extension<Arc<dyn TaskQueue>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(auth_state): Extension<AuthState>,
    Json(payload): Json<ScrapeRequestDto>,
) -> impl IntoResponse {
    // 1. Validate request
    // 2. Check rate limits
    // 3. Check SSRF protection
    // 4. Create task in database
    // 5. Enqueue for processing
    // 6. Return task ID
}
```

**Middleware Stack:**

1. **Authentication Middleware** - API key validation, Bearer token, team ID extraction
2. **Security Headers Middleware** - CSP, X-Content-Type-Options, X-Frame-Options
3. **CORS** - Configurable origins via `tower-http`
4. **Rate Limiting** - Three variants: basic (in-memory), distributed (Redis-backed), limiteron (PostgreSQL-backed)
5. **Team Semaphore Middleware** - Per-team concurrency control
6. **Scope Validation** - API key permission scope checking

---

### Application Layer

**Location:** `src/application/`

**Responsibilities:**
- Use case orchestration
- DTO definitions and transformations
- Business workflow coordination

**Components:**

```
application/
├── use_cases/             # Business use cases
│   ├── mod.rs
│   ├── create_scrape.rs
│   ├── crawl_use_case.rs
│   ├── crawl_link_processor.rs
│   └── crawl_state_machine.rs
└── dto/                   # Data Transfer Objects
    ├── mod.rs
    ├── scrape_request.rs
    ├── scrape_response.rs
    ├── crawl_request.rs
    ├── extract_request.rs
    ├── search_request.rs
    ├── task_query_request.rs
    ├── webhook_request.rs
    └── geo_restriction_request.rs
```

**Use Case Pattern:**

```rust
pub struct CreateScrapeUseCase<R, Q, C> {
    task_repo: Arc<R>,
    queue: Arc<Q>,
    cache: Arc<C>,
}

impl<R, Q, C> CreateScrapeUseCase<R, Q, C>
where
    R: TaskRepository + Send + Sync,
    Q: TaskQueue + Send + Sync,
    C: CacheClient + Send + Sync,
{
    pub async fn execute(&self, request: ScrapeRequestDto) -> Result<ScrapeResponseDto> {
        // 1. Validate request
        // 2. Check rate limits
        // 3. Check cache
        // 4. Create task
        // 5. Enqueue
        // 6. Return response
    }
}
```

---

### Domain Layer

**Location:** `src/domain/`

**Responsibilities:**
- Core business entities
- Business rules and validation
- Repository interfaces (contracts)
- Domain services

**Components:**

```
domain/
├── models/                 # Domain entities
│   ├── mod.rs
│   ├── builders.rs
│   ├── task_domain.rs
│   ├── task_model.rs
│   ├── crawl_model.rs
│   ├── scrape_result_model.rs
│   ├── search_result.rs
│   ├── team_model.rs
│   ├── webhook_model.rs
│   ├── credits_model.rs
│   └── validations.rs
├── repositories/           # Repository interfaces
│   ├── mod.rs
│   ├── task_repository.rs
│   ├── crawl_repository.rs
│   ├── scrape_result_repository.rs
│   ├── webhook_repository.rs
│   ├── webhook_event_repository.rs
│   ├── team_repository.rs
│   ├── credits_repository.rs
│   ├── geo_restriction_repository.rs
│   ├── audit_log_repository.rs
│   └── tasks_backlog_repository.rs
├── services/               # Domain services
│   ├── mod.rs
│   ├── rate_limiting_service.rs
│   ├── extraction_service.rs
│   ├── extraction_utils.rs
│   ├── team_service.rs
│   ├── team_semaphore.rs
│   ├── search_service.rs
│   ├── webhook_service.rs
│   ├── webhook_sender.rs
│   ├── webhook_event_builder.rs
│   ├── noop_webhook_service.rs
│   ├── audit_service.rs
│   ├── audit_log_builder.rs
│   ├── geo_location.rs
│   ├── llm_service.rs
│   ├── llm_provider_strategy.rs  # T006/R-sec-006: ProviderStrategy 策略模式（OllamaStrategy）
│   ├── llm/                       # LLM 子模块（prompt_builder, provider_adapter）
│   ├── content_extractor/         # 正文提取（trafilatura/dom_smoothie/css_rule/facade）
│   ├── markdown_service.rs
│   ├── relevance_scorer.rs
│   └── retry_handler.rs
├── auth/                   # Authentication models
│   ├── mod.rs
│   └── scope.rs
├── errors.rs
└── use_cases/              # Domain use cases
    ├── mod.rs
    └── create_webhook.rs
```

**Core Domain Entities:**

**Task Entity:**
```rust
pub struct Task {
    pub id: Uuid,
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub priority: i32,
    pub url: String,
    pub payload: Value,
    pub retry_count: i32,
    pub attempt_count: i32,
    pub max_retries: i32,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub crawl_id: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
    pub lock_token: Option<Uuid>,
    pub lock_expires_at: Option<DateTime<Utc>>,
}
```

**Task Types:**
- `Scrape` - Single page scrape
- `Crawl` - Multi-page crawl
- `Extract` - Data extraction

**Task Statuses:**
- `Queued` - Created, waiting for worker
- `Running` - Being processed by worker
- `Completed` - Successfully completed
- `Failed` - Failed with error
- `Cancelled` - Cancelled by user
- `Expired` - TTL exceeded

---

### Infrastructure Layer

**Location:** `src/infrastructure/`

**Responsibilities:**
- External service implementations
- Database access (via dbnexus + Sea-ORM)
- Cache management (oxcache)
- Security (SSRF validation, API key hashing)
- DNS resolution and caching
- Geolocation
- Metrics and observability

**Components:**

```
infrastructure/
├── database/               # Database layer
│   ├── mod.rs
│   ├── dbnexus_connection.rs
│   ├── query_monitor.rs
│   ├── transaction.rs
│   ├── entities/           # Sea-ORM entity models
│   │   ├── mod.rs
│   │   ├── api_key.rs
│   │   ├── crawl.rs
│   │   ├── credits.rs
│   │   ├── credits_transactions.rs
│   │   ├── geo_restriction_log.rs
│   │   ├── scrape_result.rs
│   │   ├── sea_orm_active_enums.rs
│   │   ├── task.rs
│   │   ├── tasks_backlog.rs
│   │   ├── team.rs
│   │   ├── webhook.rs
│   │   ├── webhook_event.rs
│   │   └── auth/
│   │       ├── mod.rs
│   │       ├── audit_log.rs
│   │       └── scope.rs
│   └── repositories/       # Repository implementations
│       ├── mod.rs
│       ├── macros.rs
│       ├── task_repo_impl.rs
│       ├── crawl_repo_impl.rs
│       ├── scrape_result_repo_impl.rs
│       ├── credits_repo_impl.rs
│       ├── webhook_repo_impl.rs
│       ├── webhook_event_repo_impl.rs
│       ├── database_geo_restriction_repo.rs
│       ├── audit_log_repo_impl.rs
│       └── tasks_backlog_repo_impl.rs
├── oxcache/                # Unified cache (oxcache-backed)
│   ├── mod.rs
│   └── cache_service.rs
├── security/               # Security implementations
│   ├── mod.rs
│   ├── api_key_hash.rs
│   ├── constant_time_compare.rs
│   ├── env_var_security.rs
│   ├── env_injection.rs
│   ├── env_validation.rs
│   └── secure_ip.rs
├── persistence/            # Domain <-> DB entity mappers
│   ├── mod.rs
│   └── mappers/
│       ├── mod.rs
│       ├── task_mapper.rs
│       ├── crawl_mapper.rs
│       ├── credits_mapper.rs
│       └── webhook_mapper.rs
├── services/               # Infrastructure services
│   ├── mod.rs
│   ├── config_service.rs
│   ├── limiteron_service.rs
│   ├── noop_rate_limiting_service.rs
│   └── webhook_sender_impl.rs
├── dns/                    # DNS resolution + caching
│   ├── mod.rs
│   ├── dns_cache.rs
│   └── ipv4_resolver.rs
├── observability/          # Observability
│   ├── mod.rs
│   └── metrics.rs
├── geolocation.rs
├── metrics.rs
├── errors.rs
└── mod.rs
```

**Database Layer:**

**Technology:** dbnexus 0.4 (builds on Sea-ORM 2.0.1)

dbnexus provides connection pooling, permission control, migration framework, metrics monitoring, and audit logging on top of Sea-ORM's type-safe database access. PostgreSQL is the only supported backend.

**Key Tables:**

| Table | Purpose |
|-------|---------|
| `tasks` | Task records with locking for worker coordination |
| `tasks_backlog` | Backlog of expired/pending tasks for reprocessing |
| `crawls` | Crawl configurations (depth, patterns) |
| `scrape_results` | Scrape results (raw HTML, markdown, metadata) |
| `api_keys` | ⚠️ PARTIALLY DEPRECATED (0.2.0): `key_hash`/`scopes` 列由 garrison 接管，仅保留 `id`/`team_id`/`key` 列供 `api_key_id→team_id` 反查映射（`deprecated_at` 标记） |
| `webhooks` | Webhook configurations (URL, events, retry) |
| `webhook_events` | Webhook event logs and delivery status |
| `audit_logs` | API access and event audit logs (garrison `audit-log` 也写入此表) |
| `team` | Team accounts and settings |
| `credits` | Team credit balances |
| `credits_transactions` | Credit usage history |
| `geo_restriction_logs` | Geographic restriction check logs |
| `auth_scopes` | ⚠️ DEPRECATED (0.2.0): garrison RBAC 接管权限映射，本表不再写入（`deprecated_at` 标记，仅作历史审计只读） |

**Cache Layer:**

**Technology:** oxcache 0.3 (in-memory, no Redis backend)

```rust
// Cache types
pub type SearchCache = Cache<String, Vec<SearchResult>>;
pub type DnsCache = Cache<String, DnsCacheEntry>;
pub type RegexCacheType = Cache<String, String>;
```

oxcache features activated: memory, serialization, macros, batch-write, metrics, bloom-filter, tracing. Type-specific TTL:
- Search cache: configurable (default 60s)
- DNS cache: configurable (default 300s)
- Regex cache: configurable (default 600s)

---

## Core Components

### Dependency Injection (trait-kit 0.3)

DI uses **trait-kit 0.3** with async module builders. Three top-level modules are registered:
- `InfrastructureModule` - Database pool, HTTP client, cache, repositories
- `EngineModule` - ReqwestEngine, PlaywrightEngine, FlareSolverrEngine, EngineRouter, EngineClient
- `ServiceModule` - Rate limiting, search, webhook, team services, workers

**CrawlRsState** is the runtime state extracted from the built DI container. Since v0.2.0 (`feature-gate-optional-modules` change), 7 fields are gated by `#[cfg(feature = "...")]` and only compiled when the corresponding business-capability feature is enabled:

```rust
#[derive(Clone)]
pub struct CrawlRsState {
    pub db_pool: Arc<DbPool>,
    pub task_repo: Arc<dyn TaskRepository>,
    pub credits_repo: Arc<dyn CreditsRepository>,
    pub crawl_repo: Arc<dyn CrawlRepository>,
    pub result_repo: Arc<dyn ScrapeResultRepository>,
    #[cfg(feature = "webhook")]
    pub webhook_repo: Arc<dyn WebhookRepository>,
    #[cfg(feature = "webhook")]
    pub webhook_event_repo: Arc<dyn WebhookEventRepository>,
    pub tasks_backlog_repo: Arc<dyn TasksBacklogRepository>,
    pub task_queue: Arc<dyn TaskQueue>,
    pub rate_limiting_service: Arc<dyn RateLimitingService>,
    #[cfg(feature = "teams")]
    pub team_service: Arc<TeamService>,
    // webhook_service 始终编译（trait），webhook-off 时装配 NoopWebhookService
    pub webhook_service: Arc<dyn WebhookService>,
    pub robots_checker: Arc<dyn RobotsCheckerTrait>,
    pub team_semaphore: Arc<TeamSemaphore>,
    pub engine_router: Arc<EngineRouter>,
    pub engine_client: Arc<EngineClient>,
    pub create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    pub search_client: Arc<SearchClient>,
    pub search_service: Arc<dyn SearchServiceTrait>,
    pub llm_service: Arc<dyn LLMServiceTrait>,
    pub extraction_service: Arc<dyn ExtractionServiceTrait>,
    pub content_extractor: Arc<ContentExtractionFacade>,
    pub regex_cache: Arc<RegexCache>,
    pub cache_service: Arc<dyn CacheService>,
    pub audit_service: Arc<dyn AuditServiceTrait>,
    pub request_coalescer: Arc<RequestCoalescer>,
    #[cfg(feature = "webhook")]
    pub webhook_worker: Arc<WebhookWorker>,
    pub backlog_worker: Arc<BacklogWorker>,
    pub expiration_worker: Arc<ExpirationWorker>,
    #[cfg(feature = "teams")]
    pub geo_location_service: Arc<dyn GeoLocationService>,
    #[cfg(feature = "teams")]
    pub geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
    pub i18n_bundle: Arc<I18nBundle>,
}
```

| Field | Gated By | Off-mode Behavior |
|-------|----------|-------------------|
| `webhook_repo` / `webhook_event_repo` / `webhook_worker` | `webhook` | Field not compiled; `/v1/webhooks/*` routes not registered; `webhook_worker` spawn block skipped |
| `team_service` / `geo_location_service` / `geo_restriction_repo` | `teams` | Field not compiled; `/v1/teams/*` routes not registered; `extract_handler` signature loses GR generic |
| `webhook_service` | (none, always compiled) | webhook-off: assembled with `NoopWebhookService` (no-op trait impl) |
| `rate_limiting_service` | (none, always compiled) | rate-limit-off: assembled with `NoopRateLimitingService` (always-allow trait impl) |

See [Feature Gate Architecture](#feature-gate-architecture) for the full gating matrix.

State is injected into Axum handlers via `Extension<CrawlRsState>`.

**Module dependency graph:**

```text
SettingsModule (config: Arc<Settings>)
  ├── DatabaseModule → Arc<DatabasePool>
  ├── HttpModule → Arc<reqwest::Client>
  └── CacheModule → CacheComponents
         ├── RepositoryModule → Repositories (depends: DatabaseModule)
         └── EngineModule → EngineComponents (depends: HttpModule, SettingsModule)
                └── ServiceModule → ServicesComponents (depends: all above)
```

### Feature Gate Architecture

Since v0.2.0 (`feature-gate-optional-modules` change), crawlrs exposes **4 business-capability features** that allow operators to build stripped-down binaries for single-tenant / no-auth / no-webhook / no-rate-limit deployments. The gating strategy follows two complementary patterns:

1. **Field/module gating** (`#[cfg(feature = "...")]` on `pub mod`/struct field/fn) — gated symbols are **not compiled** when the feature is off, eliminating dead code and reducing binary size.
2. **Noop trait injection** — for traits that business logic calls unconditionally (`WebhookService`, `RateLimitingService`), the trait is always compiled but the DI container assembles a no-op implementation when the feature is off, preserving call-site compatibility.

### Feature Matrix

| Feature | Default | Depends On | Off-mode Behavior |
|---------|---------|------------|-------------------|
| `teams` | on | `auth` | `/v1/teams/*` routes not registered; `extract_handler` loses GR generic + geo-restriction block; `CrawlRsState.{team_service, geo_location_service, geo_restriction_repo}` not compiled; `team_id` falls back to `DEFAULT_TEAM_ID` |
| `auth` | on | `dep:garrison, dep:inventory` | **0.2.0 起 garrison v0.8.1 接管认证**：`auth_middleware_inner` 调用 `GarrisonUtil::check_api_key` + `bridge_to_auth_state` 注入 `AuthState`；提供 RBAC + JWT + firewall-bruteforce（5 次/60 秒/300 秒锁定）+ audit-log。关闭时改为 `default_identity_middleware` 注入固定 `AuthState{team_id=DEFAULT_TEAM_ID, api_key_id=DEFAULT_API_KEY_ID, scope=ApiKeyScope::full_access()}`，无 DB 查询、无暴力破解防护 |
| `rate-limit` | on | `dep:limiteron` | `LimiteronService` replaced by `NoopRateLimitingService` (check_rate_limit→Allowed, check_and_deduct_quota→Ok, get_quota_balance→Ok(i64::MAX), process_backlog_tasks→Ok(0)); `limiteron_service`/`distributed_rate_limit_middleware`/`limiteron_rate_limit_middleware` modules not compiled |
| `webhook` | on | — | `WebhookServiceImpl`/`WebhookManagementServiceImpl`/`webhook_sender`/`webhook_handler`/`webhook_worker` modules not compiled; `/v1/webhooks/*` routes not registered; `webhook_worker` spawn blocks skipped; `WebhookService` trait preserved and assembled with `NoopWebhookService` (all ops return `Ok(())`) |

### Default feature set

```toml
[features]
default = ["teams", "auth", "rate-limit", "webhook"]
teams   = ["auth"]
auth    = ["dep:garrison", "dep:inventory"]   # 0.2.0: garrison v0.8.1 接管认证引擎
rate-limit = ["dep:limiteron"]
webhook = []
full    = ["standard", "engine-flaresolverr", "extractor-full", "http"]
```

### Gating pattern examples

**Field gating** (`src/di/axum_state.rs`):

```rust
#[cfg(feature = "webhook")]
pub webhook_repo: Arc<dyn WebhookRepository>,
```

**Noop injection** (`src/bootstrap/services.rs::init_rate_limiting_service`):

```rust
#[cfg(feature = "rate-limit")]
{ /* assemble LimiteronService */ }
#[cfg(not(feature = "rate-limit"))]
{
    log::warn!("rate-limit feature disabled, using NoopRateLimitingService");
    Arc::new(NoopRateLimitingService::new())
}
```

**Route gating with shadowing** (`src/bootstrap/routes.rs`):

```rust
#[cfg(feature = "webhook")]
let app = app.route("/v1/webhooks", post(webhook_handler::create_webhook::<WebhookRepoImpl>));
// webhook-off: route not registered, returns 404
```

**Middleware layer gating** (`src/bootstrap/routes.rs`):

```rust
#[cfg(feature = "auth")]
let app = app.layer(axum::middleware::from_fn(auth_middleware::auth_middleware()));
#[cfg(not(feature = "auth"))]
let app = {
    let template = build_default_identity_template(state);
    app.layer(axum::middleware::from_fn_with_state(
        template,
        auth_middleware::default_identity_middleware,
    ))
};
```

### CI verification

The `feature-matrix` job in `.github/workflows/ci.yml` runs `cargo check` against 7 feature combinations to ensure no broken cfg paths slip in:

| Combination | Flags |
|-------------|-------|
| no-default | `--no-default-features` |
| teams-only | `--no-default-features --features teams` |
| auth-only | `--no-default-features --features auth` |
| rate-limit-only | `--no-default-features --features rate-limit` |
| webhook-only | `--no-default-features --features webhook` |
| default | `--features default` |
| full | `--features full` |

### Migration notes

When a feature is off, the corresponding endpoints return **404** (not 401/403) because routes are not registered at startup. Operators switching from full to stripped-down builds should:

- For `auth`-off: pre-provision credits for `DEFAULT_TEAM_ID` via `add_credits` CLI (see `docs/USER_GUIDE.md` → Single-Tenant / No-Auth Deployment)
- For `rate-limit`-off: ensure upstream gateways enforce their own rate limiting
- For `webhook`-off: notify clients that completion callbacks will not be delivered
- For `teams`-off: only `DEFAULT_TEAM_ID` exists; multi-tenant data isolation is enforced at the application layer (all rows owned by `DEFAULT_TEAM_ID`)

---

## Data Flow

### Scrape Request Flow

```mermaid
flowchart TD
    A[1. Client Request] --> B[2. API Gateway<br/>Authentication<br/>Rate Limit<br/>SSRF Check]
    B --> C[3. Handler<br/>Validation<br/>DTO Mapping]
    C --> D[4. Use Case<br/>Business Logic]
    D --> E[5. Repository<br/>Task Creation]
    E --> F[6. Task Queue<br/>enqueue]
    F --> G[7. Scrape Worker<br/>dequeue + process]
    G --> H{8. Engine Selection}
    H --> H1[ReqwestEngine]
    H --> H2[PlaywrightEngine<br/>(if needs_js)]
    H --> H3[FlareSolverrEngine<br/>(if anti-bot)]
    H1 --> I[9. Result Storage<br/>oxcache + DB]
    H2 --> I
    H3 --> I
    I --> J[10. Task Update<br/>Status = Completed]
    J --> K[11. Webhook Notification<br/>Async]
    K --> L[12. Client Response<br/>Poll/Webhook]
```

### Crawl Request Flow

```mermaid
flowchart TD
    A[1. Client creates crawl task<br/>with URL and depth] --> B[2. Handler validates request<br/>and creates crawl record]
    B --> C[3. Crawl worker picks up task]
    C --> D[4. Worker fetches initial page]
    D --> E[5. Extracts links matching patterns]
    E --> F[6. Filters URLs<br/>include/exclude patterns]
    F --> G{7. For each URL}
    G --> G1[Create scrape subtask]
    G1 --> G2[Enqueue for processing]
    G2 --> G
    G --> H[8. Wait for all subtasks<br/>to complete]
    H --> I[9. Aggregate results]
    I --> J[10. Update crawl status<br/>to completed]
    J --> K[11. Trigger webhook notification]
```

---

## Crawling Engines

### Engine Architecture

The engine layer is organized around three core abstractions: `ScraperEngine` trait, `EngineRouter` with load balancing, and `EngineClient` as the public API.

**EngineClient** (`src/engines/engine_client.rs`) is the single public entry point for all scraping operations. It encapsulates UA rotation, circuit breaker state, engine selection, and retry logic.

**ScraperEngine Trait:**

```rust
#[async_trait]
pub trait ScraperEngine: Send + Sync {
    async fn scrape(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError>;

    fn support_score(&self, request: &InternalScrapeRequest) -> u8;

    fn name(&self) -> &'static str;

    fn supports_tls_fingerprint(&self) -> bool {
        false
    }
}
```

### Engine Router

The `EngineRouter` (`src/engines/router.rs`) holds a `Vec<Arc<dyn ScraperEngine>>` and selects engines using configurable load balancing strategies:

```rust
pub enum LoadBalancingStrategy {
    RoundRobin,
    WeightedRoundRobin,
    LeastConnections,
    FastestResponse,
    Random,
    SmartHybrid,  // default - combines multiple strategies
}

pub struct EngineRouter {
    engines: Vec<Arc<dyn ScraperEngine>>,
    circuit_breaker: Arc<CircuitBreaker>,
    // T003/R-sec-003: 使用 DashMap 替代 RwLock<HashMap>，避免读写锁借用开销
    engine_stats: Arc<DashMap<String, EngineStats>>,
    round_robin_index: Arc<parking_lot::Mutex<usize>>,
    strategy: LoadBalancingStrategy,
    metrics: Arc<RouterMetrics>,
    max_engine_attempts: usize,
    max_retries: usize,
    feature_filter_enabled: bool,
    race_mode_enabled: bool,
    dynamic_threshold_factor: f64,
    // T070/R-runtime-004：Hedge 控制器，race 胜出后记录延迟用于 P84 估算
    hedge_controller: HedgeController,
}
```

The router:
1. Filters engines by `support_score` (feature-based filtering)
2. Applies the selected load balancing strategy
3. Falls back to sequential engines on failure
4. Supports race mode (concurrent execution, return first success)
5. **T070**: race mode 胜出后调用 `hedge_controller.record_latency(response_time)` 更新 EMA/方差，用于后续 P84 阈值估算（详见 [Hedge 请求副本控制器](#hedge-请求副本控制器)）

### Hedge 请求副本控制器

<a id="hedge-请求副本控制器"></a>

**Location:** `src/utils/hedge.rs`

**背景（T070/R-runtime-004，design.md §17）：** 移植 spider `hedge.rs`，基于 EMA（指数移动平均）+ 方差估算 P84 延迟阈值，超阈值时建议发送副本请求降尾延迟。

**核心算法：**

- EMA：`EMA_new = α·x + (1-α)·EMA_old`
- 方差：`Var_new = (1-α)·Var_old + α·(x-EMA_new)·(x-EMA_old)`（指数加权移动方差，递推式）
- P84 阈值：`P84 = EMA + σ_multiplier · sqrt(Var)`（标准正态分布 P84 ≈ μ+σ）

**并发模型：** 使用 `parking_lot::Mutex<HedgeState>` 保护 `(ema, var, sample_count)` 三元组原子更新。非热路径（race 胜出后单次记录），Mutex 开销相对 race 100ms+ 网络耗时占比 < 0.0001%。

**接入路径：**

```rust
// EngineRouter::route_race_mode 胜出后记录延迟
Ok((engine_name, response, response_time)) => {
    self.hedge_controller.record_latency(response_time);
    Ok(response)
}

// 顺序路径未来可调用 should_hedge 决策副本触发
if router.hedge_controller().should_hedge(elapsed) {
    // 发送副本请求...
}
```

**样本来源限制（M-2）：** 当前 `record_latency` 仅在 race 胜出后调用，记录 `min(各引擎延迟)` 分布。**不可直接用于顺序路径 P84 估算**（顺序路径是单引擎全延迟，分布完全不同，复用会导致阈值系统性偏低）。顺序路径接入时需独立的 HedgeController 实例。

### WaitFor 策略

<a id="waitfor-策略"></a>

**Location:** 枚举定义在 `src/engines/engine_client.rs`，实现在 `src/engines/wait.rs`（`engine-playwright` feature 门控）

**背景（T069/R-jsrender-004，design.md §17）：** 替代原有 `sync_wait_ms` 固定 sleep，提供条件式等待：满足条件立即返回，超时返回错误，避免无谓阻塞。

```rust
pub enum WaitFor {
    NetworkIdle,           // 等待网络空闲（无新请求持续 500ms）
    Selector(String),      // 等待指定 CSS selector 出现在 DOM 中
    DomStable(Duration),   // 等待 DOM 稳定（无变化持续指定时长，上限 60s）
}
```

**接入路径：** `InternalScrapeRequest.wait_for: Option<WaitFor>` 字段，Playwright 引擎在 `goto` 后调用 `request.wait_for.unwrap_or_default().wait(&page, timeout)`。

**架构拆分动机：** 枚举定义在非 feature-gated 的 `engine_client.rs`，实现在 feature-gated 的 `wait.rs`，解决跨引擎依赖问题。

**已知限制（DTO 未桥接）：** API DTO 层 `ScrapeRequestOptions.wait_for: Option<u64>` 是数值字段（毫秒），当前 handler 未将其转换为 `WaitFor` 枚举。API 用户传入的 `wait_for` 数值被忽略，Playwright 引擎始终走 `NetworkIdle` 默认策略。计划在后续迭代中将 DTO 升级为 tagged enum JSON 格式（`{"network_idle": {}}` / `{"selector": "#target"}` / `{"dom_stable": {"duration_ms": 500}}`）。

### TabPool 浏览器 Tab 复用

<a id="tabpool-浏览器-tab-复用"></a>

**Location:** `src/engines/client/tab_pool.rs`

**背景（T068/R-runtime-003，design.md §17）：** Playwright 引擎每次抓取创建新 Tab 开销大（CDP `Target.createTarget` + `Page.goto`，约 50-200ms），TabPool 在 BrowserPool 之上进一步复用 Page，消除 tab 创建开销。

**实现：** 基于 `DashMap<usize, Page>` + `AtomicUsize` 栈顶指针的 LIFO 无锁栈：

```rust
pub struct TabPool {
    slots: DashMap<usize, Page>,  // slot 索引 → Page
    head: AtomicUsize,           // 单调递增的栈顶指针
    max_size: usize,             // 最大池容量
}
```

**acquire/release 流程：**

- `acquire(&browser)`: 原子递减 `head` 取栈顶 Page；池空时调用 `browser.new_page("about:blank")` 新建。
- `release(page)`: 先将 Page 导航到 `about:blank` 清理状态（5s 超时，超时则 drop 关闭 tab），再压回栈；池满（`head >= max_size`）则直接 drop。

**多 Browser 安全：** TabPool 不绑定特定 Browser，`acquire` 接受 `&Browser` 参数。池空时用传入的 Browser 新建 Page；池非空时弹出的 Page 可能属于其他 Browser。调用方应保证 TabPool 生命周期与单个 Browser 一致（per-Browser 实例化），或在多 Browser 场景下由上层 `BrowserPool` 按 instance_id 路由。

### Page Action Types

```rust
pub enum PageAction {
    Wait { milliseconds: u64 },
    Click { selector: String },
    Scroll { direction: ScrollDirection },
    Input { selector: String, text: String },
}
```

### Engine Types

#### 1. Reqwest Engine

**Use Cases:**
- Static HTML pages
- API responses
- JSON/XML data
- Fast scraping without JS

**Features:**
- HTTP/2 support via rustls
- TLS 1.2/1.3
- Cookie handling
- Custom headers and proxy support
- Gzip/brotli decompression

**Pros:**
- Fastest performance
- Lowest resource usage
- No browser overhead

**Cons:**
- No JavaScript execution
- Limited dynamic content support

#### 2. Playwright Engine (via chromiumoxide 0.9)

**Use Cases:**
- Single Page Applications (SPAs)
- JavaScript-heavy sites
- Sites requiring interactions
- Screenshots

**Features:**
- Full JavaScript execution via Chrome DevTools Protocol
- Page interactions (click, scroll, input)
- Screenshots
- Network interception

**Pros:**
- Full browser capabilities
- Renders dynamic content
- Can handle complex interactions

**Cons:**
- Higher resource usage
- Slower than HTTP client
- Requires Chromium installation

#### 3. FlareSolverr Engine

FlareSolverr merges the original three fire engines (FlareSolverrEngine, FireEngineCdp, FireEngineTls) into a single engine with mode selection:

```rust
pub enum FlareSolverrMode {
    Full,  // Full mode: JS rendering, session mgmt, CAPTCHA detection, screenshots
    Cdp,   // CDP mode: browser automation, TLS fingerprinting
    Tls,   // TLS mode: TLS fingerprinting only, no screenshots
}
```

All three modes share the same FlareSolverr API client implementation, differing only in `support_score` and `name`:

| Mode | `name()` | `supports_tls_fingerprint()` | Screenshots |
|------|----------|------------------------------|-------------|
| Full | `flaresolverr` | No | Yes |
| Cdp | `flaresolverr_cdp` | Yes | Yes |
| Tls | `flaresolverr_tls` | Yes | Rejected |

**Use Cases:**
- Cloudflare-protected sites
- Anti-bot protected sites
- High-anonymity requirements
- Google search CAPTCHA bypass

**Features (by mode):**
- Full: JS rendering, session management, CAPTCHA detection
- Cdp: TLS fingerprinting, browser automation, CDP protocol
- Tls: TLS fingerprint adversarial, fast execution, no screenshot

---

### Search Subsystem (T007/R-sec-007 委托架构)

**Location:** `src/search/`

**职责分层：**

- **smart 层** (`src/search/smart/mod.rs`)：URL 构建、速率限制、超时/重试控制、scoring、验证码前置检查、测试数据加载
- **client 层** (`src/search/client/{google,bing,baidu,sogou}.rs`)：HTML 解析、URL 提取、XSS 防护、CSS 选择器管理

**委托关系**：smart 端 `parse_google/bing/baidu/sogou_results` 委托 client 端 `parse_results`/`parse_search_results` 实现，消除 Stage 0 前的平行解析实现。smart 端补 score 由 `apply_scoring` 统一处理（PERF-03/MEDIUM-1）。

**性能优化**：
- PERF-02：smart 端用 `OnceCell` 缓存 4 个 client 引擎实例，避免每次解析都 `new` 一个 client 引擎
- PERF-05：client 端 `HtmlParser` 改为 `once_cell::sync::Lazy` 全局单例，`baidu.rs`/`bing.rs` 不再持有 `parser` 字段
- PERF-01：`google.rs` 的 `GoogleParseContext` 改为 `Lazy` 全局单例，避免重复编译 15 个 CSS 选择器

---

## Crawl Capability Enhancement Modules

> 以下模块由 `crawler-capability-absorption` 变更引入（0.2.0）。

### Anti-bot Detection (`src/engines/antibot/`)

三层分类器移植自 crawl4ai `antibot_detector.py`，gated `antibot` 特性：

- `patterns.rs`：Tier1（20+ WAF 结构标记：Cloudflare/Akamai/PerimeterX/DataDome 等）+ Tier2（通用词字面量集，构建 `aho_corasick::AhoCorasick`）+ Tier3 结构正则
- `classifier.rs`：`AntiBotTech` 枚举 + `Detection{tech,reason,needs_browser}` + `classify(status,body,headers,url)->Option<Detection>`
- 路由集成：`EngineRouter::route_internal` 在引擎成功分支调用 `classify`，命中 `needs_browser` 时强制后续 attempt `needs_js=true` 改派浏览器引擎

### HTTP→Chrome Upgrade Probe (`src/engines/upgrade_probe.rs`)

`JsUpgradeProbe` 通过强/弱信号评分探测 SPA 空壳：

- 强信号（score+=10）：`__NUXT_DATA__`/`__NEXT_DATA__`/`window.__INITIAL_STATE__`/空 root+hydration
- 弱信号（score+=1）：非追踪 `<script src>`（排除 ga/gtm/analytics/pixel）
- `score>=threshold` 时 `EngineRouter` 以 `needs_js=true` 重新改派 Playwright

### Memory-aware Scheduler (`src/workers/scheduler/`)

- `memory_scheduler.rs`：`MemoryState{Normal,Pressure,Critical}` + `MemoryScheduler` 复用 `SystemMonitorTrait`，`admit()` 返回 `Admission::{Proceed,Defer,Reschedule}`
- `priority_queue.rs`：`BinaryHeap<ScheduledTask>` 按 `effective_priority = base + waited_secs/aging_factor` 排序，防饿死
- 接入 `scrape_worker::process_task`：获取并发许可前调用 `admit()`

### UA Pool (`src/utils/ua_pool.rs`)

`UaProfile{ua,accept_language,sec_ch_ua,platform,viewport,mobile}` + `UaPool{desktop,mobile}`：

- 内置 20+ 桌面 + 移动真实 profile，每 profile 绑定一致的 UA/Accept-Language/sec-ch-ua/viewport
- `pick(mobile)`/`pick_seeded(seed,mobile)`：同 seed 稳定返回
- 已集成到 `ReqwestEngine`（header 设置）和 `PlaywrightEngine`（UA + viewport 设置）

### Smart Retry (`src/utils/retry/` + `src/utils/backoff.rs`)

- `tracker.rs`：`RetryReason{Transient,FeatureToggle,AntiBot}` + `RetryTracker` 各 reason 独立计数与上限
- `directive.rs`：`RetryDirective{rotate_proxy,rotate_ua,change_viewport,enable_stealth,force_browser}` + `for_attempt(reason,attempt)` 递增升级
- `backoff.rs`：full-jitter 退避（`cap = min(base*2^attempt, max)`，均匀 `[0, cap]`）
- 已集成到 `scrape_worker` 错误路径：分类错误 → 计算指令 → reason-specific 限制检查

### JS Injection (`src/engines/js_inject/`)

gated `engine-playwright`：

- `scripts/`：`flatten_shadow_dom.js`/`navigator_overrider.js`/`remove_consent_popups.js`/`remove_overlay_elements.js`（源自 crawl4ai，`include_str!` 嵌入）
- `injector.rs`：`InjectPhase{BeforeLoad,AfterLoad}` + `JsInjector::stealth()`/`cleanup()`/`apply(page,phase)`
- Playwright 引擎导航前注入 stealth，加载后注入 cleanup

### Request Interception (`src/engines/intercept.rs`)

gated `engine-playwright`，T033 / R-jsrender-003：

- 广告/追踪域名黑名单（`AD_DOMAIN_BLACKLIST`）：命中请求经 CDP `Fetch.failRequest` 中止
- 可选媒体资源拦截（`ResourceType::{Image, Media, Font}`）：由 `ScrapeOptions.block_media` 开关
- 拦截计数可观测
- `ScrapeOptions.block_ads` / `ScrapeOptions.block_media` 控制开关，经 `InternalScrapeRequest` 桥接至 Playwright 引擎

### Request Coalescing (`src/utils/coalesce.rs`)

移植 spider `coalesce.rs`：`RequestCoalescer{ in_flight: Arc<DashMap<String, InFlightEntry>> }`：

- `try_start(url) -> Proceed(CoalesceGuard) | Wait(broadcast::Receiver)`
- `STALE_TIMEOUT=120s` + `purge_stale()`
- 经 `CoalesceCoordinator` 接入 `scrape_worker`，同 URL 并发仅 1 次实际 fetch

### AIMD Adaptive Concurrency (`src/utils/adaptive_concurrency.rs`)

移植 spider `adaptive_concurrency.rs`：

- `AIMDController`：`AtomicUsize` 无锁，连续 N 次成功 +1，失败减半 clamp min
- `AdaptiveSemaphore`：桥接 `tokio::sync::Semaphore`，`set_target` 经 `add_permits`/`forget_permits`
- 集成 `TeamSemaphore::with_adaptive(...)`，默认关闭（`concurrency.adaptive_enabled=false`）

### Markdown Conversion (`src/domain/services/markdown_service.rs`)

gated `markdown` 特性：

- `MarkdownServiceTrait::to_markdown(html,only_main_content)->Result<String>` + `HtmdMarkdownService`
- `ScrapeResponse.markdown: Option<String>`
- `scrape_worker` 经 `MarkdownPostProcessor` 在 formats 含 `"markdown"` 时生成

### Content Extraction (`src/domain/services/content_extractor/`)

gated `extractor-trafilatura`/`extractor-dom-smoothie`/`extractor-full`：

- `traits.rs`：`ContentExtractor` trait + `ExtractedContent{text,title,author,confidence,page_type}`
- `trafilatura_extractor.rs`（主路径）→ `dom_smoothie_extractor.rs`（回退）→ `css_rule_extractor.rs`（兆底）
- `facade.rs`：`ContentExtractionFacade` 按优先级路由，`confidence<0.7` 触发 LLM 回退

### URL Normalization & Layered Dedup (`src/utils/url_normalizer.rs` + `src/utils/dedup/`)

- `url_normalizer.rs`：小写 host/去 fragment/统一 trailing slash/query 排序/可选去 query/`permutations`
- `dedup/bloom.rs` + `dedup/interner.rs`：Bloom⊕Interner 分层去重
- 接入 `scrape_worker::extract_and_queue_links`：Bloom 阴性直接入队，阳性回落 DB 精确校验

### Proxy Pool (`src/engines/proxy_pool.rs`)

- `ProxyEntry{url,healthy,cooldown,category}` + `ProxyPool`（RR `next(category)` + `sticky(session_id)`+TTL + `mark_failure/success`）
- 实现 `ProxyProvider` trait，`ReqwestEngine` 依赖抽象 trait
- `ProxySettings` 扩展 `urls: Vec<String>` + `strategy`（保留单 `url` 向后兼容）

### Advanced Cache Modes (`src/common/cache_mode.rs`)

- `CacheMode{Enabled,Disabled,ReadOnly,WriteOnly,Bypass}` + `CacheContext{url,method,mode}`
- `should_read()`/`should_write()`/`is_cacheable()`（data:/blob:/非幂等→false）
- `ScrapeOptions.cache_mode: Option<CacheMode>` + DTO `cache_mode`/`bypass_cache` 桥接
- `scrape_worker` 读写缓存前经 `CacheContext` 门控

### Waterfall/MRT Timeout

- `ScraperEngine::max_response_time() -> Duration`：各引擎覆写（fetch:5s/playwright:30s/cdp:30s/tls:15s）
- `EngineRouter::route_internal` 用 `min(remaining, engine.mrt())` 包裹单引擎调用，超 MRT 切下一引擎
- `EngineError::EngineMrtExceeded{engine,mrt}` 变体，`is_retryable()=true`

### Deep Crawling (`src/workers/crawl/`)

- `filters.rs`：`UrlFilter` trait + `FilterChain` + `DomainFilter`/`ContentTypeFilter`/`UrlPatternFilter`
- `scorers.rs`：`UrlScorer` trait + `CompositeScorer` + `KeywordRelevanceScorer`/`PathDepthScorer`
- `frontier.rs`：`ScoredUrl` + `BinaryHeap` 优先级 + 域名 round-robin
- `adaptive.rs`：`AdaptiveStrategy`（BM25/覆盖率/饱和度）+ `StopCondition`（最大页数/置信度/饱和度/无待处理链接）
- `scrape_worker::handle_crawl_success` 集成 `StopCondition` 检查，命中时提前终止 crawl

---

## Queue System

### Architecture

The queue system uses a **trait-based** approach with a PostgreSQL-backed implementation:

```rust
#[async_trait]
pub trait TaskQueue: Send + Sync {
    async fn enqueue(&self, task: Task) -> Result<Task, QueueError>;
    async fn dequeue(&self, worker_id: Uuid) -> Result<Option<Task>, QueueError>;
    async fn complete(&self, task_id: Uuid) -> Result<(), QueueError>;
    async fn fail(&self, task_id: Uuid) -> Result<(), QueueError>;
    async fn cancel(&self, task_id: Uuid) -> Result<(), QueueError>;
}
```

**PostgresTaskQueue** wraps the `TaskRepository` trait:

```rust
pub struct PostgresTaskQueue {
    pub repository: Arc<dyn TaskRepository>,
}
```

The `TaskRepository` provides `acquire_next(worker_id)` which uses `FOR UPDATE SKIP LOCKED` semantics for safe concurrent worker access.

### Worker Types

Six worker types run in the background:

| Worker | Purpose | Key Trait |
|--------|---------|-----------|
| `scrape_worker` | Process scrape tasks via EngineClient | `WorkerProcess` |
| `webhook_worker` | Deliver webhook events to configured URLs | `WorkerProcess` |
| `backlog_worker` | Reprocess expired/pending tasks | `WorkerProcess` |
| `expiration_worker` | Expire stale tasks past their TTL | `WorkerProcess` |
| `task_state_machine` | Handle task state transitions (queued → running → completed/failed) | `WorkerProcess` |
| `manager` | Orchestrate all worker lifecycle | `Manager` |

All workers use the `AbstractWorker<P>` template:

```rust
pub struct AbstractWorker<P>
where
    P: WorkerProcess + Send + Sync,
{
    processor: Arc<P>,
    interval: Duration,
}
```

Worker lifecycle:
1. `manager` starts all workers on application boot
2. Each worker runs its `process()` on a configurable interval
3. Workers share the same TaskRepository for task coordination via `FOR UPDATE SKIP LOCKED`
4. Graceful shutdown via broadcast channel

---

## Caching Strategy

### Architecture

Caching uses **oxcache 0.3** with a single in-memory backend (moka). There is no Redis or multi-tier cache.

```mermaid
flowchart TD
    subgraph L1 [oxcache in-memory (moka)]
        L1Feat1[oxcache::Cache K,V]
        L1Feat2[Configurable capacity & TTL]
        L1Feat3[LRU eviction]
        L1Feat4[Bloom filter]
        L1Feat5[Batch-write]
        L1Feat6[Metrics]
    end

    subgraph Source [Source]
        S1[Actual scraping]
        S2[Database queries]
        S3[DNS resolution]
    end

    L1 -->|"miss"| Source
```

### Cache Types

**Search Result Cache:**
```
search:query:lang=en:country=US:limit=10
TTL: configurable (default 60s)
```

**DNS Cache:**
```
dns:hostname:port
TTL: configurable (default 300s)
```

**Regex Cache:**
```
regex:{hash(pattern)}
TTL: configurable (default 600s)
```

### Cache Service Trait

```rust
#[async_trait]
pub trait CacheService: Send + Sync {
    fn get(&self, key: &str) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send + '_>>;
    fn set(&self, key: &str, value: &str, ttl_seconds: u64) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    fn delete(&self, key: &str) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    fn exists(&self, key: &str) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + '_>>;
}
```

### Cache Invalidation

- **Time-based TTL** - Automatic expiration per entry
- **Capacity-based** - LRU eviction when capacity exceeded
- **Bloom filter** - Quick negative existence check before cache lookup

---

## Rate Limiting

**Technology:** limiteron 0.2 (PostgreSQL-backed)

Rate limiting is implemented using **limiteron 0.2**, which provides PostgreSQL-backed token bucket rate limiting with the following features:

| Feature | Purpose |
|---------|---------|
| Token bucket | Per-key rate limiting with refill rate |
| Ban manager | Automatic ban on repeated violations |
| Quota control | Per-team quota management |
| Circuit breaker | Stop-the-world on sustained overload |
| Telemetry | Request tracking and monitoring |
| Parallel checker | High-concurrency rate limit checks |
| Audit log | Rate limit violation logging |

### Middleware Variants

Three rate limiting middleware implementations coexist:

1. **Basic Rate Limit** (`rate_limit_middleware.rs`) - In-memory token bucket for simple scenarios
2. **Distributed Rate Limit** (`distributed_rate_limit_middleware.rs`) - Coordination across instances
3. **Limiteron Rate Limit** (`limiteron_rate_limit_middleware.rs`) - PostgreSQL-backed via limiteron 0.2

### Multi-Level Limiting

```mermaid
flowchart TD
    subgraph Global [Global Rate Limit - System-wide]
        G1[Total requests/second]
        G2[Max concurrent connections]
        G3[Prevents system overload]
    end

    subgraph PerKey [Per-API Key Rate Limit]
        K1[Requests per minute]
        K2[Burst allowance - token bucket]
        K3[Configurable per key]
    end

    subgraph PerTeam [Per-Team Concurrency Limit]
        T1[Max concurrent tasks]
        T2[Semaphore-based enforcement]
        T3[Resource allocation per team]
    end

    Global --> PerKey
    PerKey --> PerTeam
```

### Concurrency Control

```rust
pub struct TeamSemaphore {
    permits: Arc<DashMap<Uuid, Arc<tokio::sync::Semaphore>>>,
    max_concurrent: Arc<DashMap<Uuid, usize>>,
}
```

The `TeamSemaphore` enforces per-team concurrency limits. Each team has a dedicated semaphore; the `team_semaphore_middleware` acquires/releases permits per request.

### Limiteron Service

```rust
pub struct LimiteronService {
    config: GlobalConfig,
    storage: Storage,
    ban_storage: BanStorage,
}
```

Configured with rules for:
- Default team rate limits
- API key-level rate limits
- Burst allowances

---

## Security Model

### Authentication

> **0.2.0 变更（`garrison-auth-migration`）：** 认证引擎由 garrison v0.8.1 接管。旧的手写 SHA-256 `api_key_hash` 查表、`AuthRateLimiter`、`AuthScopeService` 已被删除；`api_keys.key_hash` / `scopes` 表标记为弃用（`deprecated_at` 列）。

**Garrison 接管的认证流程：**

1. 客户端在 `Authorization: Bearer` 头中传入 garrison 签发的 key，格式为 `garrison_key_id.garrison_secret`
2. `auth_middleware_inner` 调用 `GarrisonUtil::check_api_key` 校验 key、过期、状态、RBAC 权限
3. garrison 内部维护 oxcache + 自管 postgres schema，命中时无 DB 往返
4. `auth_bridge::bridge_to_auth_state` 将 garrison 返回的 `permissions` 通过 `map_perms_to_scope` 映射为 `ApiKeyScope`，注入 `AuthState`
5. crawlrs 侧仅保留 `api_keys` 表的 `id`/`team_id`/`key` 列，用于 `api_key_id→team_id` 反查；该映射由 `TEAM_ID_CACHE` (LRU) 缓存

```rust
pub struct AuthState {
    pub pool: Arc<DbPool>,
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub scope: ApiKeyScope,
}
```

**401 / 429 由 garrison `firewall-bruteforce` 触发：**
- 默认策略：5 次失败 / 60 秒窗口 / 300 秒锁定
- 锁定期间所有请求返回 429（带 `Retry-After` 头）
- 失败计数与锁定状态由 garrison 自管 oxcache 持有

### Garrison 认证引擎

garrison v0.8.1 提供五大组件，crawlrs 通过 `auth` feature 隐式依赖：

| 组件 | 职责 | crawlrs 集成方式 |
|------|------|------------------|
| **DAO** | 自管 postgres schema（`garrison_api_keys` / `garrison_roles` / `garrison_permissions` 等） | 由 garrison 自行迁移，crawlrs 不感知 |
| **Interface** | `GarrisonUtil::check_api_key` / `ApiKeyHandler::generate_with_namespace` | `auth_middleware_inner` 与 `POST /v1/admin/api-keys` handler 直接调用 |
| **Config** | RBAC 角色 / 权限 / tenant 隔离配置 | crawlrs 启动时通过 `reissue_api_keys` CLI 预置 RBAC（`tenant_id=0`，所有 team 共享） |
| **RBAC + JWT** | HS256 JWT 签发、角色-权限映射、API Key 生成与校验 | 必填 `CRAWLRS__AUTH__JWT_SECRET`（≥32 字节，弱密钥拒绝启动） |
| **firewall-bruteforce** | 失败计数、窗口锁定、IP 隔离 | 401/429 由 garrison 直接返回，crawlrs 透传 |
| **audit-log** | 认证事件落库（成功 / 失败 / 锁定） | 写入 crawlrs `audit_logs` 表 + garrison 自管 schema |

**预置 RBAC 模型（`tenant_id=0`，所有 team 共享）：**

| 权限 | 角色 admin | 角色 user | 角色 read_only |
|------|-----------|-----------|----------------|
| `crawlrs:admin` | ✅ | ❌ | ❌ |
| `crawlrs:write` | ✅ | ✅ | ❌ |
| `crawlrs:read` | ✅ | ✅ | ✅ |

`auth_bridge::map_perms_to_scope` 将上述权限映射回 `ApiKeyScope`（确定性查找表，admin 蕴含 read+write）：
- `crawlrs:admin` → `read=true, write=true, admin=true`
- `crawlrs:write` → `write=true`
- `crawlrs:read` → `read=true`

全部通过 `ApiKeyScope::with_custom_limits` 构造（`DEFAULT_SEARCH_LIMIT=100`, `DEFAULT_SCRAPE_LIMIT=50`），不调用 `full_access()`——避免无限制 `u32::MAX` 与项目配额语义冲突。

### Authorization

**Scope-based Access Control:**

```rust
pub async fn scope_middleware(req: Request<Body>, next: Next) -> Result<Response, StatusCode>
```

`scope_middleware`（`auth_middleware.rs:335`）从 `AuthState.scope` 读取 `ApiKeyScope`，调用 `determine_required_scope(path, method)` 推导所需 `ScopePermission`，再用 `ApiKeyScope::has_permission` 校验。`ScopePermission` 枚举：`Read` / `Write` / `Admin`（`domain/auth/mod.rs:38`）。

Scopes: `scrape`, `crawl`, `search`, `extract`, `admin`

> **0.2.0 起**：`AuthState.scope` 由 `auth_bridge::map_perms_to_scope` 从 garrison permissions 桥接而来，下游 `scope_middleware` 无感知。

### 迁移指南（0.2.0 garrison-auth-migration）

**对现有部署的影响：**

1. **旧 API Key 作废**：原 `api_keys.key_hash` (SHA-256) 中的所有 key 失效，客户端必须经 garrison 重新领取
2. **`scopes` 表只读**：原有 scope 映射不再生效；新权限由 garrison RBAC 提供
3. **必填配置**：`CRAWLRS__AUTH__JWT_SECRET` 必须设置（HS256，≥32 字节），弱密钥将拒绝启动

**运维步骤：**

迁移已完成。新 team 通过 `POST /v1/admin/api-keys` 签发 API Key，无需运维工具介入。
开发/测试环境可通过 `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` 环境变量自动签发 admin key。

**`migrations/005_deprecate_legacy_api_keys.sql` 行为：**
- 仅给 `api_keys` 表追加 `deprecated_at` 列并回填 `NOW()`
- 仅给 `scopes` 表追加 `deprecated_at` 列并回填 `NOW()`
- 不删除任何数据、不删除任何列，便于回滚与历史审计

### SSRF Protection

SSRF validation occurs at two stages:
1. **Static validation** (synchronous pre-filter) - checks URL scheme, hostname patterns, IP ranges
2. **Full validation** (async with DNS resolution) - resolves hostnames, checks resolved IPs

**Blocked patterns (static validation):**
- `http://localhost:*` (loopback hostnames)
- `http://127.*` (IPv4 loopback)
- `http://10.*` (RFC 1918 class A)
- `http://192.168.*` (RFC 1918 class C)
- `http://172.16-31.*` (RFC 1918 class B)
- `http://[::1]*` (IPv6 loopback)
- `http://[fe80:*` (IPv6 link-local)
- `file://*` and `ftp://*` (non-HTTP schemes)

**Blocked ports:**
- 25 (SMTP), 465 (SMTPS), 587 (SMTP submission)
- 3306 (MySQL), 5432 (PostgreSQL), 6379 (Redis), 27017 (MongoDB)

**DNS rebinding protection:**
- Resolve hostname to IPs at request time
- Validate all resolved IPs against private ranges
- Cache DNS results with configurable TTL

```rust
pub enum SsrfValidationResult {
    Safe(ValidatedUrl),
    Blocked { url: String, reason: String },
    RequiresDnsResolution { url: String, hostname: String },
}
```

### Audit Logging

**Logged Events:**
- API requests (endpoint, method, timestamp, team ID)
- Authentication attempts (success/failure)
- Rate limit violations
- Denied requests (SSRF, invalid key, scope violation)
- Task lifecycle events (created, completed, failed)

---

## Deployment Architecture

### Single Instance

```mermaid
flowchart TB
    subgraph Container [Docker Container]
        subgraph App [crawlrs Application]
            S1[Axum Server<br/>port 8899]
            S2[Worker Pool<br/>6 worker types]
        end
    end

    subgraph Resources [External Resources]
        DB[(Postgres Database)]
        RC[(oxcache<br/>in-memory)]
    end

    App --> DB
    App --> RC
```

### Multi-Instance Deployment (Kubernetes)

```mermaid
flowchart TB
    subgraph K8s [Kubernetes Cluster]
        subgraph Pods [Application Pods]
            subgraph Pod1 [Pod 1]
                A1[crawlrs<br/>API + Workers]
            end

            subgraph Pod2 [Pod 2]
                A2[crawlrs<br/>API + Workers]
            end
        end

        subgraph Infrastructure [Infrastructure Services]
            DB[(Postgres<br/>Primary + Replica)]
            MON[Prometheus + Grafana<br/>Metrics collection<br/>Visualization]
        end
    end

    Pod1 --> DB
    Pod2 --> DB
    Pod1 --> MON
    Pod2 --> MON
```

### Deployment Components

1. **Application Pods**
   - Each pod runs both API server and worker pool
   - Horizontal Pod Autoscaler (CPU/memory based)
   - Load balancer (Ingress)
   - Health check endpoints: `/health`, `/metrics`

2. **PostgreSQL**
   - Primary + Read replicas
   - Connection pooling via dbnexus
   - Automated backups

3. **Monitoring**
   - Prometheus metrics scraping (via `metrics-exporter-prometheus`)
   - inklog structured logging
   - Grafana dashboards

---

## Scalability Considerations

### Horizontal Scaling

**Stateless Workers:**
- Workers share no local state
- Task coordination via PostgreSQL `FOR UPDATE SKIP LOCKED`
- No session affinity required

**Shared State:**
- PostgreSQL is the single source of truth
- oxcache is per-instance (in-memory), not shared

### Scaling Strategy

```mermaid
flowchart TD
    A[Load ↑] --> B[Scale workers]
    B --> C{Check: CPU < 80%,<br/>Memory < 80%}
    C -->|OK| D[Scale up]
    C -->|Not OK| E[Scale database]
```

### Bottleneck Identification

| Metric | Action |
|--------|--------|
| Queue depth > 1000 | Scale worker pods |
| Cache hit < 50% | Increase cache size |
| DB query time > 100ms | Add replicas / optimize queries |
| Memory > 80% | Scale vertically or horizontally |
| CPU > 80% | Scale horizontally |

---

## Future Enhancements

### Planned Architecture Improvements

1. **Event-Driven Architecture**
   - Event bus for internal communication
   - Webhook event sourcing

2. **WebSocket Support**
   - Real-time task status updates
   - Push notifications (reduced polling)

3. **Redis Cache Layer**
   - Shared cache across instances
   - Replace per-instance oxcache for multi-pod deployments

---

## Documentation

- [API Reference](API_REFERENCE.md)
- [User Guide](USER_GUIDE.md)

---

**Last Updated:** 2025-07-21
