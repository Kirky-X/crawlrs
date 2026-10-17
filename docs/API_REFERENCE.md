
# 📘 Crawlrs API Reference

<div align="center">

![API Version](https://img.shields.io/badge/api-0.2.0--rc.2-blue)
![Base URL](https://img.shields.io/badge/base%20URL-http://localhost:8899-green)
![License](https://img.shields.io/badge/license-Apache%202.0-orange)

**Version:** 0.2.0-rc.2 | **Base URL:** `http://localhost:8899` | **Updated:** 2026-10-13

</div>

---

## 📋 目录

- [🔐 Authentication](#-authentication)
- [📄 Common Response Format](#-common-response-format)
- [⚠️ Errors](#️-errors)
- [🌐 Public Endpoints](#-public-endpoints)
  - [Health Check](#health-check-liveness)
  - [Readiness Check](#readiness-check)
  - [Garrison Health Probes](#garrison-health-probes-auth-builds-only)
  - [OpenAPI Specification](#openapi-specification)
  - [Get Version](#get-version)
  - [Get Metrics](#get-metrics)
- [🔒 Protected Endpoints](#-protected-endpoints)
  - [Scrape API](#scrape-api)
  - [Crawl API](#crawl-api)
  - [Search API](#search-api)
  - [Extract API](#extract-api)
  - [Task API](#task-api)
  - [Team API](#team-api)
  - [Webhook API](#webhook-api)
  - [Audit API](#audit-api)
  - [Admin API](#admin-api)
- [⏱️ Rate Limiting](#️-rate-limiting)
- [🪝 Webhooks](#-webhooks)
- [🧩 SDK API](#-sdk-api)
- [💻 SDK Examples](#-sdk-examples)
- [✅ Best Practices](#-best-practices)
- [📋 Changelog](#-changelog)
- [📞 Support](#-support)

---

## 🔐 Authentication

All protected endpoints require authentication using an API key in the `Authorization` header:

```http
Authorization: Bearer YOUR_API_KEY
```

> **0.2.0 起（`garrison-auth-migration`）：** Bearer token 由 **garrison v0.9.0-rc.2** 校验，格式为 `garrison_key_id.garrison_secret`（明文 key 仅在签发时返回一次）。旧的 SHA-256 `api_key_hash` 已作废，需经 garrison 重新领取。

### Garrison 认证

**认证流程：**

1. 客户端在 `Authorization: Bearer` 头中传入 garrison 签发的 key（格式 `garrison_key_id.garrison_secret`）
2. `auth_middleware_inner` 调用 `GarrisonUtil::check_api_key` 校验 key、过期、状态、RBAC 权限
3. `auth_bridge::bridge_to_auth_state` 将 garrison permissions 通过 `map_perms_to_scope` 映射为 `ApiKeyScope`，注入 `AuthState`
4. 401（无效 key）/ 429（暴力破解锁定）由 garrison `firewall-bruteforce` 触发（默认 5 次失败/60 秒窗口/300 秒锁定）

**RBAC 权限映射（`tenant_id=0`，所有 team 共享，admin 蕴含 read+write）：**

| garrison 权限 | 映射到 `ApiKeyScope` |
|---------------|----------------------|
| `crawlrs:admin` | `read=true, write=true, admin=true` |
| `crawlrs:write` | `write=true` |
| `crawlrs:read` | `read=true` |

全部通过 `ApiKeyScope::with_custom_limits` 构造（`DEFAULT_SEARCH_LIMIT=100`, `DEFAULT_SCRAPE_LIMIT=50`），不调用 `full_access()`——避免无限制 `u32::MAX` 与项目配额语义冲突。

**JWT 配置：**

| 环境变量 | 描述 | 必填 |
|----------|------|------|
| `CRAWLRS__AUTH__JWT_SECRET` | HS256 JWT 密钥，≥32 字节；弱密钥拒绝启动 | 是（`auth` feature 开启时） |

### API Key Scopes

API keys carry an `ApiKeyScope` with three boolean permissions plus two hourly limits (`src/domain/auth/mod.rs`):

| Field | Description |
|--------|-------------|
| `read` | Permission to access read-only endpoints |
| `write` | Permission to access write endpoints |
| `admin` | Permission to access administrative endpoints（蕴含 read + write） |
| `search_limit` | Maximum search requests per hour |
| `scrape_limit` | Maximum scrape requests per hour |

> **重要现状（0.2.0-rc.2）：** `scope_middleware` 虽已实现（`src/presentation/middleware/auth_middleware.rs`），但**未挂载到生产路由**——仅存在于测试 Router 装配。除 `POST /v1/admin/api-keys` 在 handler 内部显式校验 `Admin` 权限外，其余端点目前不校验 API Key scope，仅校验 key 有效性。请勿误以为 RBAC 已在端点层生效。

### Auth Disabled Behavior (auth-off)

> **Conditional:** When the `auth` feature is disabled (via `--no-default-features` or explicit feature selection), the real `auth_middleware_inner` is not compiled. `default_identity_middleware` injects a prebuilt `AuthState` template instead — but **默认拒绝**：未显式 opt-in 时，所有 protected 请求一律返回 `401`。

| Behavior | When `auth` Enabled (default) | When `auth` Disabled |
|----------|------------------------------|---------------------|
| `Authorization` header | Required (401 if missing/invalid; 429 on brute-force lockout) | **Ignored, but requests still 401** until `allow_unauthenticated_protected()` is called |
| Opt-in | N/A | Must explicitly call `auth_middleware::allow_unauthenticated_protected()` (one-time `error!` log declares the security downgrade) |
| `AuthState.team_id` | Resolved via garrison → `api_key_id→team_id` 反查 + `TEAM_ID_CACHE` LRU | Fixed to `DEFAULT_TEAM_ID` (`domain::id::Id::from_u128(1)`) |
| `AuthState.api_key_id` | Resolved from garrison key | Fixed to `DEFAULT_API_KEY_ID` (`domain::id::Id::from_u128(2)`) |
| `AuthState.scope` | `map_perms_to_scope(garrison permissions)` | `ApiKeyScope::denied()` (read/write/admin all `false`, limits 0) — 受限匿名身份，非 `full_access()` |
| Token cache | garrison 自管 oxcache；crawlrs 侧 `TEAM_ID_CACHE` 缓存映射 | N/A — fixed identity, no caching |
| Rate limit lockout | garrison `firewall-bruteforce` (5/60s/300s) | Not tracked (but `rate-limit` feature may still apply per-IP limits) |

> 源码依据：`src/bootstrap/routes.rs::build_default_identity_template`（模板携带 `ApiKeyScope::denied()`）与 `src/presentation/middleware/auth_middleware.rs::default_identity_middleware`（默认 401，opt-in 后注入模板）。opt-in 放行后的身份仍是 denied 受限身份，业务层若按权限判断将拒绝服务。

**Build commands:**
```bash
# Auth disabled (single-tenant, no auth)
cargo build --release --no-default-features

# Auth enabled (default, includes garrison)
cargo build --release --features default
```

### 迁移指南（0.2.0 garrison-auth-migration）

**对现有 API 客户端的影响：**

- 旧 API Key（基于 SHA-256 `api_key_hash`）全部作废，必须经 garrison 重新领取
- 客户端无需改动 `Authorization: Bearer` 调用方式，仅需替换为新的 `garrison_key_id.garrison_secret`
- 401/429 响应语义不变，但触发逻辑由 garrison `firewall-bruteforce` 接管

**迁移已完成：**

- 旧 API Key 已作废，客户端须使用 `Authorization: Bearer <garrison_key_id>.<garrison_secret>` 格式
- 新 team 通过 `POST /v1/admin/api-keys` 签发 API Key，无需运维工具介入

详细架构说明参见 [ARCHITECTURE.md → Garrison 认证引擎](ARCHITECTURE.md#garrison-认证引擎)。

---

## 📄 Common Response Format

All API responses follow this unified structure:

### Success Response

```json
{
  "success": true,
  "data": {
    // Response data here
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

### Success Response with Pagination

```json
{
  "success": true,
  "data": {
    // Response data here
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

> **当前无端点使用 `meta` 分页包封。** `ApiResponse` 结构体与 `success_response_with_meta` 构造器在 `src/presentation/handlers/response_builder.rs` 有定义（携带 `PaginationMeta`），但生产 handler 无任何调用点；列表端点（如 `GET /v1/webhooks`、audit logs）的分页信息直接平铺在 `data` 内。上述 `meta` 形态仅为保留的库能力，不是现行 API 契约。

### Error Response

```json
{
  "success": false,
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Detailed error message"
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

### Rate Limit Error Response

限流拒绝（429）由各 handler 内 `check_rate_limit` 经标准 `ApiResponse` 包封返回，`error` 为 `{code,message}` 对象：

```json
{
  "success": false,
  "error": {
    "code": "RATE_LIMITED",
    "message": "Rate limit exceeded: <reason>"
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

当命中 `RetryAfter` 结果时额外带 `retry_after_seconds` 字段：

```json
{
  "success": false,
  "error": {
    "code": "RATE_LIMITED",
    "message": "Rate limit exceeded, please retry after 60 seconds"
  },
  "retry_after_seconds": 60,
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

> 源码依据：`src/presentation/helpers/rate_limit_helper.rs::check_rate_limit`（`Denied` → `errors::too_many_requests`，`RetryAfter` → `errors::too_many_requests_with_retry`，后端错误 fail-closed 503 → `errors::service_unavailable`）。响应头仅设 `Retry-After`，无 `X-RateLimit-*` 系列头。

### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `success` | boolean | Whether the request was successful |
| `data` | object | Response data (only present on success) |
| `error` | object | Error details (only present on failure) |
| `error.code` | string | Error code for programmatic handling |
| `error.message` | string | Human-readable error message |
| `timestamp` | string | Response timestamp in RFC3339 format |

---

## ⚠️ Errors

### HTTP Status Codes

| Code | Description |
|------|-------------|
| 200 | Success |
| 201 | Created |
| 400 | Bad Request - Invalid parameters |
| 401 | Unauthorized - Missing or invalid API key |
| 403 | Forbidden - Insufficient permissions |
| 429 | Too Many Requests - Rate limit exceeded |
| 422 | Unprocessable Entity - Validation error |
| 500 | Internal Server Error |

### Error Codes

生产错误码清单（`src/common/error.rs::CrawlRsError::error_code` 与 `src/presentation/handlers/response_builder.rs::error_codes`）：

| Error Code | HTTP Status | Description |
|------------|-------------|-------------|
| `VALIDATION_ERROR` | 400 | Invalid request parameters |
| `NOT_FOUND` | 404 | Resource not found |
| `UNAUTHORIZED` | 401 | Missing or invalid API key |
| `AUTHENTICATION_ERROR` | 401 | Authentication failed (e.g. webhook signature verification) |
| `FORBIDDEN` | 403 | Insufficient permissions |
| `RATE_LIMITED` | 429 | Rate limit exceeded |
| `UNPROCESSABLE_ENTITY` | 422 | Structural validation error (field missing / wrong type / unknown field) |
| `INTERNAL_ERROR` | 500 | Internal server error |
| `SERVICE_UNAVAILABLE` | 503 | Service unavailable |
| `TASK_ERROR` | 500 | Task processing error |
| `ENGINE_ERROR` | 502 | Scraping engine error |
| `JSON_ERROR` | 400 | JSON serialization/deserialization error |
| `IO_ERROR` | 500 | Internal I/O error |
| `CONFIGURATION_ERROR` | 500 | Configuration error |
| `QUOTA_EXCEEDED` | 402 | Quota exceeded |
| `DATABASE_ERROR` | 500 | Database error（经 `CrawlRsError` 路径间接产出） |
| `CACHE_ERROR` | 503 | Cache error（经 `CrawlRsError` 路径间接产出） |
| `EXTERNAL_SERVICE_ERROR` | 502 | External service error（经 `CrawlRsError::Network` 间接产出） |
| `TIMEOUT` | 504 | Request timeout（经 `CrawlRsError::Timeout` 间接产出） |

> `CONFLICT` / `PRECONDITION_FAILED` / `FEATURE_DISABLED` 在 `error_codes` 模块有常量定义，但无生产发射点；feature 未编译时对应路由根本不注册，请求得到的是普通 404（`NOT_FOUND`）。

---

## 🌐 Public Endpoints

### Health Check (Liveness)

Check if the API process is running. Always returns 200 when the process is alive — dependencies are not probed here.

**Endpoint:** `GET /health`

**Response:**
```json
{
  "status": "healthy",
  "version": "0.2.0-rc.2"
}
```

### Readiness Check

Kubernetes readiness probe: probes PostgreSQL (`SELECT 1`, 3s timeout) and the cache service (`get("__readiness_probe__")`, 1s timeout). Returns 200 with `status: "ready"` when all dependencies are up, 503 with `status: "not_ready"` otherwise.

**Endpoint:** `GET /ready`

**Response (200):**
```json
{
  "status": "ready",
  "checks": {
    "database": { "status": "up" },
    "cache": { "status": "up" },
    "database_pool": { "status": "ok" }
  },
  "version": "0.2.0-rc.2"
}
```

**Response (503):**
```json
{
  "status": "not_ready",
  "checks": {
    "database": { "status": "down", "error": "..." },
    "cache": { "status": "up" },
    "database_pool": { "status": "ok" }
  },
  "version": "0.2.0-rc.2"
}
```

> `checks` 含 `database`、`cache` 与 `database_pool`（连接池结构化快照：容量/空闲/等待，仅诊断用途，不影响就绪判定）。各探测失败标记 `status: "down"` 并附 `error`（超时为 `"timeout"`）。

### Garrison Health Probes (`auth` builds only)

When the `auth` feature is enabled, garrison's `health_routes` are merged into the public router (`src/bootstrap/routes.rs::create_public_routes`), exposing two additional paths:

- `GET /health/live` — liveness probe, always 200
- `GET /health/ready` — readiness probe aggregating the garrison `HealthRegistry` checks; `overall` Unhealthy returns 503 (K8s traffic-removal semantics carried by the library)

These paths do not collide with `/health` and `/ready`. `auth`-off builds do not compile these routes.

### OpenAPI Specification

**Endpoint:** `GET /openapi.json` (requires the `openapi` feature; registered at the root path by `src/presentation/sdk/mod.rs`)

**Response:** OpenAPI 3.1 document (JSON), generated by sdforge from all registered SDK endpoints.

### Get Version

Get the current API version.

**Endpoint:** `GET /v1/version`

**Response:**
```text
0.2.0-rc.2
```

### Get Metrics

**Endpoint:** `GET /metrics`

> **⚠️ 当前为占位实现。** `metrics_handler` 返回 JSON 包封，其中 `metrics` 字段是一段**硬编码的 Prometheus 风格文本**——`version` 固定为 `1.0.0`、uptime 固定为 `3600`，并非运行时真实指标，也不是 Prometheus 文本格式聚合端点。权威的真实指标清单（采集点与标签定义）见 [PERFORMANCE.md](PERFORMANCE.md)。`/metrics` 的权威抓取格式（可直接被 Prometheus scraper 消费）尚未实现。

**Response:**
```json
{
  "status": "ok",
  "metrics": "# HELP app_info Information about the application\n# TYPE app_info gauge\napp_info{version=\"1.0.0\"} 1\n# HELP app_uptime_seconds Application uptime in seconds\n# TYPE app_uptime_seconds counter\napp_uptime_seconds 3600"
}
```

---

## 🔒 Protected Endpoints

### Scrape API

Scrape a single web page.

#### Create Scrape Task

**Endpoint:** `POST /v1/scrape`

**Request Body:**
```json
{
  "url": "https://example.com",
  "formats": ["markdown", "html"],
  "include_tags": ["h1", "h2", "p"],
  "exclude_tags": ["script", "style"],
  "webhook": "https://your-webhook.com/callback",
  "extraction_rules": {
    "title": {
      "selector": "h1",
      "attr": "text"
    }
  },
  "actions": [
    {
      "type": "wait",
      "milliseconds": 1000
    },
    {
      "type": "click",
      "selector": ".load-more"
    }
  ],
  "options": {
    "headers": {
      "User-Agent": "Mozilla/5.0..."
    },
    "wait_for": 2000,
    "timeout": 30,
    "js_rendering": false,
    "screenshot": true,
    "screenshot_options": {
      "full_page": true,
      "selector": "#main",
      "quality": 90,
      "format": "png"
    },
    "mobile": false,
    "proxy": "http://proxy.example.com:8080",
    "skip_tls_verification": false,
    "needs_tls_fingerprint": false,
    "use_fire_engine": false
  },
  "metadata": {
    "custom_key": "custom_value"
  },
  "sync_wait_ms": 5000
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `url` | string | Yes | Target URL (http/https only) |
| `formats` | array | No | Output formats: `markdown`, `html`, `text` |
| `include_tags` | array | No | HTML tags to include in output |
| `exclude_tags` | array | No | HTML tags to exclude from output |
| `webhook` | string | No | Webhook URL for completion notification |
| `extraction_rules` | object | No | CSS selector extraction rules |
| `extraction_prompt` | string | No | LLM prompt for data extraction (free text) |
| `extraction_schema` | object | No | JSON Schema for structured LLM extraction |
| `actions` | array | No | Page interaction actions |
| `options` | object | No | Scraping options (see [Scrape API](#scrape-api)) |
| `metadata` | object | No | Custom metadata for the task |
| `sync_wait_ms` | integer | No | Wait time for synchronous response (max 30000) |

<a id="scrape-options-parameters"></a>

**Options Parameters:**

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `headers` | object | empty | Custom HTTP headers (e.g. `User-Agent`) |
| `wait_for` | integer | — | Wait hint in milliseconds (DTO validation max 60000, but the use case clamps `ms.min(10_000)` — values above 10000 are silently truncated to 10000). Mapped to the `DomStable` wait strategy (DOM unchanged for this duration ⇒ ready); `None` uses the engine default (`WaitFor::NetworkIdle`, CDP network-event based). See [ARCHITECTURE.md → WaitFor](ARCHITECTURE.md#waitfor-策略). |
| `timeout` | integer | 30 | Request timeout in seconds |
| `js_rendering` | boolean | false | Enable JavaScript rendering |
| `screenshot` | boolean | false | Capture screenshot |
| `screenshot_options` | object | — | Screenshot config (`full_page`, `selector`, `quality`, `format`) |
| `mobile` | boolean | false | Emulate mobile device |
| `proxy` | string | — | Proxy URL (e.g. `http://proxy:8080`) |
| `skip_tls_verification` | boolean | false | Skip TLS certificate verification |
| `needs_tls_fingerprint` | boolean | false | Require TLS fingerprint adversarial engine (`engine-tls-fingerprint` feature) |
| `use_fire_engine` | boolean | false | Force use FlareSolverr engine |
| `cache_mode` | string | `"enabled"` | Cache read/write mode: `enabled` (default, normal R/W), `disabled` (cache fully off), `read_only` (hit returns, miss fetches without writing), `bypass` (skip read, normal write — emergency untrusted-cache scenario) |
| `bypass_cache` | boolean | false | Emergency cache bypass shortcut. `true` overrides `cache_mode` to `bypass` (skip cache read, normal write-back). Use when runtime cache data is untrusted |
| `only_main_content` | boolean | — | Extract main content before Markdown conversion (strips nav/footer/sidebar/ads). Requires the `markdown` format |
| `respect_robots` | boolean | — | Honor the target site's robots.txt for this request (falls back to global `robots.scrape_respect_robots`, default off) |
| `conditional_request` | boolean | — | Incremental fetch: send `If-None-Match`/`If-Modified-Since` when a prior result exists; a `304` reuses the historical content (`x-crawlrs-not-modified: true`) |
| `block_ads` | boolean | false | Block common ad-domain requests in browser engines |
| `block_media` | boolean | false | Block image/video/audio media loading in browser engines (speeds up text-only scraping) |
| `needs_mllm` | boolean | false | Require MLLM visual analysis of the screenshot. Requires the `engine-mllm` feature: builds without it reject the request **at submission time** with an explicit validation error (`needs_mllm requires the \`engine-mllm\` feature`) instead of silently degrading to a non-MLLM engine; the worker repeats the check for paths that bypass the `create_scrape` use case |

> `session_id` remains an internal engine field (browser-session reuse is not exposed per-request yet). Unknown fields are still rejected with 422 via `deny_unknown_fields`.

**Action Types:**

| Type | Parameters | Description |
|-------|-----------|-------------|
| `wait` | `milliseconds` | Wait for specified time |
| `click` | `selector` | Click element matching selector |
| `scroll` | `direction` | Scroll page (up/down) |
| `screenshot` | `full_page` | Take screenshot |
| `input` | `selector`, `text` | Input text into element |

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "url": "https://example.com",
    "credits_used": 1
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

> `credits_used` 固定为 1（每次抓取任务入队时扣减 1 credit；入队失败自动退还）。状态码：入队成功为 `201 Created`；`sync_wait_ms > 0` 且等待超时时为 `202 Accepted`（body 结构不变，仍含任务 `id`）。

#### Get Scrape Status

**Endpoint:** `GET /v1/scrape/{id}`

**Parameters:**
- `id` (path) - Task UUID

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "completed",
    "url": "https://example.com",
    "created_at": "2025-01-15T12:00:00",
    "completed_at": "2025-01-15T12:00:03",
    "result": {
      "content": "<html>...</html>",
      "status_code": 200,
      "content_type": "text/html",
      "response_time_ms": 1200,
      "headers": { "content-type": "text/html" },
      "meta_data": {},
      "screenshot": null,
      "created_at": "2025-01-15T12:00:03"
    },
    "metadata": null,
    "error": null
  },
  "timestamp": "2025-01-15T12:00:03+00:00"
}
```

> 响应为平铺的 `ScrapeStatusResponseDto`（无 `task` 外层键）。`result` 仅在任务 `completed` 时出现；敏感响应头（`set-cookie` / `authorization` 等）在返回前脱敏为 `[REDACTED]`。`error` 仅在任务 `failed` 时出现。

#### Cancel Scrape

Cancel a single scrape task by ID. Only the owning team can cancel; the task is marked `cancelled`.

**Endpoint:** `POST /v1/scrape/{id}/_cancel`

**Parameters:**
- `id` (path) - Task UUID

**Response:**
```json
{
  "success": true,
  "data": {
    "message": "Scrape task cancelled"
  },
  "timestamp": "2025-01-15T12:00:05+00:00"
}
```

---

### Crawl API

Crawl multiple pages from a starting URL.

#### Create Crawl Task

**Endpoint:** `POST /v1/crawl`

**Request Body:**
```json
{
  "url": "https://example.com",
  "name": "my-crawl",
  "config": {
    "max_depth": 2,
    "max_pages": 100,
    "include_patterns": ["/blog/.*"],
    "exclude_patterns": ["/admin/.*"],
    "strategy": "bfs",
    "crawl_delay_ms": 1000,
    "max_concurrency": 4,
    "proxy": "http://proxy.example.com:8080"
  },
  "sync_wait_ms": 10000
}
```

> 请求体经 `#[serde(deny_unknown_fields)]` 校验：未列出的字段（如 `formats`、`webhook`、`options`、`follow_links`）会被 422 拒绝。

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `url` | string | Yes | Starting URL (http/https only) |
| `name` | string | No | Task name (1-255 chars) |
| `config` | object | Yes | Crawl configuration (see below) |
| `sync_wait_ms` | integer | No | Wait time for synchronous response (0-30000, default 5000) |
| `expires_at` | string | No | Task expiry time (RFC3339) |
| `sitemap_seed` | boolean | No | Use sitemap as URL discovery source (requires `CRAWLRS__SITEMAP__ENABLED`) |
| `sitemap_url` | string | No | Explicit sitemap URL (defaults to `<origin>/sitemap.xml`) |

**`config` object (`CrawlConfigDto`):**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `max_depth` | integer | Yes | Maximum crawl depth (required; DTO max 100, but the handler tightens this to ≤ 5 — `max_depth > 5` returns 422) |
| `include_patterns` | array | No | Regex patterns for URLs to include |
| `exclude_patterns` | array | No | Regex patterns for URLs to exclude |
| `strategy` | string | No | Traversal strategy: `bfs` (default) or `dfs` |
| `crawl_delay_ms` | integer | No | Per-host crawl delay (max 86400000) |
| `max_concurrency` | integer | No | Max concurrent pages (max 50) |
| `max_pages` | integer | No | Maximum pages to crawl (default 1000, max 100000; exceeded stops early) |
| `proxy` | string | No | Proxy URL |
| `headers` | object | No | Custom HTTP headers |
| `extraction_rules` | object | No | CSS selector extraction rules |
| `extraction_prompt` | string | No | LLM prompt for data extraction on each crawled page |
| `extraction_schema` | object | No | JSON Schema for structured LLM extraction |

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "team_id": "660e8400-e29b-41d4-a716-446655440001",
    "name": "my-crawl",
    "root_url": "https://example.com",
    "url": "https://example.com",
    "status": "queued",
    "config": { "max_depth": 2 },
    "total_tasks": 0,
    "completed_tasks": 0,
    "failed_tasks": 0,
    "created_at": "2025-01-15T12:00:00Z",
    "updated_at": "2025-01-15T12:00:00Z",
    "completed_at": null
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

> 响应为裸 `Crawl` 域模型（无 `credits_used` 字段，无 `task` 包装）。状态码：创建成功为 `201 Created`；`sync_wait_ms > 0` 且等待超时时为 `202 Accepted`。

#### Get Crawl Status

**Endpoint:** `GET /v1/crawl/{id}`

**Parameters:**
- `id` (path) - Crawl UUID

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "team_id": "660e8400-e29b-41d4-a716-446655440001",
    "name": "my-crawl",
    "root_url": "https://example.com",
    "url": "https://example.com",
    "status": "processing",
    "config": { "max_depth": 2 },
    "total_tasks": 100,
    "completed_tasks": 45,
    "failed_tasks": 2,
    "created_at": "2025-01-15T12:00:00Z",
    "updated_at": "2025-01-15T12:01:30Z",
    "completed_at": null
  },
  "timestamp": "2025-01-15T12:01:30+00:00"
}
```

> 响应为裸 `Crawl` 域模型（无 `task` / `progress` 包装）。`status` 取 `CrawlStatus` 序列化值（`queued` / `processing` / `completed` / `failed` / `cancelled`）。

#### Get Crawl Results

**Endpoint:** `GET /v1/crawl/{id}/results`

**Parameters:**
- `id` (path) - Crawl UUID

> **无查询参数**：该端点不接受 `page` / `limit`，一次返回该 crawl 下全部子任务的抓取结果。

**Response:**
```json
{
  "success": true,
  "data": [
    {
      "id": "770e8400-e29b-41d4-a716-446655440000",
      "task_id": "880e8400-e29b-41d4-a716-446655440001",
      "url": "https://example.com/page1",
      "status_code": 200,
      "content": "<html>...</html>",
      "content_type": "text/html",
      "headers": {},
      "meta_data": {},
      "screenshot": null,
      "response_time_ms": 1200,
      "created_at": "2025-01-15T12:00:05Z"
    }
  ],
  "timestamp": "2025-01-15T12:00:05+00:00"
}
```

> 响应 `data` 是裸 `Vec<ScrapeResult>` 数组（非分页包封）。

#### Cancel Crawl

Cancel a crawl task. All sub-tasks of the crawl are cancelled with it.

**Endpoint:** `DELETE /v1/crawl/{id}`

**Parameters:**
- `id` (path) - Crawl UUID

**Response:** `204 No Content` — 空 body，无 JSON 包封（`errors` 时返回标准错误包封：403 非属主 / 404 不存在 / 500 内部错误）。

---

### Search API

Search using various search engines.

#### Search

**Endpoint:** `POST /v1/search`

**Request Body:**
```json
{
  "engine": "smart",
  "query": "Rust web scraping",
  "limit": 10,
  "lang": "en",
  "country": "us",
  "sync_wait_ms": 5000
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `engine` | string | No | Search engine: `smart`（默认，多引擎 fan-out + RRF 聚合）、`bing`、`baidu`、`sogou`。未提供或 `smart` 时并发查询多个引擎并做融合排序与去重；显式引擎名走单引擎路径；未知引擎名返回 400 参数错误（不会静默替换）。`exa`/`tavily` 保留枚举值，需对应引擎注册才可用，未注册时返回 `NoEngineAvailable` |
| `query` | string | Yes | Search query |
| `sources` | array | No | `engine` 的别名（multi-source 写法） |
| `limit` | integer | No | Number of results (default: 10; silently capped at `settings.search.max_results`, default **50**) |
| `lang` | string | No | Search language — **当前版本接受但尚未生效（已知缺口）**：`SearchService::perform_search` 的 `_lang` 参数被忽略；默认 `en` 的 Bing mkt/Accept-Language 驱动尚未实现（baidu/sogou 路径亦不读取该值） |
| `country` | string | No | Search region — **当前版本接受但尚未生效（已知缺口）**：`_country` 参数被忽略，默认 `us` 亦未驱动任何请求参数 |
| `sync_wait_ms` | integer | No | 同步等待时长（毫秒，默认 5000）。**注意：** 无 DTO 级上限校验（不同于 scrape/crawl 的显式 `MAX_SYNC_WAIT_MS` 检查），超过 30000 亦照常进入等待轮询 |
| `crawl_results` | boolean | No | 为 true 时对每条搜索结果创建爬取任务 |
| `crawl_config` | object | No | 搜索结果异步爬取的配置（结构同爬取任务配置） |

> **0.2.0 行为变更**：`engine` 省略时从"单引擎 bing"改为 smart 聚合（fan-out + RRF + 跨来源去重）；
> 显式指定 `engine: "bing"` 等仍为单引擎路径。

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "query": "Rust web scraping",
    "results": [
      {
        "title": "Web Scraping with Rust",
        "url": "https://example.com/rust-scraping",
        "description": "Learn how to scrape websites using Rust",
        "engine": "bing"
      }
    ],
    "crawl_id": "550e8400-e29b-41d4-a716-446655440000",
    "credits_used": 5
  }
}
```

---

### Map API (new in 0.2.0)

Discover URLs from a site s's sitemap. Synchronous endpoint.

**Endpoint:** `POST /v1/map`

**Request Body:**
```json
{
  "url": "https://example.com",
  "include_patterns": ["*/blog/*"],
  "exclude_patterns": ["*/tag/*"],
  "limit": 1000
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `url` | string | Yes | Site origin; `{origin}/sitemap.xml` is fetched |
| `include_patterns` | string[] | No | Glob whitelist (`*`/`?`) applied first |
| `exclude_patterns` | string[] | No | Glob blacklist applied after include |
| `limit` | integer | No | Max URLs returned (default: 1000). No DTO-level cap — the overall total is bounded by `sitemap.max_urls` (default **50000**, `config/default.toml`) |

**Response:**
```json
{
  "success": true,
  "data": {
    "links": ["https://example.com/", "https://example.com/blog/post-1"]
  }
}
```

**Behavior notes:**
- Missing sitemap (404) returns 200 with empty `links` (valid state, not an error).
- Sitemap index (`<sitemapindex>`) is expanded with sub-sitemap fetches bounded by `max_concurrent_sub_sitemaps = 4` (`config/default.toml`), one level deep.

**Errors:** `400` invalid URL/params (`OriginUnparsable` / internal validation) · `502` `MAP_TARGET_UNREACHABLE` target unreachable or 5xx · unknown DTO fields rejected with `422` via `deny_unknown_fields`

---

### Extract API

> **Conditional Endpoint (R-teams-003):** This endpoint's signature depends on the `teams` feature:
> - **`teams` enabled (default):** `POST /v1/extract` accepts `Extension<Arc<TeamService>>` and `Extension<Arc<GR>>` (geo-restriction repository) parameters, enforcing team-scoped geo-restrictions before task creation.
> - **`teams` disabled:** `POST /v1/extract` degrades to a simpler signature without geo-restriction parameters; tasks are created directly against `DEFAULT_TEAM_ID` without geo checks.

Extract structured data from target pages. The endpoint creates an asynchronous extract task (it does **not** extract from inline HTML); results are fetched via the task status endpoints.

#### Extract Data

**Endpoint:** `POST /v1/extract`

**Request Body:**
```json
{
  "urls": ["https://example.com"],
  "prompt": "Extract the page title and all article links",
  "sync_wait_ms": 5000
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `urls` | string[] | Yes | Target URLs (1–100 entries, http/https only) |
| `prompt` (alias: `extraction_prompt`) | string | One of `prompt`/`schema`/`rules` required | Free-text LLM prompt describing what to extract |
| `schema` (alias: `extraction_schema`) | object | One of `prompt`/`schema`/`rules` required | JSON Schema for structured LLM extraction |
| `rules` (alias: `extraction_rules`) | object | One of `prompt`/`schema`/`rules` required | CSS selector extraction rules |
| `model` | string | No | Model identifier for LLM extraction |
| `sync_wait_ms` | integer | No | Synchronous wait in milliseconds (default 5000, max 30000; `> 30000` returns 400) |

Each `rules` entry (`ExtractionRule`) supports: `selector`, `attr` (attribute name; `None` extracts text), `is_array` (multiple values), `use_llm`, `llm_prompt`, `output_format` (`json` default / `plaintext`).

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "pending"
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

**Status codes:**

| Status | Meaning |
|--------|---------|
| `201` | Task enqueued, no `sync_wait_ms` (or 0) |
| `202` | `sync_wait_ms` elapsed with the task still not finished — the body still reports `status: "pending"` |
| `400` | `urls` empty, or none of `prompt`/`schema`/`rules` provided, or `sync_wait_ms > 30000` |
| `402` | Insufficient credits (`check_and_deduct_quota` failed; the task is not enqueued) |
| `403`/`500` | Geographic restriction denied / other server-side failure |

---

### Task API

Query and manage tasks. Task API follows RESTful conventions with action suffixes (`_query` for queries, `_cancel` for cancellations). Both endpoints are registered in the production router (`src/presentation/routes/task.rs::task_routes`, merged via `src/bootstrap/routes.rs::create_v2_routes_with_state`).

#### Query Tasks

**Endpoint:** `POST /v1/tasks/_query`

**Request Body:**
```json
{
  "task_ids": ["550e8400-e29b-41d4-a716-446655440000"],
  "task_types": ["scrape", "crawl", "extract"],
  "statuses": ["queued", "active", "completed", "failed", "cancelled"],
  "created_after": "2025-01-01T00:00:00Z",
  "created_before": "2025-01-15T00:00:00Z",
  "crawl_id": "770e8400-e29b-41d4-a716-446655440000",
  "limit": 20,
  "offset": 0,
  "include_results": false,
  "sync_wait_ms": 5000
}
```

> All fields are optional and flat (no `filters`/`pagination`/`sort` wrappers). Defaults: `limit` 100, `offset` 0, `include_results` false, `sync_wait_ms` 5000. `limit` range 1–1000 (0 or >1000 rejected with 422). `sync_wait_ms` range 0–30000.

**Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `task_ids` | array | Query specific task IDs |
| `task_types` (plural) | array | Filter by type: `scrape`, `crawl`, `extract` |
| `statuses` | array | Filter by status: `queued`, `active`, `completed`, `failed`, `cancelled` |
| `created_after` | string | Filter by creation date (RFC3339 with offset) |
| `created_before` | string | Filter by creation date (RFC3339 with offset) |
| `crawl_id` | string | All sub-tasks of a crawl |
| `limit` | integer | Results per page (default 100, max 1000) |
| `offset` | integer | Pagination offset (0-based; no `page` parameter) |
| `include_results` | boolean | Include task result payload |
| `sync_wait_ms` | integer | Synchronous poll wait (default 5000, max 30000) |

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "tasks": [
      {
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "task_type": "scrape",
        "status": "completed",
        "priority": 0,
        "url": "https://example.com",
        "attempt_count": 1,
        "max_retries": 3,
        "created_at": "2025-01-15T12:00:00+00:00",
        "started_at": "2025-01-15T12:00:01+00:00",
        "completed_at": "2025-01-15T12:00:03+00:00",
        "crawl_id": null,
        "result": null
      }
    ],
    "total": 150,
    "has_more": true
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

> 分页用 `limit`/`offset`：`has_more = offset + len(tasks) < total`。无 `page` 参数、无 `pagination` 包封。`status` 序列化为 `queued`/`active`/`completed`/`failed`/`cancelled`。

#### Cancel Tasks

**Endpoint:** `POST /v1/tasks/_cancel`

**Request Body:**
```json
{
  "task_ids": [
    "550e8400-e29b-41d4-a716-446655440000",
    "660e8400-e29b-41d4-a716-446655440001"
  ],
  "force": false,
  "sync_wait_ms": 5000
}
```

> `task_ids` is required and must be non-empty (empty list → 422). `force` (default false) cancels tasks even while they are executing. `sync_wait_ms` (default 5000, max 30000) polls until cancellation settles.

**Response:**
```json
{
  "success": true,
  "data": {
    "cancelled_tasks": [
      {
        "task_id": "550e8400-e29b-41d4-a716-446655440000",
        "status": "cancelled",
        "cancelled_at": "2025-01-15T12:00:05+00:00"
      }
    ],
    "failed_tasks": [
      {
        "task_id": "660e8400-e29b-41d4-a716-446655440001",
        "reason": "Already completed"
      }
    ],
    "total_cancelled": 1,
    "total_failed": 1
  },
  "timestamp": "2025-01-15T12:00:05+00:00"
}
```

---

### Team API

> **Conditional Endpoints (R-teams-002):** All `/v1/teams/*` endpoints are only available when the `teams` feature is enabled (default). When `teams` is disabled, these routes are not registered and return 404 Not Found. In single-tenant mode (`teams` disabled), all requests are attributed to `DEFAULT_TEAM_ID` (`domain::id::Id::from_u128(1)`), making team management endpoints unnecessary.

#### Get Current Team

**Endpoint:** `GET /v1/teams/me`

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "770e8400-e29b-41d4-a716-446655440000",
    "name": "Team",
    "credits_balance": 1000,
    "total_tasks": 50,
    "completed_tasks": 0,
    "failed_tasks": 0,
    "created_at": "2025-01-15T12:00:00Z"
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

> `total_tasks` counts tasks found via `find_by_crawl_id(team_id)`; `completed_tasks`/`failed_tasks` are currently fixed `0`（handler 统计尚未接通）。`credits_balance` 查询失败时回退为 `0`（不报错）。

#### Get Team Usage

**Endpoint:** `GET /v1/teams/me/usage`

**Response:**
```json
{
  "success": true,
  "data": {
    "team_id": "770e8400-e29b-41d4-a716-446655440000",
    "period": "30d",
    "total_requests": 0,
    "successful_requests": 0,
    "failed_requests": 0,
    "credits_used": 1234,
    "avg_response_time_ms": 123.45
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

> `period` 固定 `"30d"`。`total_requests`/`successful_requests`/`failed_requests` 当前恒为 `0`（handler 统计尚未接通）；`credits_used` 取积分余额绝对值（负余额表示超支）。无 `credits_limit` / `requests_today` / `requests_limit` 字段。`avg_response_time_ms` 为该团队抓取结果平均响应时间。

#### Get Team Geo Restrictions

**Endpoint:** `GET /v1/teams/geo-restrictions`

**Response:**
```json
{
  "success": true,
  "data": {
    "team_id": "770e8400-e29b-41d4-a716-446655440000",
    "enable_geo_restrictions": true,
    "allowed_countries": ["US", "CA"],
    "blocked_countries": ["CN", "RU"],
    "ip_whitelist": ["10.0.0.0/8"],
    "domain_blacklist": ["evil.com"]
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

> 响应为平铺的 `TeamGeoRestrictionsResponse`（无 `restrictions` 外层包装）。字段名是 `enable_geo_restrictions`（不是 `enabled`）；未设置的列表字段序列化为 `null`。

#### Update Team Geo Restrictions

**Endpoint:** `PUT /v1/teams/geo-restrictions`

**Request Body:**
```json
{
  "enable_geo_restrictions": true,
  "allowed_countries": ["US", "CA"],
  "blocked_countries": ["CN", "RU"],
  "ip_whitelist": ["10.0.0.0/8"],
  "domain_blacklist": ["evil.com"]
}
```

> `enable_geo_restrictions` **必填**（bool，无缺省值）。国家代码必须为 2 字母 ISO 3166-1 alpha-2，`ip_whitelist` 支持 IP 或 CIDR——校验失败返回 `400 VALIDATION_ERROR`。`deny_unknown_fields` 拒绝未知字段（422）。allowed/blocked 列表若提供则不能为空数组。

**Response:**
```json
{
  "success": true,
  "data": {
    "team_id": "770e8400-e29b-41d4-a716-446655440000",
    "enable_geo_restrictions": true,
    "allowed_countries": ["US", "CA"],
    "blocked_countries": ["CN", "RU"],
    "ip_whitelist": ["10.0.0.0/8"],
    "domain_blacklist": ["evil.com"]
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

> 响应为更新后的 restrictions 对象本身（非 `{message}` 包封）。

---

### Webhook API

> **Conditional Endpoints (R-wh-001):** All `/v1/webhooks/*` endpoints are only available when the `webhook` feature is enabled (default). When `webhook` is disabled, these routes are not registered and return 404 Not Found. Additionally, the `webhook_worker` is not spawned, and `NoopWebhookService` is injected (task completion/failure notifications become no-ops).

#### List Webhooks

**Endpoint:** `GET /v1/webhooks`

> **无查询参数**：一次返回该团队全部 webhook（无 `page` / `limit`）。列表响应**不含** `secret` 字段。

**Response:**
```json
{
  "success": true,
  "data": {
    "webhooks": [
      {
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "team_id": "660e8400-e29b-41d4-a716-446655440001",
        "url": "https://your-webhook.com/callback",
        "created_at": "2026-01-15T00:00:00Z",
        "is_active": true
      }
    ],
    "total": 5
  },
  "timestamp": "2026-01-15T00:00:00+00:00"
}
```

#### Create Webhook

**Endpoint:** `POST /v1/webhooks`

**Request Body:**
```json
{
  "url": "https://your-webhook.com/callback"
}
```

> 请求体经 `#[serde(deny_unknown_fields)]` 校验：`CreateWebhookRequest` 仅接受 `url`，传入 `events` / `secret` / `active` 等未知字段会被拒绝并返回 **400 `VALIDATION_ERROR`**（handler 以 `serde_json::from_slice` 解析后映射为 `CrawlRsError::Validation`）。此外，创建请求本身必须携带按服务端全局密钥（`CRAWLRS__WEBHOOK__SECRET`）计算的 Standard Webhooks 签名头（见 [Webhook Signature](#webhook-signature)），签名验证失败返回 **401 `AUTHENTICATION_ERROR`**。

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `url` | string | Yes | Webhook callback URL (http/https) |

**Events（服务端自动按任务类型派发，无需订阅）:**
- `scrape.completed` - Scrape task completed
- `scrape.failed` - Scrape task failed
- `crawl.completed` - Crawl task completed
- `crawl.failed` - Crawl task failed
- `extract.completed` / `extract.failed` - Extract task completed/failed

**Response (Success):** `201 Created`，标准 `{"success":true,"data":{...}}` 包封。**创建响应返回 `secret`**（各 webhook 的签名密钥由服务端按 `settings.webhook.secret` 统一派生，仅在本次创建响应中返回；列表端点不回传）。

```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "team_id": "660e8400-e29b-41d4-a716-446655440001",
    "url": "https://your-webhook.com/callback",
    "created_at": "2026-01-15T00:00:00Z",
    "is_active": true,
    "secret": "your-webhook-signing-secret"
  },
  "timestamp": "2026-01-15T00:00:00+00:00"
}
```

---

### Audit API

#### Get Audit Logs

**Endpoint:** `GET /v1/audit/logs`

**Query Parameters:**
- `limit` - Results per page (default `server_config::DEFAULT_PAGE_LIMIT`, capped at `MAX_PAGE_LIMIT`)
- `offset` - Pagination offset (default 0)
- `api_key_id` - Filter by API key (non-admin callers may only pass their own key; otherwise 403)
- `team_id` - Filter by team (non-admin callers may only pass their own team; otherwise 403)

> 无 `event_type` / `start_time` / `end_time` / `page` 参数。缺省查询调用方自身 `api_key_id` 的日志；admin scope 可查任意 key/team。IDOR 防护见 `audit_handler.rs`。

**Response:**
```json
{
  "success": true,
  "data": {
    "logs": [
      {
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "api_key_id": "770e8400-e29b-41d4-a716-446655440000",
        "team_id": "660e8400-e29b-41d4-a716-446655440001",
        "requested_action": "GET /v1/scrape",
        "decision": "Allow",
        "denial_reason": null,
        "scope_used": null,
        "ip_address": "192.168.1.1",
        "trace_id": null,
        "user_agent": "Mozilla/5.0...",
        "request_path": "/v1/scrape",
        "request_method": "GET",
        "metadata": {},
        "created_at": "2025-01-15T00:00:00Z"
      }
    ]
  },
  "timestamp": "2025-01-15T00:00:00+00:00"
}
```

> 响应仅含 `logs` 数组（`AuditLogEntry`），无 `pagination` 包封。`decision` 序列化为 `"Allow"` / `"Deny"`。

#### Get Denied Requests

**Endpoint:** `GET /v1/audit/denied`

**Query Parameters:**
- `limit` - Results per page (default `server_config::DEFAULT_PAGE_LIMIT`, capped at `MAX_PAGE_LIMIT`)

> 仅支持 `limit`；不支持 `reason` / `start_time` / `end_time` / `page`。始终查询调用方自身 `api_key_id` 的拒绝记录（避免 IDOR）。

**Response:**
```json
{
  "success": true,
  "data": {
    "denied_requests": [
      {
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "api_key_id": "770e8400-e29b-41d4-a716-446655440000",
        "team_id": "660e8400-e29b-41d4-a716-446655440001",
        "requested_action": "POST /v1/scrape",
        "decision": "Deny",
        "denial_reason": "rate_limit_exceeded",
        "scope_used": null,
        "ip_address": "192.168.1.1",
        "trace_id": null,
        "user_agent": "Mozilla/5.0...",
        "request_path": "/v1/scrape",
        "request_method": "POST",
        "metadata": {},
        "created_at": "2025-01-15T00:00:00Z"
      }
    ]
  },
  "timestamp": "2025-01-15T00:00:00+00:00"
}
```

> 响应仅含 `denied_requests` 数组（同为 `AuditLogEntry`，`decision` 为 `"Deny"`），无 `pagination` 包封。

---

### Admin API

> **0.2.0 新增（`garrison-auth-migration`）：** 管理员可通过此端点签发新的 garrison API Key。要求调用方持 `crawlrs:admin` 权限（即 `admin` scope）。

#### Issue API Key

为指定 team 签发新的 garrison API Key。明文 key 仅返回一次，响应头含 `Cache-Control: no-store` / `Pragma: no-cache` / `Expires: 0` 防止缓存。

**Endpoint:** `POST /v1/admin/api-keys`

**Required Scope:** `admin`（对应 garrison 权限 `crawlrs:admin`）

**Request Body:**
```json
{
  "team_id": "770e8400-e29b-41d4-a716-446655440000",
  "scopes": ["read", "write", "admin"],
  "expires_in_secs": 2592000
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `team_id` | string (UUID) | Yes | 该 API Key 所属的 team ID |
| `scopes` | array<string> | Yes | scope 列表，可选值：`read` / `write` / `admin`（handler 自动映射为 garrison 权限 `crawlrs:read` / `crawlrs:write` / `crawlrs:admin`） |
| `expires_in_secs` | integer | No | 过期秒数（默认 2592000 = 30 天） |

**Request Example:**
```bash
curl -X POST http://localhost:8899/v1/admin/api-keys \
  -H "Authorization: Bearer garrison_admin_key_id.garrison_admin_secret" \
  -H "Content-Type: application/json" \
  -d '{
    "team_id": "770e8400-e29b-41d4-a716-446655440000",
    "scopes": ["read", "write"],
    "expires_in_secs": 2592000
  }'
```

**Response Headers:**
```
Cache-Control: no-store
Pragma: no-cache
Expires: 0
```

**Response (201 Created):**
```json
{
  "success": true,
  "data": {
    "api_key": "garrison_key_id.garrison_secret",
    "api_key_id": "880e8400-e29b-41d4-a716-446655440000",
    "team_id": "770e8400-e29b-41d4-a716-446655440000",
    "scopes": ["read", "write"],
    "expires_at": "2025-08-20T12:00:00+00:00"
  },
  "timestamp": "2025-07-21T12:00:00+00:00"
}
```

> **注：** 响应中 `scopes` 字段回显请求时传入的 scope（`read`/`write`/`admin`），实际 garrison 权限串（`crawlrs:read` 等）由 handler 在签发时映射，调用方无需关心。`expires_at` 为 RFC3339 过期时间（缺省 30 天；`expires_in_secs` 必须 > 0 且 ≤ 1 年，`scopes` 最多 10 个元素）。

> **⚠️ 安全提示：** `api_key` 字段是明文 key，仅在本次响应中返回一次。请立即妥善保存（如写入 secrets manager），后续无法再次查询。若丢失需重新签发。

**Error Codes:**

| Error Code | HTTP Status | Description |
|------------|-------------|-------------|
| `UNAUTHORIZED` | 401 | 调用方未认证或 garrison key 无效 |
| `FORBIDDEN` | 403 | 调用方缺少 `crawlrs:admin` 权限（handler 内 `has_permission(Admin)` 校验） |
| `VALIDATION_ERROR` | 400 | 请求值校验失败（`team_id` 为 nil UUID、`scopes` 为空/含未知权限、`expires_in_secs` 越界） |
| `UNPROCESSABLE_ENTITY` | 422 | 请求体结构错误（如 `team_id` 非 UUID 的 JSON 数据错误、字段缺失/类型不符） |
| `INTERNAL_ERROR` | 500 | `team_id` 在 teams 表不存在、garrison DAO 未注入或 `ApiKeyHandler::generate_with_namespace` 内部错误 |

> **注意：** `auth` feature 未启用时路由不注册，请求得到普通 404（`NOT_FOUND`），不存在 `FEATURE_DISABLED` 响应码。

---

## 🎭 Session Identity & Cookie Semantics

When `[session].enabled = true` (default: `false`), the engine router binds consecutive
requests to a session identity (proxy + UA/TLS profile + cookie cache).

- **Cookie scope**: response `Set-Cookie` values are captured per target host and only
  replayed to requests targeting the **same host**. Cookies captured from site A are never
  injected into requests to site B, even when requests share one `session_id` across hosts.
- **Known limitations of the manual cookie channel**: no `expiry`/`path` attribute handling
  (short-lived cookies may be replayed past expiration; only the first `name=value` pair of
  each `Set-Cookie` is kept). Disable `[session].enabled` if the target site relies on
  strict cookie semantics. Browser engines (Playwright/MLLM) manage cookies in their own
  context and do not use this channel.

### XHR Capture (`capture_xhr`)

When `CRAWLRS__ENGINES__PLAYWRIGHT__CAPTURE_XHR=true`, the Playwright engine can capture
background XHR/Fetch responses:

- Captured data is persisted with the task result under `meta_data.captured_responses`
  (read it via the task result endpoint). The `POST /v1/scrape` enqueue response itself
  does not carry capture data (`captured_responses` is always `null` there).
- XHR/Fetch URLs pointing to loopback/private/link-local addresses are discarded (SSRF
  protection). Configure `capture_xhr_url_patterns` as a whitelist to limit what gets
  captured.

---

## ⏱️ Rate Limiting

> **Conditional Behavior (R-rl-003):** Rate limiting is only active when the `rate-limit` feature is enabled (default). When disabled, `NoopRateLimitingService` is injected: `check_rate_limit` returns `Allowed`, `check_and_deduct_quota` returns `Ok(())`, `get_quota_balance` returns `Ok(i64::MAX)`. All requests are allowed without limit, and quota deductions are no-ops.

The API implements rate limiting at multiple levels:

1. **Per-API Key Rate Limit** - Limits requests per API key
2. **Per-Team Concurrency Limit** - Limits concurrent requests per team
3. **Global Rate Limit** - System-wide protection

### Rate Limit Headers

**当前实现仅设一个响应头：** 命中限流返回 429 时设 `Retry-After`。**不存在 `X-RateLimit-Limit` / `X-RateLimit-Remaining` / `X-RateLimit-Reset` 响应头**（全库无发射点，读取这些头的客户端逻辑应移除）。

```
Retry-After: 60
```

响应体示例见上方 [Rate Limit Error Response](#rate-limit-error-response)：标准 `ApiResponse` 包封（`error.code = "RATE_LIMITED"`），`RetryAfter` 结果额外带 `retry_after_seconds`。

> 附加语义：限流服务本身出错时默认 fail-closed（返回 503 `SERVICE_UNAVAILABLE`）；设 `RATE_LIMIT_FAIL_OPEN=true` 可改为放行并记录 warn 日志。

---

## 🪝 Webhooks

Webhooks allow you to receive notifications about task events.

### Webhook Payload Format

投递的 payload 为包含 `task_id`、`status`、`url`、`timestamp`（Unix 秒）的 JSON 对象，失败时附带 `error`：

```json
{
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "completed",
  "url": "https://example.com",
  "timestamp": 1760000000
}
```

事件类型由任务类型决定（`scrape.completed` / `scrape.failed` / `crawl.completed` / `crawl.failed` / `extract.completed` / `extract.failed`）。

### Webhook Signature

签名遵循 [Standard Webhooks](https://www.standardwebhooks.com/) 规范，投递携带三个头：

```
webhook-id: <msg_id>
webhook-timestamp: <unix_seconds>
webhook-signature: v1,<base64-encoded-hmac-sha256>
```

签名消息为 `{msg_id}.{timestamp}.{payload}`（三段以句点拼接），使用服务端配置的密钥（`CRAWLRS__WEBHOOK__SECRET`）计算 HMAC-SHA256。验证时用 `webhook-id` 与 `webhook-timestamp` 头的值重算并对比 `webhook-signature`（推荐使用官方 `standardwebhooks` 库，恒时比较）。

---

## 🧩 SDK API

SDK endpoints provide simplified interfaces for common operations, wrapping the underlying REST API. All SDK endpoints return the payload object directly (no `success`/`data` envelope).

> **路径解析：** SDK 路由由 sdforge `#[forge]` 宏注册（`version = "v1"` + `path = "/sdk/..."`），sdforge 默认前缀为 `/api/{version}`，因此实际对外路径统一为 `/api/v1/sdk/*`。SDK 路由同样经过认证中间件（`team_id`/`api_key_id` 从 `AuthState` 提取，绝不信任请求体）。

### SDK Search

**Endpoint:** `POST /api/v1/sdk/search`

**Request Body:**
```json
{
  "query": "Rust web scraping",
  "limit": 10
}
```

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | Yes | Search query (non-empty) |
| `limit` | integer | No | Max results |

**Response:**
```json
{
  "query": "Rust web scraping",
  "results": [
    {
      "title": "...",
      "url": "https://...",
      "description": "...",
      "engine": "bing"
    }
  ],
  "credits_used": 5
}
```

### SDK Tasks

Create a queued task (`task_type`: `scrape` or `crawl`).

**Endpoint:** `POST /api/v1/sdk/tasks`

**Request Body:**
```json
{
  "url": "https://example.com",
  "task_type": "scrape"
}
```

**Response:**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "url": "https://example.com",
  "status": "Queued"
}
```

### SDK Scrape

**Endpoint:** `POST /api/v1/sdk/scrape`

**Request Body:**
```json
{
  "url": "https://example.com"
}
```

**Response:**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "url": "https://example.com"
}
```

### SDK Crawl

**Endpoint:** `POST /api/v1/sdk/crawl`

**Request Body:**
```json
{
  "name": "my-crawl",
  "url": "https://example.com",
  "seed_url": "https://example.com"
}
```

**Response:**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "Queued",
  "url": "https://example.com"
}
```

---

## 💻 SDK Examples

### JavaScript/Node.js

```javascript
const axios = require('axios');

const client = axios.create({
  baseURL: 'http://localhost:8899',
  headers: {
    'Authorization': 'Bearer YOUR_API_KEY'
  }
});

const scrape = async (url) => {
  const response = await client.post('/v1/scrape', {
    url: url,
    formats: ['markdown'],
    sync_wait_ms: 5000
  });
  return response.data;
};
```

### Python

```python
import requests

client = requests.Session()
client.headers.update({
  'Authorization': 'Bearer YOUR_API_KEY'
})

def scrape(url):
  response = client.post('http://localhost:8899/v1/scrape', json={
    'url': url,
    'formats': ['markdown'],
    'sync_wait_ms': 5000
  })
  return response.json()
```

### Go

```go
package main

import (
  "bytes"
  "encoding/json"
  "net/http"
)

func Scrape(url string) error {
  client := &http.Client{}
  body := map[string]interface{}{
    "url": url,
    "formats": []string{"markdown"},
    "sync_wait_ms": 5000,
  }

  jsonData, _ := json.Marshal(body)
  req, _ := http.NewRequest("POST", "http://localhost:8899/v1/scrape", bytes.NewBuffer(jsonData))
  req.Header.Set("Authorization", "Bearer YOUR_API_KEY")
  req.Header.Set("Content-Type", "application/json")

  resp, err := client.Do(req)
  return err
}
```

---

## ✅ Best Practices

1. **Use Sync Mode Sparingly** - Only use `sync_wait_ms` when you need immediate results
2. **Implement Retry Logic** - Handle rate limits with exponential backoff
3. **Use Webhooks** - Prefer webhooks over polling for task status
4. **Set Timeouts** - Always configure appropriate timeouts
5. **Monitor Credits** - Track credit usage to avoid service interruption
6. **Handle Errors Gracefully** - Check both HTTP status and response `success` field
7. **Validate Inputs** - Validate URLs and parameters before sending requests
8. **Use Caching** - Enable oxcache for frequently accessed content
9. **Set Proper Rates** - Configure rate limits appropriate for your capacity
10. **Secure Webhooks** - Always verify webhook signatures

---

## 📋 Changelog

### Unreleased

**Changed:**
- ID 体系全量迁移至 NebulaId（`domain::id::Id`，默认 `uuid_v8` 算法；API JSON 契约保持 UUID 字符串不变）
- `/v1/search` 无 `engine` 参数时行为从"单引擎 bing"变为 smart 聚合（多引擎 fan-out + RRF 融合）
- auth-off 语义：protected routes 默认 401 拒绝，须显式 `allow_unauthenticated_protected()` opt-in；opt-in 后注入 `ApiKeyScope::denied()` 受限匿名身份（`DEFAULT_TEAM_ID`/`DEFAULT_API_KEY_ID`），非 `full_access()`

**New Engines / Algorithms:**
- TLS Fingerprint Engine (`engine-tls-fingerprint`): WreqEngine based on BoringSSL, real JA3/JA4 fingerprint spoofing
- MLLM Autonomous Navigation Engine (`engine-mllm`): Vision LLM agentic loop for autonomous page navigation
- RAG 增强提取、知识图谱覆盖感知爬取、DRL 自适应爬取策略

**Observability:**
- 5 个新 Prometheus 指标：`crawlrs_queue_depth` / `crawlrs_engine_success_total` / `crawlrs_engine_duration_seconds` / `crawlrs_cache_hit_total` / `crawlrs_webhook_delivery_total`

### 0.2.0-rc.2 (2026-09-21)
- 自研依赖全面切换 crates.io registry（garrison → 0.9.0-rc.2 等），移除跨仓 path 依赖

### 0.2.0-rc.1 (2026-08-25)
- auth-off 安全语义收紧：protected routes 默认 401，`allow_unauthenticated_protected()` 显式 opt-in（该收紧为现行语义，见上方 Auth Disabled Behavior 节）

### 0.2.0 (2026-07-29)
- **`garrison-auth-migration`：** 认证引擎由 garrison 接管，Bearer token 格式改为 `garrison_key_id.garrison_secret`
- 新增 `POST /v1/admin/api-keys` 端点：管理员为指定 team 签发 garrison API Key
- 401/429 触发逻辑由 garrison `firewall-bruteforce` 接管（5 次/60 秒/300 秒锁定）
- Scrape API `options` 新增 `cache_mode`（4 种缓存读写模式：`enabled` / `disabled` / `read_only` / `bypass`）和 `bypass_cache`（应急缓存绕过）字段
- Added `GET /v1/webhooks` to list webhooks
- Added `GET /v1/teams/me` and `GET /v1/teams/me/usage` team endpoints
- Added SDK API section (`/api/v1/sdk/*` endpoints)
- Merged Task API sections into single unified section

### 0.1.0 (2026-07-22)
- Initial release
- Scrape, Crawl, Search, Extract APIs
- Rate limiting and concurrency control
- Webhook support
- Audit logging
- Metrics export

---

## 📞 Support

For questions or issues:
- 📖 [Documentation](../README.md)
- 🐛 [Issue Tracker](https://github.com/Kirky-X/crawlrs/issues)
- 📧 Email: Kirky-X@outlook.com
