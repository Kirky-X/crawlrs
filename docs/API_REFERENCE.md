
# 📡 Complete REST API Documentation

<div align="center">

![API Version](https://img.shields.io/badge/api-0.2.0-blue)
![Base URL](https://img.shields.io/badge/base%20URL-http://localhost:8899-green)
![License](https://img.shields.io/badge/license-Apache%202.0-orange)

**Version:** 0.2.0 | **Base URL:** `http://localhost:8899` | **Updated:** 2025-07-21

</div>

---

## 📖 Table of Contents

- [Authentication](#authentication)
- [Common Response Format](#common-response-format)
- [Errors](#errors)
- [Public Endpoints](#public-endpoints)
  - [Health Check](#health-check)
  - [Get Version](#get-version)
  - [Get Metrics](#get-metrics)
- [Protected Endpoints](#protected-endpoints)
  - [Scrape API](#scrape-api)
  - [Crawl API](#crawl-api)
  - [Search API](#search-api)
  - [Extract API](#extract-api)
  - [Task API](#task-api)
  - [Team API](#team-api)
  - [Webhook API](#webhook-api)
  - [Audit API](#audit-api)
  - [Admin API](#admin-api)
- [Rate Limiting](#rate-limiting)
- [Webhooks](#webhooks)
- [SDK API](#sdk-api)
- [SDK Examples](#sdk-examples)
- [Best Practices](#best-practices)
- [Changelog](#changelog)
- [Support](#support)

---

## Authentication

All protected endpoints require authentication using an API key in the `Authorization` header:

```http
Authorization: Bearer YOUR_API_KEY
```

> **0.2.0 起（`garrison-auth-migration`）：** Bearer token 由 **garrison v0.8.1** 校验，格式为 `garrison_key_id.garrison_secret`（明文 key 仅在签发时返回一次）。旧的 SHA-256 `api_key_hash` 已作废，需经 garrison 重新领取。

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

API keys can have different scopes that control access to specific features:

| Scope | Description |
|--------|-------------|
| `scrape` | Access to scrape endpoints |
| `crawl` | Access to crawl endpoints |
| `search` | Access to search endpoints |
| `extract` | Access to extract endpoints |
| `admin` | Full administrative access |

> **0.2.0 起**：scope 由 garrison RBAC 权限经 `auth_bridge::map_perms_to_scope` 桥接而来，下游业务层无感知。

### Auth Disabled Behavior (R-flags-005)

> **Conditional:** When the `auth` feature is disabled (via `--no-default-features` or explicit feature selection), API Key authentication is bypassed. The `default_identity_middleware` injects a fixed `AuthState` into all requests.

| Behavior | When `auth` Enabled (default) | When `auth` Disabled |
|----------|------------------------------|---------------------|
| `Authorization` header | Required (401 if missing/invalid; 429 on brute-force lockout) | Ignored — requests pass through |
| `AuthState.team_id` | Resolved via garrison → `api_key_id→team_id` 反查 + `TEAM_ID_CACHE` LRU | Fixed to `DEFAULT_TEAM_ID` (`Uuid::from_u128(1)`) |
| `AuthState.api_key_id` | Resolved from garrison key | Fixed to `DEFAULT_API_KEY_ID` (`Uuid::from_u128(2)`) |
| `AuthState.scope` | `map_perms_to_scope(garrison permissions)` | `ApiKeyScope::full_access()` (all permissions `true`, limits `u32::MAX`) |
| Token cache | garrison 自管 oxcache；crawlrs 侧 `TEAM_ID_CACHE` 缓存映射 | N/A — fixed identity, no caching |
| Rate limit lockout | garrison `firewall-bruteforce` (5/60s/300s) | Not tracked (but `rate-limit` feature may still apply per-IP limits) |

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

**运维重签流程：**

```bash
# 1. 构建带 admin-tools + auth 的二进制
cargo build --release --features admin-tools,auth

# 2. 运行 reissue_api_keys 工具：
#    - 预置 garrison RBAC（3 权限 / 3 角色 / 6 角色权限映射）
#    - 枚举需重签 team 清单
#    - 为每个 team 签发新 garrison API Key（明文 key 仅打印一次）
./target/release/reissue_api_keys

# 3. 应用数据库迁移（标记旧表弃用，不删表不删列）
psql -f migrations/005_deprecate_legacy_api_keys.sql
```

详细架构说明参见 [ARCHITECTURE.md → Garrison 认证引擎](ARCHITECTURE.md#garrison-认证引擎)。

---

## Common Response Format

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
  "meta": {
    "page": 1,
    "per_page": 20,
    "total_items": 100,
    "total_pages": 5,
    "has_next": true,
    "has_previous": false
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

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

```json
{
  "success": false,
  "error": {
    "code": "RATE_LIMITED",
    "message": "Rate limit exceeded"
  },
  "retry_after_seconds": 60,
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `success` | boolean | Whether the request was successful |
| `data` | object | Response data (only present on success) |
| `error` | object | Error details (only present on failure) |
| `error.code` | string | Error code for programmatic handling |
| `error.message` | string | Human-readable error message |
| `meta` | object | Pagination metadata (only for list responses) |
| `timestamp` | string | Response timestamp in RFC3339 format |

---

## Errors

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

| Error Code | HTTP Status | Description |
|------------|-------------|-------------|
| `VALIDATION_ERROR` | 400 | Invalid request parameters |
| `NOT_FOUND` | 404 | Resource not found |
| `UNAUTHORIZED` | 401 | Missing or invalid API key |
| `FORBIDDEN` | 403 | Insufficient permissions |
| `RATE_LIMITED` | 429 | Rate limit exceeded |
| `CONFLICT` | 409 | Resource conflict |
| `PRECONDITION_FAILED` | 412 | Precondition failed |
| `UNPROCESSABLE_ENTITY` | 422 | Validation error |
| `INTERNAL_ERROR` | 500 | Internal server error |
| `SERVICE_UNAVAILABLE` | 503 | Service unavailable |
| `DATABASE_ERROR` | 500 | Database error |
| `CACHE_ERROR` | 500 | Cache error |
| `EXTERNAL_SERVICE_ERROR` | 502 | External service error |
| `TIMEOUT` | 504 | Request timeout |
| `QUOTA_EXCEEDED` | 402 | Quota exceeded |
| `FEATURE_DISABLED` | 404 | Feature not enabled (route not registered) |

---

## Public Endpoints

### Health Check

Check if the API is running.

**Endpoint:** `GET /health`

**Response:**
```json
{
  "status": "healthy"
}
```

### Get Version

Get the current API version.

**Endpoint:** `GET /v1/version`

**Response:**
```text
0.2.0
```

### Get Metrics

Get system performance metrics (requires `metrics` feature).

**Endpoint:** `GET /metrics`

**Response:**
```text
# Prometheus metrics format
api_requests_total{method="POST",endpoint="/v1/scrape"} 1234
api_request_duration_seconds{method="POST",endpoint="/v1/scrape",quantile="0.5"} 0.045
```

---

## Protected Endpoints

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
      "attribute": "text"
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
| `actions` | array | No | Page interaction actions |
| `options` | object | No | Scraping options (see [Options Parameters](#scrape-options-parameters)) |
| `metadata` | object | No | Custom metadata for the task |
| `sync_wait_ms` | integer | No | Wait time for synchronous response (max 30000) |

<a id="scrape-options-parameters"></a>

**Options Parameters:**

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `headers` | object | empty | Custom HTTP headers (e.g. `User-Agent`) |
| `wait_for` | integer | — | **0.2.0 reserved field** (ignored by handler). Playwright engine internally uses conditional wait strategy (`WaitFor::NetworkIdle` by default); see [ARCHITECTURE.md → WaitFor](ARCHITECTURE.md#waitfor-策略) for the internal enum and planned API surface. |
| `timeout` | integer | 30 | Request timeout in seconds |
| `js_rendering` | boolean | false | Enable JavaScript rendering |
| `screenshot` | boolean | false | Capture screenshot |
| `screenshot_options` | object | — | Screenshot config (`full_page`, `quality`, `format`) |
| `mobile` | boolean | false | Emulate mobile device |
| `proxy` | string | — | Proxy URL (e.g. `http://proxy:8080`) |
| `skip_tls_verification` | boolean | false | Skip TLS certificate verification |
| `needs_tls_fingerprint` | boolean | false | Require TLS fingerprint adversarial engine |
| `use_fire_engine` | boolean | false | Force use FlareSolverr engine |
| `cache_mode` | string | `"enabled"` | Cache read/write mode: `enabled` (default, normal R/W), `disabled` (cache fully off), `read_only` (hit returns, miss fetches without writing), `bypass` (skip read, normal write — emergency untrusted-cache scenario) |
| `bypass_cache` | boolean | false | Emergency cache bypass shortcut. `true` overrides `cache_mode` to `bypass` (skip cache read, normal write-back). Use when runtime cache data is untrusted |
| `block_ads` | boolean | false | **0.2.0** Block ad/tracker domain requests via CDP interception (browser engines only). Matches `intercept::AD_DOMAIN_BLACKLIST` |
| `block_media` | boolean | false | **0.2.0** Block media resources (image/media/font) via CDP interception (browser engines only) |

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
    "credits_used": 10
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

#### Get Scrape Status

**Endpoint:** `GET /v1/scrape/{id}`

**Parameters:**
- `id` (path) - Task UUID

**Response:**
```json
{
  "success": true,
  "data": {
    "task": {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "status": "completed",
      "url": "https://example.com",
      "result": {
        "html": "...",
        "markdown": "...",
        "text": "..."
      }
    }
  }
}
```

#### Cancel Scrape

**Endpoint:** `POST /v1/scrape/{id}/_cancel`

**Parameters:**
- `id` (path) - Task UUID

**Response:**
```json
{
  "success": true,
  "data": {
    "message": "Scrape task cancelled"
  }
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
  "max_depth": 2,
  "max_pages": 100,
  "follow_links": true,
  "include_patterns": ["/blog/.*"],
  "exclude_patterns": ["/admin/.*"],
  "formats": ["markdown"],
  "webhook": "https://your-webhook.com/callback",
  "options": {
    "timeout": 30,
    "js_rendering": false,
    "proxy": "http://proxy.example.com:8080"
  },
  "sync_wait_ms": 10000
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `url` | string | Yes | Starting URL |
| `max_depth` | integer | No | Maximum crawl depth (default: 1) |
| `max_pages` | integer | No | Maximum pages to crawl |
| `follow_links` | boolean | No | Follow links on pages (default: true) |
| `include_patterns` | array | No | Regex patterns for URLs to include |
| `exclude_patterns` | array | No | Regex patterns for URLs to exclude |
| `formats` | array | No | Output formats |
| `webhook` | string | No | Webhook URL for notifications |
| `options` | object | No | Scraping options |
| `sync_wait_ms` | integer | No | Wait time for synchronous response |

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "url": "https://example.com",
    "credits_used": 50
  }
}
```

#### Get Crawl Status

**Endpoint:** `GET /v1/crawl/{id}`

**Parameters:**
- `id` (path) - Task UUID

**Response:**
```json
{
  "success": true,
  "data": {
    "task": {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "status": "running",
      "url": "https://example.com",
      "progress": {
        "pages_processed": 45,
        "total_pages": 100
      }
    }
  }
}
```

#### Get Crawl Results

**Endpoint:** `GET /v1/crawl/{id}/results`

**Parameters:**
- `id` (path) - Task UUID

**Query Parameters:**
- `page` - Page number (default: 1)
- `limit` - Results per page (default: 20, max: 100)

**Response:**
```json
{
  "success": true,
  "data": {
    "results": [
      {
        "url": "https://example.com/page1",
        "html": "...",
        "markdown": "..."
      }
    ],
    "pagination": {
      "page": 1,
      "limit": 20,
      "total": 100
    }
  }
}
```

#### Cancel Crawl

Cancel a crawl task. Supports both POST and DELETE methods.

**Endpoint:** `POST /v1/crawl/{id}/_cancel`

**Endpoint:** `DELETE /v1/crawl/{id}`

**Parameters:**
- `id` (path) - Task UUID

**Response:**
```json
{
  "success": true,
  "data": {
    "message": "Crawl task cancelled"
  }
}
```

---

### Search API

Search using various search engines.

#### Search

**Endpoint:** `POST /v1/search`

**Request Body:**
```json
{
  "engine": "google",
  "query": "Rust web scraping",
  "num_results": 10,
  "language": "en",
  "region": "us",
  "safe_search": false,
  "webhook": "https://your-webhook.com/callback",
  "sync_wait_ms": 5000
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `engine` | string | Yes | Search engine: `google`, `bing`, `baidu`, `sogou` |
| `query` | string | Yes | Search query |
| `limit` | integer | No | Number of results (default: 10, max: 100) |
| `lang` | string | No | Search language (default: `en`) |
| `country` | string | No | Search region (default: `us`) |
| `safe_search` | boolean | No | Enable safe search (default: false) |
| `webhook` | string | No | Webhook URL for notifications |
| `sync_wait_ms` | integer | No | Wait time for synchronous response |

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
        "engine": "google"
      }
    ],
    "crawl_id": "550e8400-e29b-41d4-a716-446655440000",
    "credits_used": 5
  }
}
```

---

### Extract API

> **Conditional Endpoint (R-teams-003):** This endpoint's signature depends on the `teams` feature:
> - **`teams` enabled (default):** `POST /v1/extract` accepts `Extension<Arc<TeamService>>` and `Extension<Arc<GR>>` (geo-restriction repository) parameters, enforcing team-scoped geo-restrictions before task creation.
> - **`teams` disabled:** `POST /v1/extract` degrades to a simpler signature without geo-restriction parameters; tasks are created directly against `DEFAULT_TEAM_ID` without geo checks.

Extract structured data from HTML.

#### Extract Data

**Endpoint:** `POST /v1/extract`

**Request Body:**
```json
{
  "html": "<html>...</html>",
  "extraction_rules": {
    "title": {
      "selector": "h1",
      "attribute": "text"
    },
    "links": {
      "selector": "a",
      "attribute": "href",
      "multiple": true
    }
  },
  "options": {
    "return_html": true
  }
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `html` | string | Yes | HTML content to extract from |
| `extraction_rules` | object | Yes | CSS selector extraction rules |
| `options` | object | No | Extraction options |

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "title": "Example Page",
    "links": [
      "https://example.com/page1",
      "https://example.com/page2"
    ]
  }
}
```

---

### Task API

Query and manage tasks. Task API follows RESTful conventions with action suffixes (`_query` for queries, `_cancel` for cancellations).

#### Query Tasks

**Endpoint:** `POST /v1/tasks/_query`

**Request Body:**
```json
{
  "filters": {
    "status": ["completed", "running"],
    "type": ["scrape", "crawl", "extract"],
    "created_after": "2025-01-01T00:00:00Z",
    "created_before": "2025-01-15T00:00:00Z"
  },
  "pagination": {
    "page": 1,
    "limit": 20
  },
  "sort": {
    "field": "created_at",
    "order": "desc"
  }
}
```

**Filter Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `status` | array | Filter by status: `pending`, `running`, `completed`, `failed`, `cancelled` |
| `type` | array | Filter by type: `scrape`, `crawl`, `extract` |
| `created_after` | string | Filter by creation date (RFC3339) |
| `created_before` | string | Filter by creation date (RFC3339) |

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "tasks": [...],
    "pagination": {
      "page": 1,
      "limit": 20,
      "total": 150
    }
  }
}
```

#### Cancel Tasks

**Endpoint:** `POST /v1/tasks/_cancel`

**Request Body:**
```json
{
  "task_ids": [
    "550e8400-e29b-41d4-a716-446655440000",
    "660e8400-e29b-41d4-a716-446655440001"
  ]
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "cancelled_count": 2
  }
}
```

---

### Team API

> **Conditional Endpoints (R-teams-002):** All `/v1/teams/*` endpoints are only available when the `teams` feature is enabled (default). When `teams` is disabled, these routes are not registered and return 404 Not Found. In single-tenant mode (`teams` disabled), all requests are attributed to `DEFAULT_TEAM_ID` (`Uuid::from_u128(1)`), making team management endpoints unnecessary.

#### Get Current Team

**Endpoint:** `GET /v1/teams/me`

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "770e8400-e29b-41d4-a716-446655440000",
    "name": "My Team",
    "created_at": "2025-01-01T00:00:00Z"
  }
}
```

#### Get Team Usage

**Endpoint:** `GET /v1/teams/me/usage`

**Response:**
```json
{
  "success": true,
  "data": {
    "credits_used": 1234,
    "credits_limit": 10000,
    "requests_today": 42,
    "requests_limit": 1000
  }
}
```

#### Get Team Geo Restrictions

**Endpoint:** `GET /v1/teams/geo-restrictions`

**Response:**
```json
{
  "success": true,
  "data": {
    "restrictions": {
      "allowed_countries": ["US", "UK", "CA"],
      "blocked_countries": ["CN", "RU"],
      "enabled": true
    }
  }
}
```

#### Update Team Geo Restrictions

**Endpoint:** `PUT /v1/teams/geo-restrictions`

**Request Body:**
```json
{
  "allowed_countries": ["US", "UK", "CA"],
  "blocked_countries": ["CN", "RU"],
  "enabled": true
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "message": "Geo restrictions updated"
  }
}
```

---

### Webhook API

> **Conditional Endpoints (R-wh-001):** All `/v1/webhooks/*` endpoints are only available when the `webhook` feature is enabled (default). When `webhook` is disabled, these routes are not registered and return 404 Not Found. Additionally, the `webhook_worker` is not spawned, and `NoopWebhookService` is injected (task completion/failure notifications become no-ops).

#### List Webhooks

**Endpoint:** `GET /v1/webhooks`

**Query Parameters:**
- `page` - Page number (default: 1)
- `limit` - Results per page (default: 20, max: 100)

**Response:**
```json
{
  "success": true,
  "data": {
    "webhooks": [
      {
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "url": "https://your-webhook.com/callback",
        "events": ["task.completed", "task.failed"],
        "active": true,
        "created_at": "2025-01-15T00:00:00Z"
      }
    ],
    "pagination": {
      "page": 1,
      "limit": 20,
      "total": 5
    }
  }
}
```

#### Create Webhook

**Endpoint:** `POST /v1/webhooks`

**Request Body:**
```json
{
  "url": "https://your-webhook.com/callback",
  "events": ["task.completed", "task.failed"],
  "secret": "your-webhook-secret",
  "active": true
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `url` | string | Yes | Webhook URL |
| `events` | array | Yes | Events to subscribe to |
| `secret` | string | No | Secret for webhook signature |
| `active` | boolean | No | Enable/disable webhook |

**Events:**
- `task.created` - Task created
- `task.started` - Task started
- `task.completed` - Task completed
- `task.failed` - Task failed
- `task.cancelled` - Task cancelled

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "webhook": {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "url": "https://your-webhook.com/callback",
      "events": ["task.completed", "task.failed"],
      "active": true
    }
  }
}
```

---

### Audit API

#### Get Audit Logs

**Endpoint:** `GET /v1/audit/logs`

**Query Parameters:**
- `event_type` - Filter by event type
- `start_time` - Start timestamp
- `end_time` - End timestamp
- `page` - Page number
- `limit` - Results per page

**Response:**
```json
{
  "success": true,
  "data": {
    "logs": [
      {
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "event_type": "api_request",
        "timestamp": "2025-01-15T00:00:00Z",
        "api_key_id": "770e8400-e29b-41d4-a716-446655440000",
        "endpoint": "/v1/scrape",
        "ip_address": "192.168.1.1",
        "user_agent": "Mozilla/5.0..."
      }
    ],
    "pagination": {
      "page": 1,
      "limit": 20,
      "total": 500
    }
  }
}
```

#### Get Denied Requests

**Endpoint:** `GET /v1/audit/denied`

**Query Parameters:**
- `reason` - Filter by denial reason
- `start_time` - Start timestamp
- `end_time` - End timestamp
- `page` - Page number
- `limit` - Results per page

**Response:**
```json
{
  "success": true,
  "data": {
    "denied_requests": [
      {
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "timestamp": "2025-01-15T00:00:00Z",
        "reason": "rate_limit_exceeded",
        "api_key_id": "770e8400-e29b-41d4-a716-446655440000",
        "endpoint": "/v1/scrape",
        "ip_address": "192.168.1.1"
      }
    ],
    "pagination": {
      "page": 1,
      "limit": 20,
      "total": 100
    }
  }
}
```

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
    "scopes": ["read", "write"]
  },
  "timestamp": "2025-07-21T12:00:00+00:00"
}
```

> **注：** 响应中 `scopes` 字段回显请求时传入的 scope（`read`/`write`/`admin`），实际 garrison 权限串（`crawlrs:read` 等）由 handler 在签发时映射，调用方无需关心。

> **⚠️ 安全提示：** `api_key` 字段是明文 key，仅在本次响应中返回一次。请立即妥善保存（如写入 secrets manager），后续无法再次查询。若丢失需重新签发。

**Error Codes:**

| Error Code | HTTP Status | Description |
|------------|-------------|-------------|
| `UNAUTHORIZED` | 401 | 调用方未认证或 garrison key 无效 |
| `FORBIDDEN` | 403 | 调用方缺少 `crawlrs:admin` 权限 |
| `VALIDATION_ERROR` | 400 | 请求体格式错误（如 `team_id` 非 UUID、`scopes` 含未知权限） |
| `FEATURE_DISABLED` | 404 | `auth` feature 未启用，路由未注册 |
| `INTERNAL_ERROR` | 500 | garrison `ApiKeyHandler::generate_with_namespace` 内部错误 |

---

## Rate Limiting

> **Conditional Behavior (R-rl-003):** Rate limiting is only active when the `rate-limit` feature is enabled (default). When disabled, `NoopRateLimitingService` is injected: `check_rate_limit` returns `Allowed`, `check_and_deduct_quota` returns `Ok(())`, `get_quota_balance` returns `Ok(i64::MAX)`. All requests are allowed without limit, and quota deductions are no-ops. The `X-RateLimit-*` headers are not populated.

The API implements rate limiting at multiple levels:

1. **Per-API Key Rate Limit** - Limits requests per API key
2. **Per-Team Concurrency Limit** - Limits concurrent requests per team
3. **Global Rate Limit** - System-wide protection

### Rate Limit Headers

Rate limit information is included in response headers:

```
X-RateLimit-Limit: 60
X-RateLimit-Remaining: 45
X-RateLimit-Reset: 1705315200
```

---

## Webhooks

Webhooks allow you to receive notifications about task events.

### Webhook Payload Format

```json
{
  "event": "task.completed",
  "timestamp": "2025-01-15T00:00:00Z",
  "task": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "type": "scrape",
    "status": "completed",
    "url": "https://example.com"
  },
  "result": {
    "html": "...",
    "markdown": "..."
  }
}
```

### Webhook Signature

If a secret is provided, the webhook includes an `X-Webhook-Signature` header:

```
X-Webhook-Signature: sha256=hexdigest
```

Verify the signature by computing HMAC SHA256 of the payload using your secret.

---

## SDK API

SDK endpoints provide simplified interfaces for common operations, wrapping the underlying REST API.

### SDK Search

**Endpoint:** `POST /api/v1/sdk/search`

**Request Body:**
```json
{
  "query": "Rust web scraping",
  "engine": "google",
  "num_results": 10
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "results": [...],
    "credits_used": 5
  }
}
```

### SDK Tasks

**Endpoint:** `POST /api/v1/sdk/tasks`

**Request Body:**
```json
{
  "filters": {
    "status": ["completed"],
    "type": ["scrape", "crawl"]
  },
  "pagination": {
    "page": 1,
    "limit": 20
  }
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "tasks": [...],
    "pagination": {
      "page": 1,
      "limit": 20,
      "total": 150
    }
  }
}
```

### SDK Scrape

**Endpoint:** `POST /api/v1/sdk/scrape`

**Request Body:**
```json
{
  "url": "https://example.com",
  "formats": ["markdown"],
  "sync_wait_ms": 5000
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "completed",
    "result": {
      "markdown": "..."
    }
  }
}
```

### SDK Crawl

**Endpoint:** `POST /api/v1/sdk/crawl`

**Request Body:**
```json
{
  "url": "https://example.com",
  "max_pages": 50,
  "max_depth": 2,
  "formats": ["markdown"]
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "running"
  }
}
```

---

## SDK Examples

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

## Best Practices

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

## Changelog

### v0.2.0 (2025-07-21)
- **`garrison-auth-migration`：** 认证引擎由 garrison v0.8.1 接管，Bearer token 格式改为 `garrison_key_id.garrison_secret`
- 新增 `POST /v1/admin/api-keys` 端点：管理员为指定 team 签发 garrison API Key
- 新增 [Garrison 认证](#garrison-认证) 与 [迁移指南](#迁移指南020-garrison-auth-migration) 章节
- 401/429 触发逻辑由 garrison `firewall-bruteforce` 接管（5 次/60 秒/300 秒锁定）
- Scrape API `options` 新增 `cache_mode`（5 种缓存读写模式）和 `bypass_cache`（应急缓存绕过）字段
- Added `GET /v1/webhooks` to list webhooks
- Added `GET /v1/teams/me` and `GET /v1/teams/me/usage` team endpoints
- Added SDK API section (`/api/v1/sdk/*` endpoints)
- Added `POST /v1/crawl/{id}/_cancel` cancel endpoint (alongside existing `DELETE /v1/crawl/{id}`)
- Merged Task API sections into single unified section
- Removed duplicate table of contents
- Updated base URL to localhost:8899

### v0.1.0 (2025-01-15)
- Initial release
- Scrape, Crawl, Search, Extract APIs
- Rate limiting and concurrency control
- Webhook support
- Audit logging
- Metrics export

---

## Support

For questions or issues:
- 📖 [Documentation](/)
- 🐛 [Issue Tracker](https://github.com/your-org/crawlrs/issues)
- 📧 Email: Kirky-X@outlook.com
