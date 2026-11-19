
# 📘 Crawlrs API Reference

<div align="center">

![API Version](https://img.shields.io/github/v/release/Kirky-X/crawlrs?label=api&style=for-the-badge)
![Base URL](https://img.shields.io/badge/base%20URL-http://localhost:8899-green?style=for-the-badge)
![License](https://img.shields.io/github/license/Kirky-X/crawlrs?label=license&style=for-the-badge)

**Version:** 0.2.0 | **Base URL:** `http://localhost:8899` | **Updated:** 2026-11-15

</div>

---

## 📋 Table of Contents

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

> **Since 0.2.0 (`garrison-auth-migration`):** the Bearer token is verified by **garrison v0.9** with the format `garrison_key_id.garrison_secret` (the plaintext key is returned once at issuance only). The legacy SHA-256 `api_key_hash` is invalidated; keys must be re-issued through garrison.

### Garrison Authentication

**Authentication flow:**

1. The client passes a garrison-issued key in the `Authorization: Bearer` header (format `garrison_key_id.garrison_secret`)
2. `auth_middleware_inner` calls `GarrisonUtil::check_api_key` to verify the key, expiration, status, and RBAC permissions
3. `auth_bridge::bridge_to_auth_state` maps garrison permissions to `ApiKeyScope` via `map_perms_to_scope` and injects them into `AuthState`
4. 401 (invalid key) / 429 (brute-force lockout) are triggered by garrison's `firewall-bruteforce` (default: 5 failures / 60-second window / 300-second lockout)

**RBAC permission mapping (`tenant_id=0`, shared by all teams, admin implies read+write):**

| garrison permission | Mapped to `ApiKeyScope` |
|---------------|----------------------|
| `crawlrs:admin` | `read=true, write=true, admin=true` |
| `crawlrs:write` | `write=true` |
| `crawlrs:read` | `read=true` |

All are constructed via `ApiKeyScope::with_custom_limits` (`DEFAULT_SEARCH_LIMIT=100`, `DEFAULT_SCRAPE_LIMIT=50`) instead of calling `full_access()` — avoiding a conflict between unlimited `u32::MAX` and per-team quota semantics.

**JWT configuration:**

| Environment Variable | Description | Required |
|----------|------|------|
| `CRAWLRS__AUTH__JWT_SECRET` | HS256 JWT secret, ≥32 bytes; startup refuses weak secrets | Yes (when the `auth` feature is enabled) |

### API Key Scopes

API keys carry an `ApiKeyScope` with three boolean permissions plus two hourly limits (`src/domain/auth/mod.rs`):

| Field | Description |
|--------|-------------|
| `read` | Permission to access read-only endpoints |
| `write` | Permission to access write endpoints |
| `admin` | Permission to access administrative endpoints (implies read + write) |
| `search_limit` | Maximum search requests per hour |
| `scrape_limit` | Maximum scrape requests per hour |

> **Current status (0.2.0):** although `scope_middleware` is implemented (`src/presentation/middleware/auth_middleware.rs`), it is **not mounted in the production router** — it exists only in the test Router assembly. Except for `POST /v1/admin/api-keys`, whose handler explicitly checks the `Admin` permission, no other endpoint currently validates the API Key scope — only key validity. Do not assume RBAC is enforced at the endpoint layer.

### Auth Disabled Behavior (auth-off)

> **Conditional:** When the `auth` feature is disabled (via `--no-default-features` or explicit feature selection), the real `auth_middleware_inner` is not compiled. `default_identity_middleware` injects a prebuilt `AuthState` template instead — but with **deny-by-default**: unless explicitly opted in, every protected request gets `401`.

| Behavior | When `auth` Enabled (default) | When `auth` Disabled |
|----------|------------------------------|---------------------|
| `Authorization` header | Required (401 if missing/invalid; 429 on brute-force lockout) | **Ignored, but requests still 401** until `allow_unauthenticated_protected()` is called |
| Opt-in | N/A | Must explicitly call `auth_middleware::allow_unauthenticated_protected()` (one-time `error!` log declares the security downgrade) |
| `AuthState.team_id` | Resolved via garrison → reverse lookup `api_key_id→team_id` + `TEAM_ID_CACHE` LRU | Fixed to `DEFAULT_TEAM_ID` (`domain::id::Id::from_u128(1)`) |
| `AuthState.api_key_id` | Resolved from garrison key | Fixed to `DEFAULT_API_KEY_ID` (`domain::id::Id::from_u128(2)`) |
| `AuthState.scope` | `map_perms_to_scope(garrison permissions)` | `ApiKeyScope::denied()` (read/write/admin all `false`, limits 0) — a restricted anonymous identity, not `full_access()` |
| Token cache | garrison-managed oxcache; the crawlrs side caches the mapping in `TEAM_ID_CACHE` | N/A — fixed identity, no caching |
| Rate limit lockout | garrison `firewall-bruteforce` (5/60s/300s) | Not tracked (but `rate-limit` feature may still apply per-IP limits) |

> Source evidence: `src/bootstrap/routes.rs::build_default_identity_template` (the template carries `ApiKeyScope::denied()`) and `src/presentation/middleware/auth_middleware.rs::default_identity_middleware` (401 by default; injects the template after opt-in). Even after opt-in the identity is still denied-restricted; business layers that check permissions will refuse service.

**Build commands:**
```bash
# Auth disabled (single-tenant, no auth)
cargo build --release --no-default-features

# Auth enabled (default, includes garrison)
cargo build --release --features default
```

### Migration Guide (0.2.0 garrison-auth-migration)

**Impact on existing API clients:**

- Legacy API Keys (based on the SHA-256 `api_key_hash`) are all invalidated and must be re-issued through garrison
- Clients need no change to the `Authorization: Bearer` calling convention — only replace the key with the new `garrison_key_id.garrison_secret`
- The 401/429 response semantics are unchanged, but triggering is now owned by garrison's `firewall-bruteforce`

**Migration completed:**

- Legacy API Keys are invalidated; clients must use the `Authorization: Bearer <garrison_key_id>.<garrison_secret>` format
- New teams get API Keys issued via `POST /v1/admin/api-keys` with no ops-tooling involvement

See [ARCHITECTURE.md → Garrison Authentication Engine](ARCHITECTURE.md#garrison-authentication-engine) for the architecture details.

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

> **No endpoint currently uses the `meta` pagination envelope.** The `ApiResponse` struct and the `success_response_with_meta` constructor are defined in `src/presentation/handlers/response_builder.rs` (carrying `PaginationMeta`), but no production handler calls them; list endpoints (e.g. `GET /v1/webhooks`, audit logs) lay pagination info flat inside `data`. The `meta` shape above is retained library capability, not the current API contract.

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

Rate-limit rejections (429) are returned by each handler's `check_rate_limit` through the standard `ApiResponse` envelope, with `error` as a `{code,message}` object:

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

When the `RetryAfter` outcome is hit, a `retry_after_seconds` field is added:

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

> Source evidence: `src/presentation/helpers/rate_limit_helper.rs::check_rate_limit` (`Denied` → `errors::too_many_requests`, `RetryAfter` → `errors::too_many_requests_with_retry`, backend errors fail-closed as 503 → `errors::service_unavailable`). The only response header set is `Retry-After`; there are no `X-RateLimit-*` headers.

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

Production error code list (`src/common/error.rs::CrawlRsError::error_code` and `src/presentation/handlers/response_builder.rs::error_codes`):

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
| `DATABASE_ERROR` | 500 | Database error (produced indirectly via the `CrawlRsError` path) |
| `CACHE_ERROR` | 503 | Cache error (produced indirectly via the `CrawlRsError` path) |
| `EXTERNAL_SERVICE_ERROR` | 502 | External service error (produced indirectly via `CrawlRsError::Network`) |
| `TIMEOUT` | 504 | Request timeout (produced indirectly via `CrawlRsError::Timeout`) |

Per-tenant outbound quota overflows (`OutboundQuotaError`) map onto the existing codes: `BrowserQuotaExceeded` and `DnsQuotaExceeded` become `RATE_LIMITED` (429, retryable), while `QueueTimeout` becomes `TIMEOUT` (504). These fire when `[outbound_isolation.<tenant>]` budgets are exhausted.

> `CONFLICT` / `PRECONDITION_FAILED` / `FEATURE_DISABLED` have constants defined in the `error_codes` module but no production emission points; when a feature is not compiled, its routes are not registered at all, so requests get a plain 404 (`NOT_FOUND`).

> `/metrics`: a public Prometheus text endpoint (no auth, no rate limiting) that exposes runtime metrics only,
> with no sensitive labels such as tenant IDs, webhook URLs, or credentials. In production, enforce a network allowlist at the reverse proxy;
> metric transport is now an in-process recorder (no listener on 9100).


---

## 🌐 Public Endpoints

### Health Check (Liveness)

Check if the API process is running. Always returns 200 when the process is alive — dependencies are not probed here.

**Endpoint:** `GET /health`

**Response:**
```json
{
  "status": "healthy",
  "version": "0.2.0"
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
  "version": "0.2.0"
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
  "version": "0.2.0"
}
```

> `checks` includes `database`, `cache`, and `database_pool` (a structured snapshot of the connection pool: capacity/idle/waiting — diagnostics only, it does not affect readiness). Each failed probe is marked `status: "down"` with an `error` attached (`"timeout"` on timeout).

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
0.2.0
```

### Get Metrics

**Endpoint:** `GET /metrics`

> **⚠️ Currently a placeholder implementation.** `metrics_handler` returns a JSON envelope whose `metrics` field is a piece of **hard-coded Prometheus-style text** — `version` fixed to `1.0.0` and uptime fixed to `3600`; it is neither real runtime metrics nor the Prometheus text-format aggregation endpoint. The authoritative list of real metrics (capture points and label definitions) is in [PERFORMANCE.md](PERFORMANCE.md). The authoritative scrape format for `/metrics` (directly consumable by a Prometheus scraper) is not yet implemented.

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
| `wait_for` | integer | — | Wait hint in milliseconds (DTO validation max 60000, but the use case clamps `ms.min(10_000)` — values above 10000 are silently truncated to 10000). Mapped to the `DomStable` wait strategy (DOM unchanged for this duration ⇒ ready); `None` uses the engine default (`WaitFor::NetworkIdle`, CDP network-event based). Mutually exclusive with `wait_for_response_url` / `wait_for_response_body`. See [ARCHITECTURE.md → WaitFor](ARCHITECTURE.md#waitfor-strategy). |
| `wait_for_response_url` | string | — | Response-level wait: resolve when a response URL matches this regex ([regex crate](https://docs.rs/regex) syntax, e.g. `api/v\d+/items`). Validated at request time — an invalid regex is rejected before the task is enqueued (`wait_for`-style silent truncation does not apply). Engine support: Playwright (CDP response events); moli returns `Unsupported` (its CDP Network event surface is unavailable — see `scripts/poc/moli/network-probe-output.txt`). On timeout the request fails with the standard wait-timeout error. |
| `wait_for_response_body` | string | — | Response-level wait: resolve when a response body contains this substring (e.g. `"status":"ready"`). Per-response matching is capped at 10 MiB (bodies beyond the cap are skipped, not buffered). Same exclusivity, engine support and timeout semantics as `wait_for_response_url`. |
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

> `credits_used` is fixed at 1 (each scrape task deducts 1 credit on enqueue; a failed enqueue is refunded automatically). Status codes: `201 Created` on successful enqueue; `202 Accepted` when `sync_wait_ms > 0` and the wait times out (the body structure is unchanged and still contains the task `id`).

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

> The response is a flat `ScrapeStatusResponseDto` (no outer `task` key). `result` appears only when the task is `completed`; sensitive response headers (`set-cookie` / `authorization` etc.) are redacted to `[REDACTED]` before returning. `error` appears only when the task is `failed`.
>
> Optional field `js_degraded` (boolean): appears as `true` only when the request asked for JS rendering but the memory watermark (`engines.memory_scheduler`, the `engine-moli` feature) triggered a static fallback — normal responses and stale cache entries never contain this field.

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

> The request body is validated with `#[serde(deny_unknown_fields)]`: unlisted fields (e.g. `formats`, `webhook`, `options`, `follow_links`) are rejected with 422.

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

> The response is the bare `Crawl` domain model (no `credits_used` field, no `task` wrapper). Status codes: `201 Created` on successful creation; `202 Accepted` when `sync_wait_ms > 0` and the wait times out.

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

> The response is the bare `Crawl` domain model (no `task` / `progress` wrapper). `status` takes the serialized `CrawlStatus` values (`queued` / `processing` / `completed` / `failed` / `cancelled`).

#### Get Crawl Results

**Endpoint:** `GET /v1/crawl/{id}/results`

**Parameters:**
- `id` (path) - Crawl UUID

> **No query parameters**: this endpoint accepts no `page` / `limit`; it returns the scrape results of all sub-tasks of the crawl in one response.

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

> The response `data` is a bare `Vec<ScrapeResult>` array (no pagination envelope).

#### Cancel Crawl

Cancel a crawl task. All sub-tasks of the crawl are cancelled with it.

**Endpoint:** `DELETE /v1/crawl/{id}`

**Parameters:**
- `id` (path) - Crawl UUID

**Response:** `204 No Content` — empty body, no JSON envelope (on error the standard error envelope is returned: 403 not owner / 404 not found / 500 internal error).

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
| `engine` | string | No | Search engine: `smart` (default — multi-engine fan-out + RRF aggregation), `bing`, `baidu`, `sogou`. When omitted or `smart`, multiple engines are queried concurrently with fusion ranking and dedup; an explicit engine name takes the single-engine path; an unknown engine name returns 400 (no silent substitution). `exa`/`tavily` are reserved enum values — usable only when the corresponding engine is registered, otherwise `NoEngineAvailable` is returned |
| `query` | string | Yes | Search query |
| `sources` | array | No | Alias of `engine` (multi-source form) |
| `limit` | integer | No | Number of results (default: 10; silently capped at `settings.search.max_results`, default **50**) |
| `lang` | string | No | Search language — **accepted but not yet effective in the current version (known gap)**: the `_lang` parameter of `SearchService::perform_search` is ignored; the default-`en` Bing mkt/Accept-Language driving is not implemented (the baidu/sogou paths do not read it either) |
| `country` | string | No | Search region — **accepted but not yet effective in the current version (known gap)**: the `_country` parameter is ignored, and no default `us` drives any request parameter |
| `sync_wait_ms` | integer | No | Synchronous wait duration (milliseconds, default 5000). **Note:** there is no DTO-level upper-bound check (unlike the explicit `MAX_SYNC_WAIT_MS` check in scrape/crawl), so even values above 30000 enter the wait-polling loop |
| `crawl_results` | boolean | No | When true, creates a crawl task for each search result |
| `crawl_config` | object | No | Configuration for async crawling of search results (same structure as the crawl task config) |

> **0.2.0 behavior change**: when `engine` is omitted, the behavior changed from "single bing engine" to smart aggregation (fan-out + RRF + cross-source dedup);
> explicitly specifying `engine: "bing"` etc. still takes the single-engine path.

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

> **The `score` field (new in 0.2.0, optional)**: with `[rag]` semantic reranking enabled, each result gains a
> `score` (a 0..1 query-doc relevance score). When disabled the field is omitted entirely
> (`skip_serializing_if = "Option::is_none"`), so existing consumers parse unchanged.

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

> Pagination uses `limit`/`offset`: `has_more = offset + len(tasks) < total`. No `page` parameter, no `pagination` envelope. `status` serializes to `queued`/`active`/`completed`/`failed`/`cancelled`.

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

> `total_tasks` counts tasks found via `find_by_crawl_id(team_id)`; `completed_tasks`/`failed_tasks` are currently fixed `0` (handler accounting is not wired up yet). On a `credits_balance` query failure the value falls back to `0` (no error).

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

> `period` is fixed to `"30d"`. `total_requests`/`successful_requests`/`failed_requests` are always `0` in the current implementation (handler accounting is not wired up yet); `credits_used` is the absolute value of the credit balance (a negative balance means overdrawn). There are no `credits_limit` / `requests_today` / `requests_limit` fields. `avg_response_time_ms` is the average response time of the team's scrape results.

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

> The response is a flat `TeamGeoRestrictionsResponse` (no outer `restrictions` wrapper). The field name is `enable_geo_restrictions` (not `enabled`); unset list fields serialize as `null`.

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

> `enable_geo_restrictions` is **required** (bool, no default). Country codes must be 2-letter ISO 3166-1 alpha-2; `ip_whitelist` accepts IPs or CIDR ranges — validation failures return `400 VALIDATION_ERROR`. `deny_unknown_fields` rejects unknown fields (422). If provided, the allowed/blocked lists must not be empty arrays.

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

> The response is the updated restrictions object itself (not a `{message}` envelope).

---

### Webhook API

> **Conditional Endpoints (R-wh-001):** All `/v1/webhooks/*` endpoints are only available when the `webhook` feature is enabled (default). When `webhook` is disabled, these routes are not registered and return 404 Not Found. Additionally, the `webhook_worker` is not spawned, and `NoopWebhookService` is injected (task completion/failure notifications become no-ops).

#### List Webhooks

**Endpoint:** `GET /v1/webhooks`

> **No query parameters**: returns all webhooks of the team in one response (no `page` / `limit`). The list response does **not** include the `secret` field.

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

> The request body is validated with `#[serde(deny_unknown_fields)]`: `CreateWebhookRequest` accepts only `url`; unknown fields such as `events` / `secret` / `active` are rejected with **400 `VALIDATION_ERROR`** (the handler parses with `serde_json::from_slice` and maps failures to `CrawlRsError::Validation`). In addition, the creation request itself must carry Standard Webhooks signature headers computed with the server-side global secret (`CRAWLRS__WEBHOOK__SECRET`) — see [Webhook Signature](#webhook-signature); signature verification failures return **401 `AUTHENTICATION_ERROR`**.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `url` | string | Yes | Webhook callback URL (http/https) |

**Events (dispatched automatically by the server per task type, no subscription needed):**
- `scrape.completed` - Scrape task completed
- `scrape.failed` - Scrape task failed
- `crawl.completed` - Crawl task completed
- `crawl.failed` - Crawl task failed
- `extract.completed` / `extract.failed` - Extract task completed/failed

**Response (Success):** `201 Created` with the standard `{"success":true,"data":{...}}` envelope. **The creation response returns the `secret`** (each webhook's signing secret is derived server-side from `settings.webhook.secret` and returned only in this creation response; list endpoints never return it).

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

> No `event_type` / `start_time` / `end_time` / `page` parameters. By default it queries the logs of the caller's own `api_key_id`; with admin scope any key/team can be queried. See `audit_handler.rs` for IDOR protection.

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

> The response contains only a `logs` array (`AuditLogEntry`), with no `pagination` envelope. `decision` serializes to `"Allow"` / `"Deny"`.

#### Get Denied Requests

**Endpoint:** `GET /v1/audit/denied`

**Query Parameters:**
- `limit` - Results per page (default `server_config::DEFAULT_PAGE_LIMIT`, capped at `MAX_PAGE_LIMIT`)

> Only `limit` is supported; `reason` / `start_time` / `end_time` / `page` are not. It always queries the denial records of the caller's own `api_key_id` (avoiding IDOR).

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

> The response contains only a `denied_requests` array (also `AuditLogEntry`, with `decision` `"Deny"`), with no `pagination` envelope.

---

### Admin API

> **New in 0.2.0 (`garrison-auth-migration`):** admins can issue new garrison API Keys through this endpoint. The caller must hold the `crawlrs:admin` permission (i.e. the `admin` scope).

#### Issue API Key

Issues a new garrison API Key for the specified team. The plaintext key is returned once only; the response headers include `Cache-Control: no-store` / `Pragma: no-cache` / `Expires: 0` to prevent caching.

**Endpoint:** `POST /v1/admin/api-keys`

**Required Scope:** `admin` (corresponding to the garrison permission `crawlrs:admin`)

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
| `team_id` | string (UUID) | Yes | The team ID the API Key belongs to |
| `scopes` | array<string> | Yes | List of scopes; allowed values: `read` / `write` / `admin` (the handler maps them to the garrison permissions `crawlrs:read` / `crawlrs:write` / `crawlrs:admin`) |
| `expires_in_secs` | integer | No | Expiration in seconds (default 2592000 = 30 days) |

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

> **Note:** the `scopes` field in the response echoes the scopes passed in the request (`read`/`write`/`admin`); the actual garrison permission strings (`crawlrs:read` etc.) are mapped by the handler at issuance and are irrelevant to the caller. `expires_at` is an RFC3339 expiration (default 30 days; `expires_in_secs` must be > 0 and ≤ 1 year, and `scopes` accepts at most 10 elements).

> **⚠️ Security note:** the `api_key` field is the plaintext key, returned once only in this response. Save it properly right away (e.g. into a secrets manager); it cannot be queried again later. If lost, a new key must be issued.

**Error Codes:**

| Error Code | HTTP Status | Description |
|------------|-------------|-------------|
| `UNAUTHORIZED` | 401 | The caller is unauthenticated or the garrison key is invalid |
| `FORBIDDEN` | 403 | The caller lacks the `crawlrs:admin` permission (checked via `has_permission(Admin)` inside the handler) |
| `VALIDATION_ERROR` | 400 | Request value validation failed (`team_id` is a nil UUID, `scopes` empty/contains unknown permissions, `expires_in_secs` out of range) |
| `UNPROCESSABLE_ENTITY` | 422 | Malformed request body (e.g. JSON data errors such as a non-UUID `team_id`, missing fields, type mismatches) |
| `INTERNAL_ERROR` | 500 | `team_id` not found in the teams table, garrison DAO not injected, or an internal error in `ApiKeyHandler::generate_with_namespace` |

> **Note:** when the `auth` feature is disabled, the routes are not registered and requests get a plain 404 (`NOT_FOUND`); there is no `FEATURE_DISABLED` response code.

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

**The current implementation sets only one response header:** `Retry-After` is set when a rate-limited request returns 429. **The `X-RateLimit-Limit` / `X-RateLimit-Remaining` / `X-RateLimit-Reset` response headers do not exist** (no emission points anywhere in the codebase — client logic reading these headers should be removed).

```
Retry-After: 60
```

For a response body example, see [Rate Limit Error Response](#rate-limit-error-response) above: the standard `ApiResponse` envelope (`error.code = "RATE_LIMITED"`), with `retry_after_seconds` added on the `RetryAfter` outcome.

> Additional semantics: when the rate-limiting service itself fails, the default is fail-closed (503 `SERVICE_UNAVAILABLE`); setting `RATE_LIMIT_FAIL_OPEN=true` switches to allowing the request through with a warn log.

---

## 🪝 Webhooks

Webhooks allow you to receive notifications about task events.

### Webhook Payload Format

The delivered payload is a JSON object containing `task_id`, `status`, `url`, and `timestamp` (Unix seconds), with `error` attached on failure:

```json
{
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "completed",
  "url": "https://example.com",
  "timestamp": 1760000000
}
```

The event type is determined by the task type (`scrape.completed` / `scrape.failed` / `crawl.completed` / `crawl.failed` / `extract.completed` / `extract.failed`).

### Webhook Signature

Signatures follow the [Standard Webhooks](https://www.standardwebhooks.com/) spec; deliveries carry three headers:

```
webhook-id: <msg_id>
webhook-timestamp: <unix_seconds>
webhook-signature: v1,<base64-encoded-hmac-sha256>
```

The signed message is `{msg_id}.{timestamp}.{payload}` (the three parts joined with periods), HMAC-SHA256 computed with the server-configured secret (`CRAWLRS__WEBHOOK__SECRET`). To verify, recompute using the values of the `webhook-id` and `webhook-timestamp` headers and compare with `webhook-signature` (use the official `standardwebhooks` library with constant-time comparison).

---

## 🧩 SDK API

SDK endpoints provide simplified interfaces for common operations, wrapping the underlying REST API. All SDK endpoints return the payload object directly (no `success`/`data` envelope).

> **Path resolution:** SDK routes are registered by the sdforge `#[forge]` macro (`version = "v1"` + `path = "/sdk/..."`); sdforge's default prefix is `/api/{version}`, so the actual external paths are uniformly `/api/v1/sdk/*`. SDK routes also go through the authentication middleware (`team_id`/`api_key_id` are extracted from `AuthState`, never trusted from the request body).

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
  "seed_url": "https://example.com",
  "max_depth": 2
}
```

`max_depth` is optional, defaulting to 2 with a range of 0–5 (the same source cap as the HTTP crawl entry).
Creation immediately seeds the first depth=0 task from `seed_url`, and `total_tasks` starts counting from 1;
if the first task fails to enqueue, the endpoint returns 500 and marks the crawl as Failed (self-consistent counting, no orphans).
Same as `/v1/crawl`: `url`/`seed_url` accept http(s) only, and the team's `domain_blacklist` is enforced
(a hit returns 400; the blacklist is persisted with crawl.config for the worker and link filtering to use); geographic (country) restrictions depend on the request's source IP —
server-to-server calls have no client IP, so the SDK entry performs no country check.
SDK endpoints are rate-limited per API key (`/sdk/crawl`); this entry consumes no credits (unlike the
per-request billing of `/sdk/search` — `/sdk/crawl`'s quota is governed by rate limiting).

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

### 0.2.0 (2026-11-15)
- First stable crates.io release: in-house dependencies normalized to stable versions (minor precision), alchemize switched to the registry (no path/git dependencies), i18n host registration rewritten, release.yml OIDC + token fallback
- acceptance teardown clippy fix; SDK test I18nBundle two-argument call

**Changed (former Unreleased items, shipped along the 0.2.0 line):**
- ID system fully migrated to NebulaId (`domain::id::Id`, default `uuid_v8` algorithm; the API JSON contract stays UUID strings)
- `/v1/search` without an `engine` parameter changed from "single bing engine" to smart aggregation (multi-engine fan-out + RRF fusion)
- auth-off semantics: protected routes reject with 401 by default and require an explicit `allow_unauthenticated_protected()` opt-in; after opt-in a `ApiKeyScope::denied()` restricted anonymous identity (`DEFAULT_TEAM_ID`/`DEFAULT_API_KEY_ID`) is injected, not `full_access()`

**New Engines / Algorithms:**
- TLS Fingerprint Engine (`engine-tls-fingerprint`): WreqEngine based on BoringSSL, real JA3/JA4 fingerprint spoofing
- MLLM Autonomous Navigation Engine (`engine-mllm`): Vision LLM agentic loop for autonomous page navigation
- RAG-augmented extraction, knowledge-graph coverage-aware crawling, and DRL adaptive crawl policies

**Observability:**
- 5 new Prometheus metrics: `crawlrs_queue_depth` / `crawlrs_engine_success_total` / `crawlrs_engine_duration_seconds` / `crawlrs_cache_hit_total` / `crawlrs_webhook_delivery_total`

### 0.2.0-rc.4 (2026-11-10)
- Release pipeline full re-run: CI five-combination / engine-moli / BDD fixes green; CodeQL 18 findings all resolved; security audit exemptions registered at the same source
- doctest/example/doc consistency fixes; dependencies html-escape 0.3.1 / toml 1.2.1 / uuid 1.29.2
- Line coverage 90.13%; E2E matrix/static/unit/integration/bench/liveapi six stages PASS (report aggregation SKIPPED by design)

### 0.2.0-rc.3 (2026-11-10)
- Single-source crawl final-state determination + crash-recovery recount; team domain blacklist enforcement chain (seeds/redirects/links/sitemap/proxy, fail-closed when the snapshot is missing)
- SDK `POST /api/v1/sdk/crawl`; search-derived 200 hard cap and depth ≤5; scrape/crawl uniformly drop custom Host headers
- release.yml CHANGELOG path aligned with docs/; crates.io publishing explicitly skipped during git dependencies with tag source archiving

### 0.2.0-rc.2 (2026-09-21)
- In-house dependencies fully switched to the crates.io registry (garrison → 0.9.0-rc.2 etc.), removing cross-repo path dependencies

### 0.2.0-rc.1 (2026-08-25)
- auth-off security semantics tightened: protected routes 401 by default with explicit `allow_unauthenticated_protected()` opt-in (this tightening is the current semantics — see the Auth Disabled Behavior section above)

### 0.2.0-legacy (2026-07-29)
- **`garrison-auth-migration`:** authentication taken over by garrison; Bearer token format changed to `garrison_key_id.garrison_secret`
- New `POST /v1/admin/api-keys` endpoint: admins issue garrison API Keys for a specified team
- 401/429 triggering now owned by garrison's `firewall-bruteforce` (5 failures / 60-second window / 300-second lockout)
- Scrape API `options` gains `cache_mode` (4 cache read/write modes: `enabled` / `disabled` / `read_only` / `bypass`) and `bypass_cache` (emergency cache bypass) fields
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
- 📖 [Documentation](../../README.md)
- 🐛 [Issue Tracker](https://github.com/Kirky-X/crawlrs/issues)
- 📧 Email: Kirky-X@outlook.com
