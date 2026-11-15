# 📖 Crawlrs User Guide
<div align="center">

![Guide](https://img.shields.io/badge/type-user%20guide-blue?style=for-the-badge)
![Version](https://img.shields.io/github/v/release/Kirky-X/crawlrs?label=version&style=for-the-badge)
![License](https://img.shields.io/github/license/Kirky-X/crawlrs?label=license&style=for-the-badge)

**Version:** 0.2.0 | **Last Updated:** 2026-11-15

</div>

---

## 📋 Table of Contents

- [👋 Introduction](#-introduction)
- [🚀 Getting Started](#-getting-started)
- [🔐 Authentication](#-authentication)
- [🏠 Single-Tenant / No-Auth Deployment](#-single-tenant--no-auth-deployment)
- [🎯 Scraping](#-scraping)
- [🕷️ Crawling](#️-crawling)
- [🔍 Searching](#-searching)
- [📊 Data Extraction](#-data-extraction)
- [🪝 Webhooks](#-webhooks)
- [📋 Tasks](#-tasks)
- [👥 Teams & Usage](#-teams--usage)
- [⚠️ Error Handling](#️-error-handling)
- [✅ Best Practices](#-best-practices)
- [🔧 Troubleshooting](#-troubleshooting)
- [❓ FAQ](#-faq)
- [📞 Support](#-support)

---

## 👋 Introduction

Welcome to **crawlrs**, a high-performance self-hosted web scraping platform built with Rust. This guide covers all available API endpoints and features.

### What You Can Do

- **Scrape** - Extract content from single web pages
- **Crawl** - Automatically discover and scrape multiple pages
- **Search** - Query Bing, Baidu, Sogou, or aggregated Smart search (Exa / Tavily available with credentials)
- **Extract** - Parse and structure data from HTML

### Key Concepts

| Concept | Description |
|---------|-------------|
| **Task** | A unit of work (scrape, crawl, extract) |
| **API Key** | Your authentication credential scoped to a team |
| **Team** | A logical group of API keys with shared usage tracking |
| **Webhook** | HTTP callback for task completion notifications |

### Available Engines

| Engine | Description | Best For |
|--------|-------------|----------|
| **Reqwest** | Pure Rust HTTP client (default engine) | Static HTML, fast scraping |
| **Playwright** | Chromium-based browser automation via chromiumoxide | JavaScript-heavy SPAs, interactions |
| **WreqEngine** | TLS fingerprint spoofing with BoringSSL JA3/JA4 (`engine-tls-fingerprint`) | Sites checking TLS fingerprints |
| **FlareSolverr** | Cloudflare challenge solver (Full/Cdp/Tls modes) | Sites behind Cloudflare |
| **MllmEngine** | Vision-LLM autonomous navigation: screenshot → decide → act (`engine-mllm`) | Unstructured pages without stable selectors |

> WreqEngine and MllmEngine are behind feature flags — build with `--features full,engine-tls-fingerprint,engine-mllm`. Note that `--features full` alone does **not** include them (`full = standard + engine-flaresolverr + extractors + llm`).

**FlareSolverr modes:**

| Mode | Description |
|------|-------------|
| `Full` | Full browser automation with session reuse, slow but reliable |
| `Cdp` | Chrome DevTools Protocol mode, moderate performance |
| `Tls` | TLS fingerprint spoofing, fastest but limited bypass capability |

---

## 🚀 Getting Started

### 1. Installation

crawlrs is a single Rust binary. Build and run:

```bash
git clone https://github.com/Kirky-X/crawlrs.git
cd crawlrs
cargo build --release

# Start the API server (default)
./target/release/crawlrs

# Or run in worker mode (requires API running)
./target/release/crawlrs worker
```

The server starts on `http://localhost:8899` by default. Configure via `config/default.toml` or environment variables (prefix `CRAWLRS__`).

### 2. Configure Authentication

Set your API key in the server config. See [Authentication](#-authentication) for details.

### 3. Your First Request

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com"
  }'
```

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "url": "https://example.com",
    "credits_used": 1
  }
}
```

> `data` is `ScrapeResponseDto` (`id`, `url`, `credits_used`, `captured_responses`). `credits_used` is always `1` on the enqueue/sync-wait response — one credit per scrape task. `captured_responses` (F6: captured XHR/Fetch responses) is `None` here and therefore omitted from JSON; read it later from the task result's `meta_data.captured_responses`.

---

## 🔐 Authentication

> **Since 0.2.0 (`garrison-auth-migration`):** Authentication is handled by **garrison v0.9**. The Bearer token format is `garrison_key_id.garrison_secret`, issued and verified by garrison; crawlrs no longer manages its own `key_hash`.

### Garrison Authentication Overview

| Component | Responsibility |
|------|------|
| **JWT issuing** | garrison issues the JWT embedded in the API Key with `CRAWLRS__AUTH__JWT_SECRET` (HS256, ≥32 bytes) |
| **RBAC** | Ships with 3 permissions (`crawlrs:read/write/admin`) + 3 roles (`admin/user/read_only`); `tenant_id=0` is shared by all teams |
| **firewall-bruteforce** | 5 failures / 60-second window / 300-second lockout; 401/429 are triggered directly by garrison |
| **audit-log** | Authentication events are stored in the crawlrs `audit_logs` table + garrison's own schema |

**Permission mapping (admin implies read+write):** garrison permissions are bridged into `ApiKeyScope` via `auth_bridge::map_perms_to_scope`, transparent to downstream business layers.

| garrison permission | Mapped to `ApiKeyScope` |
|---------------|----------------------|
| `crawlrs:admin` | `read=true, write=true, admin=true` |
| `crawlrs:write` | `write=true` |
| `crawlrs:read` | `read=true` |

All are constructed via `ApiKeyScope::with_custom_limits` (`DEFAULT_SEARCH_LIMIT=100`, `DEFAULT_SCRAPE_LIMIT=50`) instead of calling `full_access()` — avoiding a conflict between unlimited `u32::MAX` and per-team quota semantics.

### API Key Configuration

> **Since 0.2.0:** API Keys are no longer provisioned through the `CRAWLRS__AUTH__KEYS` environment variable or the `[auth] keys` config entry. Request issuance from garrison via the `POST /v1/admin/api-keys` endpoint. In dev/test environments, set `CRAWLRS_BOOTSTRAP_ADMIN=true` to let the API process bootstrap the first admin key.

**Issue a new API Key (admin):**

```bash
# Issue via the admin API
curl -X POST http://localhost:8899/v1/admin/api-keys \
  -H "Authorization: Bearer garrison_admin_key_id.garrison_admin_secret" \
  -H "Content-Type: application/json" \
  -d '{
    "team_id": "770e8400-e29b-41d4-a716-446655440000",
    "scopes": ["read", "write"],
    "expires_in_secs": 2592000
  }'
```

See [API_REFERENCE.md → Admin API](API_REFERENCE.md#admin-api) for the full parameter reference.

**Bootstrap the first admin key (dev/test):**

Two environment variables with different purposes:

| Variable | Purpose | Code Location |
|------|------|----------|
| `CRAWLRS_BOOTSTRAP_ADMIN` | When `true`/`1`, the API process calls `run_bootstrap` at startup to bootstrap an admin key | `src/main.rs:88` |
| `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` | The `<team_id_uuid>` reused during bootstrap (optional; by default `bootstrap-admin-team` is created automatically) | `src/bootstrap/services.rs:611` |

```bash
CRAWLRS_BOOTSTRAP_ADMIN=true ./target/release/crawlrs
# INFO  CRAWLRS_BOOTSTRAP_ADMIN=true, running auto-bootstrap...
# INFO  Bootstrap admin key issued: <key_id>.<secret>  (plaintext printed to TTY only)
```

> Note: both variables are documented in `.env.example` (`CRAWLRS_BOOTSTRAP_ADMIN` / `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY`). The plaintext key is printed to an interactive TTY once only — piped output is masked (CWE-532 by design), so in dev/bootstrap scenarios use `script -qefc ... /dev/null` to fake a TTY and capture the plaintext.

Keys are passed as a Bearer token in the `Authorization` header:

```bash
curl -H "Authorization: Bearer garrison_key_id.garrison_secret" http://localhost:8899/v1/scrape
```

> **⚠️ Security note:** the plaintext key is returned only once in the issuance response — store it in a secrets manager immediately.

### Garrison Authentication Migration (0.2.0)

**Impact:**

- Existing API Keys (based on the legacy `api_keys.key_hash` SHA-256 column) are **all invalidated** and must be re-issued through garrison
- The `scopes` table is read-only (marked with `deprecated_at`); the legacy scope mapping no longer applies
- `CRAWLRS__AUTH__JWT_SECRET` is required (HS256 ≥32 bytes; startup refuses weak secrets)

**Migration completed:**

- Legacy API Keys are invalidated; clients must use the `Authorization: Bearer <garrison_key_id>.<garrison_secret>` format
- New teams get API Keys issued via `POST /v1/admin/api-keys` with no ops-tooling involvement
- In dev/test environments, set `CRAWLRS_BOOTSTRAP_ADMIN=true` so the API process itself issues the admin key at startup (it must be the same process: garrison's DAO is in-process oxcache, so keys issued by another process are invisible)

**Behavior of `migrations/005_deprecate_legacy_api_keys.sql`:**

- Adds a `deprecated_at` column to the `api_keys` table and backfills it with `NOW()`
- Adds a `deprecated_at` column to the `scopes` table and backfills it with `NOW()`
- Deletes no data and drops no columns, easing rollback and historical audit

**Client migration:**

- The calling convention is unchanged: still `Authorization: Bearer <key>`
- Only replace `<key>` with the new `garrison_key_id.garrison_secret`
- The 401/429 response semantics are unchanged, but 429 triggering is now owned by garrison's `firewall-bruteforce`

See [ARCHITECTURE.md → Garrison Authentication Engine](ARCHITECTURE.md#garrison-authentication-engine) for the architecture details.

### Scopes

API keys are scoped to restrict access:

| Scope | Description |
|--------|-------------|
| `scrape` | Single page scraping |
| `crawl` | Multi-page crawling |
| `search` | Search engine queries |
| `extract` | Data extraction |
| `admin` | Full administrative access |

> **Since 0.2.0:** scopes are bridged from garrison RBAC permissions via `auth_bridge::map_perms_to_scope`, transparent to downstream business layers.

### Security Best Practices

- Never commit API keys to version control
- Use environment variables for API keys
- Rotate keys regularly
- Revoke unused keys
- Limit scopes to minimum required
- Restrict network access to the server port

### Rate Limiting

Rate limits are configured server-side. Default configuration:

```toml
[rate_limiting]
enabled = true
default_rpm = 60
burst_size = 20
```

Rate limit status is **not** exposed through response headers: normal responses carry no per-request quota headers (the Limit / Remaining / Reset style headers commonly seen in rate-limited APIs). Only the 429 response sets a header — `Retry-After` — telling you how long to wait before retrying.

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com"}' -i
# HTTP/1.1 429 Too Many Requests
# content-type: application/json
# retry-after: 60
```

> Known gap: `Retry-After` (429) is the only rate-limit response header that gets set (`src/presentation/middleware/rate_limit_middleware.rs:243-247`); quota-balance response headers are not implemented. To learn your remaining quota, poll `GET /v1/teams/me/usage` instead.

Check your current usage:

```bash
curl -X GET http://localhost:8899/v1/teams/me/usage \
  -H "Authorization: Bearer YOUR_API_KEY"
```

---

## 🏠 Single-Tenant / No-Auth Deployment

> **R-flags-005:** crawlrs supports a lightweight deployment mode with business capability features disabled. This is ideal for single-tenant self-hosted scenarios that do not require multi-tenant isolation, API Key authentication, rate limiting, or Webhook notifications.

### Overview

By default, crawlrs builds with all business capability features enabled (`default = ["platform", "db-postgres"]`, where `platform` expands to teams/auth/rate-limit/webhook/metrics/content/sitemap/http + dbnexus/axum). For simpler deployments (e.g., internal tools, single-user scraping services), you can disable these features to:

- Reduce binary size (no engines and no business-capability dependencies linked in)
- Eliminate database tables for teams/api_keys/webhooks
- Skip API Key verification (requests pass through with a fixed identity)
- Disable rate limiting (all requests allowed)
- Disable Webhook delivery (no `/v1/webhooks` routes, no `webhook_worker`)

### Building with No Default Features

```bash
# Single-tenant, no auth, no rate limiting, no Webhook
cargo build --release --no-default-features

# Single-tenant + rate limiting only (no auth, no Webhook)
cargo build --release --no-default-features --features rate-limit

# Multi-tenant + auth (no rate limiting, no Webhook)
cargo build --release --no-default-features --features teams
```

### Default Identity Constants

When `auth` is disabled, the `default_identity_middleware` injects a fixed `AuthState` into all requests, using these constants defined in `src/common/constants.rs`:

| Constant | Value | Description |
|----------|-------|-------------|
| `DEFAULT_TEAM_ID` | `domain::id::Id::from_u128(1)` | All requests are attributed to this team |
| `DEFAULT_API_KEY_ID` | `domain::id::Id::from_u128(2)` | Fixed API Key ID for audit logs |
| `DEFAULT_IDENTITY_TOKEN_HASH` | `"sha256:default_identity"` | Placeholder token hash for downstream extractors |

The injected `AuthState` carries `ApiKeyScope::denied()` (read/write/admin all `false`, limits `0`). Protected routes therefore **reject every request with 401** unless `allow_unauthenticated_protected()` is called explicitly; opt-in also yields the denied scope, not full access. Do not expose an auth-off instance to a public network.

Timeline: deny-by-default was introduced in 0.2.0-rc.1, lost during the 0.2.0-rc.2 assembly refactor, and restored in 0.2.0-rc.3 (see `docs/CHANGELOG.md`, Breaking).

### Noop Service Implementations

When business capability features are disabled, corresponding Noop implementations are injected via DI:

| Disabled Feature | Noop Implementation | Behavior |
|------------------|---------------------|----------|
| `rate-limit` | `NoopRateLimitingService` | `check_rate_limit` → `Allowed`; `check_and_deduct_quota` → `Ok(())`; `get_quota_balance` → `Ok(i64::MAX)` |
| `webhook` | `NoopWebhookService` | `trigger_completion` / `trigger_failure` → `Ok(())` (no-op) |

### Conditional Endpoints

When features are disabled, the corresponding routes are not registered (return 404):

| Endpoint | Required Feature | Disabled Behavior |
|----------|------------------|-------------------|
| `/v1/teams/me`, `/v1/teams/me/usage`, `/v1/teams/geo-restrictions` (GET/PUT) | `teams` | 404 Not Found |
| `/v1/extract` (with geo-restriction generic) | `teams` | Degrades to `extract` signature without geo-restrictions |
| `/v1/webhooks` (POST/GET) | `webhook` | 404 Not Found |

### Pre-provisioning Credits for Default Tenant

> **Important:** When `rate-limit` is enabled but `teams` is disabled, quota deductions still occur against `DEFAULT_TEAM_ID`. You must pre-provision credits for the default tenant before making requests, otherwise `check_and_deduct_quota` will return `PaymentRequired` (402).

Use direct SQL to provision the default tenant:

```bash
# Add 10000 credits to DEFAULT_TEAM_ID via psql
psql -d crawlrs -c "SELECT add_credits_safe('00000000-0000-0000-0000-000000000001', 10000, 'credit', 'Initial provisioning', NULL)"
```

### Configuration Example

`config/default.toml` for single-tenant + rate-limit deployment:

```toml
[database]
url = "postgresql://user:password@localhost/crawlrs"
max_connections = 20

[server]
host = "0.0.0.0"
port = 8899

[rate_limiting]
enabled = true
default_rpm = 60
burst_size = 20

# No [auth] section needed — auth feature disabled
# No [webhook] section needed — webhook feature disabled
```

**Configuration when `auth` is enabled (0.2.0 garrison-auth-migration):**

When the `auth` feature is enabled (the default), `CRAWLRS__AUTH__JWT_SECRET` must be set. Configure it via environment variable or TOML:

```bash
# Environment variable (recommended for containerized deployments)
export CRAWLRS__AUTH__JWT_SECRET="your-32-byte-or-longer-hs256-secret-key"
```

```toml
# config/default.toml approach
[auth]
jwt_secret = "your-32-byte-or-longer-hs256-secret-key"
```

> **⚠️ Strong-secret requirement:** the HS256 secret must be ≥32 bytes; garrison detects weak secrets at startup and refuses to start. Generate a random secret with `openssl rand -base64 48`.
>
> **Note:** the legacy `CRAWLRS__AUTH__KEYS` and `[auth] keys` config entries are deprecated and no longer take effect since 0.2.0. API Keys are now issued by garrison via `POST /v1/admin/api-keys`.

### Verification

After building with `--no-default-features`, verify the deployment:

```bash
# Health check (always available)
curl http://localhost:8899/health

# Scrape without Authorization header (auth disabled)
curl -X POST http://localhost:8899/v1/scrape \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com"}'

# Verify /v1/webhooks returns 404 (webhook disabled)
curl -X POST http://localhost:8899/v1/webhooks -d '{}'

# Verify /v1/teams/me returns 404 (teams disabled)
curl http://localhost:8899/v1/teams/me
```

### Migration from Multi-Tenant to Single-Tenant

If you are migrating from a multi-tenant deployment:

1. **Export your data** from the multi-tenant database (scrape results, crawl configurations)
2. **Re-import** into the single-tenant database, mapping all `team_id` values to `DEFAULT_TEAM_ID` (`00000000-0000-0000-0000-000000000001`)
3. **Rebuild** with `--no-default-features` (or with only the features you need)
4. **Pre-provision credits** for `DEFAULT_TEAM_ID` if `rate-limit` is enabled
5. **Verify** all endpoints behave as expected

---

## 🎯 Scraping

### Basic Scraping

Scrape a single page:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/article/123"
  }'
```

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "url": "https://example.com/article/123",
    "credits_used": 1
  }
}
```

### Get Scrape Results

```bash
curl http://localhost:8899/v1/scrape/550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "completed",
    "url": "https://example.com/article/123",
    "created_at": "2026-01-15T00:00:00Z",
    "completed_at": "2026-01-15T00:00:03Z",
    "result": {
      "content": "<html><body>...</body></html>",
      "status_code": 200,
      "content_type": "text/html; charset=utf-8",
      "response_time_ms": 812,
      "created_at": "2026-01-15T00:00:00Z"
    },
    "error": null
  }
}
```

> `result` is present only while `status` is `completed`. Its fields are `content` (raw HTML), `status_code`, `content_type`, `response_time_ms`, `headers` (sensitive headers redacted), `meta_data`, `screenshot` and `created_at`. `error` is populated when the task failed.

### Output Formats

Request data in multiple formats:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/article",
    "formats": ["html", "markdown", "text"]
  }'
```

**Available Formats:**
- `html` - Raw HTML content
- `markdown` - Converted to Markdown
- `text` - Plain text without HTML tags

### Content Filtering

Include or exclude specific HTML tags:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/blog",
    "include_tags": ["h1", "h2", "p", "article"],
    "exclude_tags": ["script", "style", "nav", "footer"]
  }'
```

### Extraction Rules

Extract specific data using CSS selectors:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/product",
    "extraction_rules": {
      "title": {
        "selector": "h1",
        "is_array": false
      },
      "price": {
        "selector": ".price",
        "is_array": false
      },
      "description": {
        "selector": ".description",
        "is_array": false
      },
      "imageUrl": {
        "selector": ".product-image img",
        "attr": "src",
        "is_array": false
      },
      "inStock": {
        "selector": "#stock-status",
        "attr": "data-stock",
        "is_array": false
      }
    }
  }'
```

**Result:**
```json
{
  "success": true,
  "data": {
    "title": "Premium Widget",
    "price": "$99.99",
    "description": "A high-quality widget...",
    "imageUrl": "https://example.com/images/widget.jpg",
    "inStock": "yes"
  }
}
```

### Custom Options

Configure how the scraper fetches the page:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/page",
    "options": {
      "headers": {
        "User-Agent": "Mozilla/5.0 (compatible; MyBot/1.0)",
        "Accept-Language": "en-US,en;q=0.9"
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
      "skip_tls_verification": false
    }
  }'
```

### Page Actions

Perform actions on the page before scraping:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/lazy-load-page",
    "options": {
      "js_rendering": true
    },
    "actions": [
      {
        "type": "wait",
        "milliseconds": 1000
      },
      {
        "type": "scroll",
        "direction": "down"
      },
      {
        "type": "wait",
        "milliseconds": 2000
      },
      {
        "type": "click",
        "selector": ".load-more-button"
      },
      {
        "type": "wait",
        "milliseconds": 3000
      },
      {
        "type": "scroll",
        "direction": "down"
      },
      {
        "type": "input",
        "selector": "#search-input",
        "text": "search query"
      },
      {
        "type": "screenshot",
        "full_page": true
      }
    ]
  }'
```

**Action Types:**

| Action | Parameters | Description |
|--------|-----------|-------------|
| `wait` | `milliseconds` | Wait before next action |
| `scroll` | `direction` | Scroll page (up/down) |
| `click` | `selector` | Click element matching selector |
| `input` | `selector`, `text` | Type text into input field |
| `screenshot` | `full_page` | Take screenshot |

### Synchronous Scraping

Wait for the scrape to complete and get results immediately:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/article",
    "sync_wait_ms": 10000
  }'
```

**Best Practices:**
- Use `sync_wait_ms` only when you need immediate results
- Recommended max: 5000ms for most cases
- Maximum: 30000ms
- For long-running tasks, use webhooks instead

### Cancel a Scrape

Stop a running scrape task:

```bash
curl -X POST http://localhost:8899/v1/scrape/550e8400-e29b-41d4-a716-446655440000/_cancel \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Response:**
```json
{
  "success": true,
  "data": {
    "message": "Scrape task cancelled."
  }
}
```

> `POST /v1/scrape/{id}/_cancel` is registered in the production route assembly (`src/bootstrap/route_groups/scrape_routes.rs:19-27`), alongside a regression test that asserts the route is registered (`src/bootstrap/route_groups/scrape_routes.rs:38-56`).

---

## 🕷️ Crawling

### Basic Crawling

Crawl a website starting from a URL:

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/blog",
    "config": {
      "max_depth": 2,
      "max_pages": 100
    }
  }'
```

> Crawl parameters such as `max_depth` (required), `max_pages`, `include_patterns`, and `exclude_patterns` live in the `config` sub-object; the request body is validated with `deny_unknown_fields`, and the top level does not accept `formats` / `webhook` / `options` / `follow_links` fields.

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "team_id": "660e8400-e29b-41d4-a716-446655440001",
    "name": "blog",
    "root_url": "https://example.com/blog",
    "url": "https://example.com/blog",
    "status": "queued",
    "config": { "max_depth": 2, "max_pages": 100 },
    "total_tasks": 0,
    "completed_tasks": 0,
    "failed_tasks": 0,
    "created_at": "2026-01-15T00:00:00Z",
    "updated_at": "2026-01-15T00:00:00Z",
    "completed_at": null
  }
}
```

### Crawl Depth

Control how deep the crawler goes:

| Depth | Description | Example |
|-------|-------------|----------|
| 0 | Only the start page | Home page only |
| 1 | Start page + linked pages | Home + 1 link deep |
| 2 | 2 levels deep | Home → Category → Article |
| 3+ | Deep crawling | Full site discovery |

**Example:**

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "config": {
      "max_depth": 3,
      "max_pages": 500
    }
  }'
```

### URL Patterns

Control which URLs to crawl:

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "config": {
      "max_depth": 3,
      "include_patterns": ["/blog/.*", "/articles/.*"],
      "exclude_patterns": ["/admin/.*", "/login/.*", "/api/.*"]
    }
  }'
```

> URL filtering runs automatically through the link-discovery mechanism (crawling follows page links); there is no `follow_links` field.

**Pattern Examples:**
- `/blog/.*` - Include all blog posts
- `/category/[a-z]+` - Include category pages
- `\.pdf$` - Include PDF files
- `/admin/.*` - Exclude admin pages
- `\\?.*` - Exclude URLs with query strings

### Crawl Progress

Track crawl progress:

```bash
# Poll for status
curl http://localhost:8899/v1/crawl/550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Response:**

```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "team_id": "660e8400-e29b-41d4-a716-446655440001",
    "name": "blog",
    "root_url": "https://example.com/blog",
    "url": "https://example.com/blog",
    "status": "processing",
    "config": { "max_depth": 2, "max_pages": 100 },
    "total_tasks": 100,
    "completed_tasks": 45,
    "failed_tasks": 0,
    "created_at": "2026-01-15T00:00:00Z",
    "updated_at": "2026-01-15T00:00:20Z",
    "completed_at": null
  }
}
```

> The response is the bare `Crawl` model in `data` (no `task` / `progress` wrapper). Progress is tracked through `completed_tasks` / `failed_tasks` / `total_tasks`.

### Get Crawl Results

Retrieve all pages crawled:

```bash
curl http://localhost:8899/v1/crawl/550e8400-e29b-41d4-a716-446655440000/results \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Response:**

```json
{
  "success": true,
  "data": [
    {
      "id": "780e8400-e29b-41d4-a716-446655440003",
      "task_id": "770e8400-e29b-41d4-a716-446655440002",
      "url": "https://example.com/blog/post-1",
      "content": "<html>...</html>",
      "status_code": 200,
      "content_type": "text/html; charset=utf-8",
      "headers": {},
      "meta_data": {},
      "screenshot": null,
      "response_time_ms": 812,
      "created_at": "2026-01-15T00:00:00Z"
    }
  ]
}
```

> `data` is a bare array of scrape results (one per crawled sub-task). There is no `page` / `limit` / `pagination` wrapper — `GET /v1/crawl/{id}/results` returns every result for the crawl in a single response. For converted content, request the `markdown` format when creating the crawl and read it from `meta_data`.

### Cancel a Crawl

Stop a running crawl via `DELETE /v1/crawl/{id}`:

```bash
curl -X DELETE http://localhost:8899/v1/crawl/550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer YOUR_API_KEY"
```

> Cancel crawls with `DELETE /v1/crawl/{id}`. The `POST /v1/crawl/{id}/_cancel` form exists only in the test route assembly (`src/presentation/routes/handlers.rs:47`, a unit-test helper explicitly not used by production) — production registers only the `DELETE` form (`src/bootstrap/route_groups/crawl_routes.rs:18-27`).

---

## 🔍 Searching

### Basic Search

Search using Bing:

```bash
curl -X POST http://localhost:8899/v1/search \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "engine": "bing",
    "query": "Rust web scraping tutorial"
  }'
```

**Response:**

```json
{
  "success": true,
  "data": {
    "query": "Rust web scraping tutorial",
    "results": [
      {
        "title": "Rust Web Scraping Tutorial",
        "url": "https://example.com/rust-scraping",
        "description": "Learn to scrape the web with Rust...",
        "engine": "bing"
      }
    ],
    "crawl_id": null,
    "credits_used": 1
  },
  "timestamp": "2026-10-06T12:00:00+00:00"
}
```

### Search Engines

Available search engines:

| Engine | Code | Best For |
|--------|-------|-----------|
| Bing | `bing` | General web search |
| Baidu | `baidu` | Chinese content |
| Sogou | `sogou` | Chinese content |
| Smart | `smart` | Aggregated multi-engine search with RRF fusion |
| Exa | `exa` | AI-powered search (requires API key) |
| Tavily | `tavily` | AI-powered search (requires API key) |
| Parallel | `parallel` | Fallback engine (MCP endpoint, `src/config/search.rs` `FallbackEngineConfig for parallel`) |

### Search Options

Customize your search:

```bash
curl -X POST http://localhost:8899/v1/search \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "engine": "bing",
    "query": "Rust programming",
    "limit": 20,
    "lang": "en",
    "country": "us",
    "sync_wait_ms": 5000
  }'
```

**Languages / regions (common example values — not yet effective in the current version):**

`lang` and `country` are free-form strings in the request DTO (`src/application/dto/search_request.rs:17-18`), and `perform_search` currently ignores them (`src/domain/services/search_service.rs:284-285` — parameters are prefixed `_`). Common values you can send:

- **Languages:** `en` (English), `zh` (Chinese), `es` (Spanish), `fr` (French), `de` (German), `ja` (Japanese), `ko` (Korean)
- **Regions:** `us` (United States), `uk` (United Kingdom), `ca` (Canada), `au` (Australia), `jp` (Japan), `cn` (China)

> ⚠️ Known gap: the search backends do not consume `lang` / `country` in this release — passing them is accepted (no validation error) but has no effect on results. Documented as a known gap; do not rely on it for language or geo targeting.

### Get Search Results

```bash
# Synchronous (wait for completion)
curl -X POST http://localhost:8899/v1/search \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "engine": "bing",
    "query": "Rust tutorial",
    "sync_wait_ms": 5000
  }'
```

**Response:**

```json
{
  "success": true,
  "data": {
    "query": "Rust tutorial",
    "results": [
      {
        "title": "Rust Programming Tutorial",
        "url": "https://example.com/rust-tutorial",
        "description": "Learn Rust programming...",
        "engine": "bing"
      }
    ],
    "crawl_id": null,
    "credits_used": 1
  },
  "timestamp": "2026-10-06T12:00:00+00:00"
}
```

### Semantic Re-ranking (optional)

Search results are ordered statistically by default (RRF fusion across engines plus heuristic
relevance). To order by semantic query-document relevance, enable the `[rag]` section and build
with the matching feature:

| Build feature | Provider | What it does |
|---------------|----------|--------------|
| `rag-local` | vecboost (local model) | One bi-encoder model serves both embedding and re-ranking; no network calls |
| *(none)* | generic HTTP re-rank | `rerank_provider = "http"` works with the default build — only `reqwest` is required |

`rerank_provider = "http"` with `rerank_format = "cohere"` (Cohere v2, Jina, most self-hosted)
or `"tei"` (text-embeddings-inference). API keys are injected via environment, never config files:

```bash
export CRAWLRS__RAG__RERANK_API_KEY=...
```

When re-ranking is active, every result carries a `score` field (0..1). When it is off, the field
is omitted entirely, so existing clients keep parsing unchanged. Provider failures fail open: the
original order is returned. Configuring a provider whose feature was not compiled fails at startup
instead of silently degrading.

```json
{
  "results": [
    { "title": "...", "url": "...", "engine": "bing", "score": 0.8731 }
  ]
}
```

---

## 📊 Data Extraction

### Extract from URLs

Submit one or more URLs for asynchronous extraction. The request body takes `urls` (1–100, http/https) plus exactly one of `prompt` / `schema` / `rules`; the endpoint enqueues a task and returns `{id, status: "pending"}` (see `docs/API_REFERENCE.md` for the full contract):

```bash
curl -X POST http://localhost:8899/v1/extract \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "urls": ["https://example.com/article"],
    "prompt": "Extract the article title and body text"
  }'
```

Using extraction rules instead of a prompt:

```bash
curl -X POST http://localhost:8899/v1/extract \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "urls": ["https://example.com/article"],
    "rules": {
      "title": { "selector": "h1", "is_array": false },
      "content": { "selector": "p", "is_array": false }
    }
  }'
```

**Response (`202 Accepted` when `sync_wait_ms` elapses before the task finishes):**

```json
{
  "id": "01932f4a-1b2c-7d8e-9f0a-3b4c5d6e7f80",
  "status": "pending"
}
```
```

### Advanced Extraction

Extract multiple elements from a URL:

```bash
curl -X POST http://localhost:8899/v1/extract \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "urls": ["https://example.com"],
    "extraction_rules": {
      "headings": {
        "selector": "h1, h2, h3",
        "is_array": true
      },
      "links": {
        "selector": "a",
        "attr": "href",
        "is_array": true
      },
      "images": {
        "selector": "img",
        "attr": "src",
        "is_array": true
      },
      "meta_description": {
        "selector": "meta[name=\"description\"]",
        "attr": "content",
        "is_array": false
      }
    }
  }'
```

---

## 🪝 Webhooks

### Create Webhook

Receive notifications when tasks complete:

```bash
curl -X POST http://localhost:8899/v1/webhooks \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -H "webhook-id: msg_1" \
  -H "webhook-timestamp: 1760000000" \
  -H "webhook-signature: v1,<base64-hmac-sha256>" \
  -d '{
    "url": "https://your-server.com/webhook"
  }'
```

> The request body accepts only the `url` field (`deny_unknown_fields`; passing `events` / `secret` / `active` is rejected with `400 VALIDATION_ERROR`). The signing secret is configured server-side (`CRAWLRS__WEBHOOK__SECRET`), and the creation request itself must carry Standard Webhooks signature headers computed with that secret, otherwise 401 is returned.

**Response (`201 Created`, standard `{success, data}` envelope):**

```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "team_id": "660e8400-2b3c-5d7e-8f9a-0b1c2d3e4f50",
    "url": "https://your-server.com/webhook",
    "created_at": "2026-01-15T00:00:00Z",
    "is_active": true,
    "secret": "whsec_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
  }
}
```

> The `secret` is returned only once in the creation response and never in list endpoints; save it immediately — it cannot be read again later.

### List Webhooks

```bash
curl http://localhost:8899/v1/webhooks \
  -H "Authorization: Bearer YOUR_API_KEY"
```

### Webhook Events

The server dispatches events automatically by task type (no subscription needed):

| Event | Trigger | Use Case |
|--------|----------|-----------|
| `scrape.completed` | Scrape task succeeded | Result processing |
| `scrape.failed` | Scrape task failed | Error handling |
| `crawl.completed` | Crawl task succeeded | Result processing |
| `crawl.failed` | Crawl task failed | Error handling |
| `extract.completed` | Extract task succeeded | Result processing |
| `extract.failed` | Extract task failed | Error handling |

### Handle Webhook

Create a receiver and verify signatures per the Standard Webhooks spec (the signed message is `{webhook-id}.{webhook-timestamp}.{payload}`, HMAC-SHA256 then Base64 encoded, with a `v1,` prefix):

```javascript
// Express.js example
const express = require('express');
const app = express();
const crypto = require('crypto');

app.post('/webhook', express.raw({type: 'application/json'}), (req, res) => {
  const msgId = req.headers['webhook-id'];
  const timestamp = req.headers['webhook-timestamp'];
  const signature = req.headers['webhook-signature'];
  const payload = req.body;

  const expected = crypto
    .createHmac('sha256', 'your-webhook-secret')
    .update(`${msgId}.${timestamp}.${payload.toString()}`)
    .digest('base64');

  const expectedSig = `v1,${expected}`;
  const a = Buffer.from(expectedSig);
  const b = Buffer.from(signature || '');
  if (a.length !== b.length || !crypto.timingSafeEqual(a, b)) {
    return res.status(401).json({ error: 'Invalid signature' });
  }

  const event = JSON.parse(payload);
  console.log('Task:', event.task_id, 'Status:', event.status);

  res.status(200).send('OK');
});

app.listen(3000, () => {
  console.log('Webhook server running on port 3000');
});
```

**Webhook Payload:**

```json
{
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "completed",
  "url": "https://example.com",
  "timestamp": 1760000000
}
```

On failure the event's `status` is `"failed"` with an `"error"` field attached.

### Webhook Retry Policy

| Failure class | Retryable? |
|---------------|------------|
| Network errors (connection refused / reset, DNS failure, broken pipe, unreachable), timeouts | Yes |
| `500`, `502`, `503`, `504` | Yes |
| `429` (rate limit at the receiving end) | Yes |
| Other `4xx` (400, 401, 403, 404, ...) | No — moved straight to dead letter |
| 2xx | Success, no retry |

**Retry strategy** (`src/utils/retry_policy.rs`, `RetryPolicy::default()`):
- exponential backoff with full jitter, starting at `initial_backoff = 1s`, `max_backoff = 60s`, `backoff_multiplier = 2.0`
- `max_retries = 5`
- after the 5th failed attempt the event is marked `dead` and lands in the dead-letter queue (no further retries)

`src/workers/webhook_worker.rs:53` builds the worker with this default policy (`WebhookWorker::with_default_policy`).

---

## 📋 Tasks

> `/v1/tasks/_query` and `/v1/tasks/_cancel` are registered in the production assembly (`src/presentation/routes/task.rs:20-30`, wired in `src/bootstrap/routes.rs:396`).

### Query Tasks

Search and filter tasks across all types:

```bash
curl -X POST http://localhost:8899/v1/tasks/_query \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "statuses": ["queued", "active"],
    "task_types": ["scrape"],
    "limit": 20,
    "offset": 0
  }'
```

### Cancel Task

Cancel queued or active tasks by ID (pass an array to `task_ids`):

```bash
curl -X POST http://localhost:8899/v1/tasks/_cancel \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "task_ids": ["550e8400-e29b-41d4-a716-446655440000", "660e8400-2b3c-5d7e-8f9a-0b1c2d3e4f50"]
  }'
```

---

## 👥 Teams & Usage

### Get Team Info

```bash
curl http://localhost:8899/v1/teams/me \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Response:**

```json
{
  "success": true,
  "data": {
    "id": "660e8400-e29b-41d4-a716-446655440001",
    "name": "Team",
    "credits_balance": 10000,
    "total_tasks": 0,
    "completed_tasks": 0,
    "failed_tasks": 0,
    "created_at": "2026-01-15T00:00:00Z"
  }
}
```

> `data` is the bare `TeamInfoResponse` (`src/presentation/handlers/team_handler.rs:22-31`). `completed_tasks` / `failed_tasks` are currently always `0` in the implementation — a known gap, not a data guarantee.

### Get Usage Stats

```bash
curl http://localhost:8899/v1/teams/me/usage \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Response:**

```json
{
  "success": true,
  "data": {
    "team_id": "660e8400-e29b-41d4-a716-446655440001",
    "period": "30d",
    "total_requests": 0,
    "successful_requests": 0,
    "failed_requests": 0,
    "credits_used": 0,
    "avg_response_time_ms": 0
  }
}
```

> Field semantics (`src/presentation/handlers/team_handler.rs`, `TeamUsageResponse`): `team_id` is the authenticated team; `period` is fixed to `"30d"`; `total_requests` / `successful_requests` / `failed_requests` are always `0` in the current implementation (time-series accounting is not wired up yet — a known gap); `credits_used` is the absolute value of the credit balance (a negative balance means consumed); `avg_response_time_ms` comes from `get_team_avg_response_time` and is `0.0` when no data exists.

---

## ⚠️ Error Handling

### Common Errors

**1. Rate Limit Exceeded**

```json
{
  "success": false,
  "error": "Rate limit exceeded: 60 requests/minute"
}
```

**Solution:**
```javascript
try {
  const response = await axios.post(url, data, {
    headers: { 'Authorization': `Bearer ${API_KEY}` }
  });
} catch (error) {
  if (error.response?.status === 429) {
    const retryAfter = error.response.headers['retry-after'];
    await new Promise(resolve => setTimeout(resolve, retryAfter * 1000));
    return makeRequest(url, data);
  }
  throw error;
}
```

**2. Invalid URL**

```json
{
  "success": false,
  "error": "Invalid URL: must be http:// or https://"
}
```

**Solution:**
```javascript
function isValidUrl(url) {
  try {
    new URL(url);
    return url.startsWith('http://') || url.startsWith('https://');
  } catch {
    return false;
  }
}

if (!isValidUrl(userUrl)) {
  throw new Error('Invalid URL format');
}
```

**3. SSRF Blocked**

```json
{
  "success": false,
  "error": "SSRF protection: Internal URLs are not allowed"
}
```

**Solution:**
- Never request internal URLs (localhost, 127.0.0.1, private IPs)
- Always validate URLs before sending

**4. Authentication Failed**

```json
{
  "success": false,
  "error": "Invalid or missing API key"
}
```

**Solution:**
- Verify the `Authorization` header is set correctly
- Check the API key is configured on the server

### Error Handling Best Practices

1. **Always Check `success` Field**

```javascript
const response = await makeRequest();
if (!response.data.success) {
  console.error('API error:', response.data.error);
}
```

2. **Use Exponential Backoff for Retries**

```javascript
async function retryWithBackoff(fn, maxRetries = 3) {
  for (let i = 0; i < maxRetries; i++) {
    try {
      return await fn();
    } catch (error) {
      if (i === maxRetries - 1) throw error;
      const delay = Math.pow(2, i) * 1000;
      await new Promise(resolve => setTimeout(resolve, delay));
    }
  }
}
```

3. **Implement Circuit Breaker**

```javascript
class CircuitBreaker {
  constructor(threshold = 5, resetTimeout = 60000) {
    this.failureCount = 0;
    this.threshold = threshold;
    this.state = 'closed';
    this.nextAttempt = Date.now();
    this.resetTimeout = resetTimeout;
  }

  async execute(fn) {
    if (this.state === 'open' && Date.now() < this.nextAttempt) {
      throw new Error('Circuit breaker is open');
    }

    try {
      const result = await fn();
      this.onSuccess();
      return result;
    } catch (error) {
      this.onFailure();
      throw error;
    }
  }

  onSuccess() {
    this.failureCount = 0;
    this.state = 'closed';
  }

  onFailure() {
    this.failureCount++;
    if (this.failureCount >= this.threshold) {
      this.state = 'open';
      this.nextAttempt = Date.now() + this.resetTimeout;
    }
  }
}
```

---

## ✅ Best Practices

### 1. Use Async Mode for Long-Running Tasks

**Don't:**

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "sync_wait_ms": 30000}'
```

**Do:** (only scrape has a top-level `webhook` field; the crawl request body is validated with `deny_unknown_fields`, so a top-level `webhook` field yields 422 directly)

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "webhook": "https://your-server.com/callback"}'
```

### 2. Implement Proper Retry Logic

```javascript
async function makeRequestWithRetry(url, data, retries = 3) {
  for (let i = 0; i < retries; i++) {
    try {
      const response = await axios.post(url, data, {
        headers: { 'Authorization': `Bearer ${API_KEY}` },
        timeout: 30000
      });
      return response.data;
    } catch (error) {
      if (error.response?.status >= 400 && error.response?.status < 500 && error.response?.status !== 429) {
        throw error;
      }

      const delay = Math.pow(2, i) * 1000;
      await new Promise(resolve => setTimeout(resolve, delay));
    }
  }
}
```

### 3. Cache Results When Appropriate

```javascript
const cache = new Map();

async function scrapeWithCache(url) {
  const cacheKey = `scrape:${url}`;

  if (cache.has(cacheKey)) {
    return cache.get(cacheKey);
  }

  const response = await axios.post('http://localhost:8899/v1/scrape', {
    url: url,
    formats: ['markdown']
  }, {
    headers: { 'Authorization': `Bearer ${API_KEY}` }
  });

  cache.set(cacheKey, response.data);
  return response.data;
}
```

### 4. Use Specific Output Formats

**Don't:**

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "formats": ["html", "markdown", "text"]}'
```

**Do:**

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "formats": ["markdown"]}'
```

### 5. Monitor Server Usage

```bash
# Check usage regularly
curl http://localhost:8899/v1/teams/me/usage \
  -H "Authorization: Bearer YOUR_API_KEY"
```

### 6. Validate Inputs Before Sending

```javascript
function validateScrapeRequest(request) {
  const errors = [];

  if (!request.url) {
    errors.push('URL is required');
  }

  if (!isValidUrl(request.url)) {
    errors.push('Invalid URL format');
  }

  if (request.sync_wait_ms && request.sync_wait_ms > 30000) {
    errors.push('sync_wait_ms must be <= 30000');
  }

  if (errors.length > 0) {
    throw new Error(`Validation errors: ${errors.join(', ')}`);
  }

  return true;
}
```

### 7. Use Environment Variables for Secrets

```bash
# .env file — server-side configuration
CRAWLRS__SERVER__PORT=8899
CRAWLRS__DATABASE__URL=postgres://user:pass@localhost/crawlrs
# Webhook signing secret (not issued server-side; you configure it and use it to compute/verify Standard Webhooks signatures)
CRAWLRS__WEBHOOK__SECRET=whsec-your-webhook-secret
```

> The API Key is **not** a server-side config entry: it is issued by `POST /v1/admin/api-keys` (or bootstrapped with `CRAWLRS_BOOTSTRAP_ADMIN=true`), and the plaintext is returned only once in the issuance response. Clients store it in their own secrets manager; the server has no environment variable for reading client keys and no path for writing a key into server configuration.

```javascript
require('dotenv').config();

// API Key comes from the issuance response; store it in your client-side secrets manager / environment
const API_KEY = process.env.MY_APP_API_KEY;
const WEBHOOK_SECRET = process.env.CRAWLRS__WEBHOOK__SECRET;

const response = await axios.post('http://localhost:8899/v1/scrape', data, {
  headers: { 'Authorization': `Bearer ${API_KEY}` }
});
```

**.gitignore:**

```
.env
.env.local
.env.production
```

### 8. Use Appropriate Engines

| Scenario | Recommended Engine | Reason |
|----------|-------------------|----------|
| Static HTML pages | Reqwest | Fastest, lowest overhead |
| JavaScript-heavy SPAs | Playwright | Renders JS |
| Anti-bot protection | FlareSolverr (Full/Cdp) | Bypasses detection |
| Cloudflare-protected sites | FlareSolverr | Solves challenge |
| Simple data extraction | Reqwest | Fast, efficient |
| Screenshots needed | Playwright | Full page rendering |
| High-volume scraping | Reqwest | Best performance |

---

## 🔧 Troubleshooting

### Issues & Solutions

**1. "Rate limit exceeded" frequently**

**Problem:** Getting 429 errors even with low usage

**Solutions:**
- Check if multiple clients are using the same API key
- Implement proper rate limiting in your application
- Increase `default_rpm` in your server config (`[rate_limiting]` section)
- Use async mode (webhooks) instead of sync waiting

**2. "SSRF protection: Internal URLs are not allowed"**

**Problem:** Request to internal URL is blocked

**Solutions:**
- Ensure URL is a public internet URL
- Never use localhost, 127.0.0.1, or internal IPs
- Validate URLs before sending to API

**3. Task stuck in "active" status**

**Problem:** Task never completes

**Solutions:**
- Check if the target URL is accessible
- Verify the URL isn't blocking bots
- Try with a different engine (Playwright vs Reqwest vs FlareSolverr)
- Check your webhook endpoint is responding
- Check worker logs if running in worker mode

**4. Webhook not being called**

**Problem:** Tasks complete but webhook not triggered

**Solutions:**
- Verify webhook URL is accessible from the crawlrs server
- Check your server logs for incoming requests
- Verify webhook signature is being sent correctly
- Test webhook endpoint manually:

  ```bash
  curl -X POST https://your-server.com/webhook \
    -H 'Content-Type: application/json' \
    -d '{"event":"test"}'
  ```

**5. Slow response times**

**Problem:** API requests take longer than expected

**Solutions:**
- Use synchronous mode only when necessary
- Reduce payload size (fewer formats, smaller sync_wait_ms)
- Use caching for repeated requests
- Check your network latency to the crawlrs server
- Ensure the server has sufficient resources (CPU, memory)

**6. Engine not available**

**Problem:** Requested engine returns an error

**Solutions:**
- Verify FlareSolverr is running and accessible if using that engine
- Check Playwright browsers are installed (`npx playwright install`)
- Ensure Reqwest engine is compiled in (default)
- Check server logs for engine-specific errors

---

## ❓ FAQ

### General Questions

**Q: What is the difference between scrape and crawl?**

A: **Scrape** extracts content from a single page. **Crawl** automatically discovers and scrapes multiple linked pages starting from a URL.

**Q: How do I install and run crawlrs?**

A: crawlrs is a single Rust binary. Clone the repo, run `cargo build --release`, then execute `./target/release/crawlrs`. See the [Getting Started](#-getting-started) section for details. The server starts on `http://localhost:8899` by default.

**Q: Can I scrape any website?**

A: Most websites, but the server respects `robots.txt` and may block sites that explicitly prohibit scraping. Always check a website's Terms of Service.

**Q: What happens if a task fails?**

A: Failed tasks are logged and you receive a webhook notification (if configured). You can retry the task with the same parameters.

### Technical Questions

**Q: How do I handle JavaScript-rendered content?**

A: Use Playwright engine by setting `js_rendering: true` in options:

```json
{
  "options": {
    "js_rendering": true
  }
}
```

**Q: How do I bypass Cloudflare protection?**

A: Use the FlareSolverr engine. Choose the appropriate mode (`Full`, `Cdp`, or `Tls`) based on your needs:

```json
{
  "options": {
    "engine": "flaresolverr",
    "flaresolverr_mode": "Full"
  }
}
```

**Q: Can I use a proxy?**

A: Yes, specify a proxy in options:

```json
{
  "options": {
    "proxy": "http://user:pass@proxy.example.com:8080"
  }
}
```

**Q: How do I extract data from multiple elements?**

A: Use `is_array: true` in extraction rules (with `attr` for the attribute to collect when it is not the element text):

```json
{
  "extraction_rules": {
    "links": {
      "selector": "a",
      "attr": "href",
      "is_array": true
    }
  }
}
```

**Q: What is the maximum sync_wait_ms?**

A: Maximum is 30,000 milliseconds (30 seconds). For longer tasks, use webhooks for async processing.

**Q: How do I configure rate limits?**

A: Rate limits are set in the server config file (`config/default.toml`):

```toml
[rate_limiting]
enabled = true
default_rpm = 60
burst_size = 20
```

---

## 📞 Support

### Getting Help

- 📖 [Documentation](../../README.md)
- 📚 [API Reference](API_REFERENCE.md)
- 🏗️ [Architecture](ARCHITECTURE.md)
- 🐛 [Issue Tracker](https://github.com/Kirky-X/crawlrs/issues)
- 📧 Email: Kirky-X@outlook.com

### Reporting Bugs

When reporting bugs, include:
1. API endpoint used
2. Request parameters (sanitized)
3. Expected vs actual behavior
4. Error messages
5. Server logs if available
6. Reproduction steps

### Feature Requests

Submit feature requests via:
- GitHub Issues (with "enhancement" label)
- Email to maintainer

---

## 🎨 Feature Flags

> This section was archived from the README: this is the authoritative reference for build presets and feature semantics.

### 📦 Build Presets

| Preset | Build command | Includes | Use case |
|------|----------|----------|----------|
| default | `cargo build --release` | `platform` (teams + auth + rate-limit + webhook + metrics + content + sitemap) + `db-postgres` | the complete platform out of the box |
| `standard` | `cargo build --release --features standard` | default + `engine-playwright` + `sitemap` | production recommended (JS rendering + metrics + content processing + sitemap) |
| `full` | `cargo build --release --features full` | `standard` + `engine-flaresolverr` + `extractors` + `llm` | full-feature combination (no vision/MLLM; add `engine-mllm` separately) |
| `no-default` | `cargo build --release --no-default-features` | pure core scraping stack | single-tenant/no-auth deployments, embedding |
| `agent-lib` | `cargo build --release --no-default-features --features agent-lib` | `content` + `trafilatura` + `dom-smoothie` | the minimal agent/embedded library surface |

### 📋 Feature Matrix

The table below is the main feature subset of `Cargo.toml`'s `[features]` (the full list lives in `Cargo.toml`), with `default = ["platform", "db-postgres"]`; the core scraping stack (oxcache / dbnexus / confers / sdforge / inklog / trait-kit + scraper / reqwest / robotstxt) is always compiled.

| Feature | Description | Default |
|------|------|------|
| `teams` | multi-tenant isolation (implies `auth`); when off, everything belongs to the single-tenant `DEFAULT_TEAM_ID` | ✅ |
| `auth` | garrison v0.9 authentication (RBAC + JWT + brute-force protection); when off, a fixed identity is injected | ✅ |
| `rate-limit` | limiteron rate limiting and circuit breaking; when off, `NoopRateLimitingService` is injected | ✅ |
| `webhook` | webhook delivery; when off, the `/v1/webhooks` routes are removed and a Noop is injected | ✅ |
| `metrics` | Prometheus metrics export (with sysinfo memory awareness) | ✅ (platform) |
| `content` | anti-bot detection (aho-corasick) + HTML→Markdown conversion (the alchemize html engine is the default; htmd is the pass-through diagnostic arm) | ✅ (platform) |
| `db-postgres` / `db-sqlite` / `db-mysql` | pick exactly one database driver (compile-time mutual exclusion enforced by dbnexus) | ✅ postgres |
| `engine-playwright` | chromiumoxide browser automation engine | ❌ |
| `engine-flaresolverr` | FlareSolverr anti-bot engine (Full/Cdp/Tls three modes) | ❌ |
| `engine-tls-fingerprint` | WreqEngine TLS fingerprint spoofing (BoringSSL JA3/JA4) | ❌ |
| `engine-mllm` | MLLM vision-LLM autonomous navigation engine (implies `engine-playwright` + `llm`; the vision surface goes through fluxweir chat) | ❌ |
| `engine-moli` | MoliEngine (a pure-Rust headless kernel controlled via CDP as an external `moli serve`) + memory-watermark scheduler (sysinfo's three levels — Abundant/Tight/Critical — drive engine priority and Playwright pool gating) | ❌ |
| `trafilatura` / `dom-smoothie` / `extractors` | main content-extraction path / performance fallback / all enabled | ❌ |
| `llm` | the LLM extraction boundary (trait + orchestration; vendor access is injected by a host carrier) | ❌ |
| `sitemap` | sitemap generation (the `/v1/map` endpoint) | ✅ (platform/standard) |
| `rag-local` | local RAG embedding and rerank (vecboost) | ❌ |
| `rag` | the RAG bundle (`rag-local` + `gateway`) | ❌ |
| `openapi` | OpenAPI 3.1 spec generation (`GET /openapi.json`) | ❌ |
| `test-mocks` | test-only mock gate (integration tests need it enabled explicitly) | ❌ |

> For the compile matrix of feature combinations (22 combinations) and how they are verified, see [🧪 Test Scenario Matrix](TEST_SCENARIOS.md); for the feature-gating implementation pattern, see [🏗️ Architecture · Feature Gate Architecture](ARCHITECTURE.md).

---

**Happy Scraping! 🚀**
