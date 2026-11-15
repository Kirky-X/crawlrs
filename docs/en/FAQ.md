# ❓ Crawlrs FAQ

This page collects frequently asked questions about crawlrs, grouped by topic. All answers are based on the repository's actual code and configuration. No answer found? Feel free to ask in [GitHub Issues](https://github.com/Kirky-X/crawlrs/issues).

## 📋 Table of Contents

- [🧭 General](#-general)
- [📦 Installation and Configuration](#-installation-and-configuration)
- [🔐 Authentication and Multi-Tenancy](#-authentication-and-multi-tenancy)
- [🚂 Engines and Scraping](#-engines-and-scraping)
- [🧪 Testing and Deployment](#-testing-and-deployment)

---

## 🧭 General

### ❓ What is crawlrs?

crawlrs is a self-hosted enterprise-grade web scraping platform built with Rust, offering five capabilities: unified search (Bing/Baidu/Sogou + Smart aggregation, with optional Exa/Tavily), single-page scraping, multi-page crawling, data extraction, and site mapping, plus built-in platform capabilities such as multi-tenancy, authentication, rate limiting, webhooks, and metrics. For the full architecture and capability landscape see [🏗️ Architecture](ARCHITECTURE.md).

### ❓ Why is it implemented in Rust?

Zero-cost abstractions and fearless concurrency deliver high throughput and low tail latency; strong typing and trait contracts (`ScraperEngine` etc.) make engine extension safe and controlled; a single static binary is easy to deploy.

### ❓ Is it production-ready?

Yes. The project ships enterprise-grade support: garrison authentication (RBAC + JWT + brute-force protection), limiteron rate limiting and circuit breaking, audit logs, Prometheus metrics, CI with a line-coverage gate ≥ 90%, a 22-combination feature matrix, and the E2E quality suite. Before going live, work through [🔒 Security Docs · Production Hardening Checklist](SECURITY.md).

### ❓ What license does the project use?

[Apache License 2.0](../../LICENSE). Copyright © 2025-2026 Kirky.X🌠.

### ❓ Where can I get help?

- Usage questions: the [📖 User Guide](USER_GUIDE.md) and this FAQ;
- API details: the [📘 API Reference](API_REFERENCE.md);
- Bugs and suggestions: [GitHub Issues](https://github.com/Kirky-X/crawlrs/issues);
- Security vulnerabilities: prefer GitHub [Security Advisories](https://github.com/Kirky-X/crawlrs/security/policy) private disclosure, with email fallback [Kirky-X@outlook.com](mailto:Kirky-X@outlook.com) (contact Kirky.X; do not disclose publicly).

---

## 📦 Installation and Configuration

### ❓ What are the system requirements?

Rust 1.97.1+ (`Cargo.toml` `rust-version`; building needs protoc), PostgreSQL 16+ (default backend), Docker 20+ (integration tests and container deployment). See the README "Quick Start".

### ❓ How do I choose a feature combination?

Pick a preset by scenario: default (full platform), `standard` (production recommended), `full` (all features), `--no-default-features` (single-tenant/embedding), `agent-lib` (minimal agent library surface). For the item-by-item matrix see the README "🎨 Feature Flags"; for how combinations are compile-verified see [🧪 Test Scenario Matrix](TEST_SCENARIOS.md).

### ❓ Which databases are supported?

Three feature groups — `db-postgres` (default) / `db-sqlite` / `db-mysql` — are selected at build time, exactly one; mutual exclusion is enforced at compile time by dbnexus. All three DDL sets ship with the repo (`migrations/postgres|sqlite|mysql/`); `crawlrs migrate` picks the matching subdirectory by the compiled-in features and applies it in order, so no schema needs to be provided at runtime.

### ❓ How are environment variables named?

A uniform `CRAWLRS__` prefix with `__` separating nesting levels (the confers convention), e.g. `CRAWLRS__DATABASE__URL`, `CRAWLRS__SERVER__PORT`. For the full reference see [.env.example](../../.env.example); for a config-section quick reference see the User Guide.

### ❓ Which port does the service listen on? How do I confirm it started?

Default `8899` (adjustable via `CRAWLRS__SERVER__PORT`). `curl http://localhost:8899/health` returning `{"status":"healthy","version":"0.2.0"}` means success.

---

## 🔐 Authentication and Multi-Tenancy

### ❓ What does an API Key look like?

Since 0.2.0, authentication is handled by garrison; the Bearer token format is `garrison_key_id.garrison_secret`. The plaintext key is returned only once in the issuance response — store it in a secrets manager immediately.

### ❓ Do legacy 0.1.x API Keys still work?

No. After the 0.2.0 authentication migration, the legacy `api_keys.key_hash` (SHA-256) system is fully invalidated; the `CRAWLRS__AUTH__KEYS` / `[auth] keys` config entries are deprecated, and keys must be re-issued through garrison.

### ❓ How do I issue a new API Key?

Two ways: `POST /v1/admin/api-keys` (requires the `crawlrs:admin` permission); in dev/test environments, the `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` environment variable auto-bootstraps an admin key (disabled in production).

### ❓ Which authentication-related items must be configured?

`CRAWLRS__AUTH__JWT_SECRET` (HS256, ≥32 bytes) — weak secrets refuse to start.

### ❓ What is the difference between 401 and 429?

401 means the key is invalid/expired/lacks permission (in auth-off builds it means the route has not been explicitly allowed via `allow_unauthenticated_protected()`); 429 means rate limiting or garrison brute-force lockout was triggered (5 failures / 60-second window / 300-second lockout).

### ❓ Can authentication be turned off entirely?

Yes, but with **deny by default**. `cargo build --release --no-default-features` builds a single-tenant/no-auth version: `default_identity_middleware` injects a fixed identity, all requests belong to the default team; when rate-limit / webhook are off, Noop implementations are likewise injected, invisible to business logic.

> ⚠️ auth-off is not "allow everything": `src/bootstrap/routes.rs:179-188` injects `ApiKeyScope::denied()` (read/write/admin all false, limits 0), so protected routes that have not explicitly called `crawlrs::presentation::middleware::auth_middleware::allow_unauthenticated_protected()` **always return 401**. To make a route anonymously accessible under auth-off, you must explicitly opt in in code; at startup a WARNING reminds you the instance runs in unauthenticated degraded mode — **never expose it to the public internet**.

---

## 🚂 Engines and Scraping

### ❓ When should I use each of the five engines?

| Engine | Applicable scenarios |
|------|----------|
| Reqwest | static HTML, API responses (the fastest default path) |
| Playwright (chromiumoxide) | JS-rendered SPAs, page interactions |
| FlareSolverr | anti-bot protected sites (Full/Cdp/Tls three modes, needs an external service) |
| WreqEngine | TLS fingerprint spoofing (BoringSSL JA3/JA4, `engine-tls-fingerprint`) |
| MllmEngine | vision-LLM autonomous navigation (screenshot → decide → act, `engine-mllm`) |

`EngineRouter` routes automatically with the `SmartHybrid` strategy; after an anti-bot detection hit, it can dynamically re-dispatch to a browser engine. Details in [🏗️ Architecture · Crawling Engines](ARCHITECTURE.md).

### ❓ What extra requirements does building the TLS fingerprint engine have?

`engine-tls-fingerprint` brings in wreq (BoringSSL backend); the build machine needs cmake and a C compiler. Licensing deliberately excludes the GPL wreq-util; fingerprint emulation is implemented by the project's own enums.

### ❓ Which output formats do scrape results support?

`ScrapeResponse` supports raw HTML, plain text, and structured extraction results; with `content` enabled a `markdown` field is attached (htmd conversion); content extraction can use Trafilatura (main path) / DomSmoothie (performance fallback) / LLM / RAG strategies.

### ❓ Does it comply with robots.txt?

Yes. `robotstxt` is a non-optional core-stack dependency; the crawl examples include a dedicated `robots_compliance` demo; deep crawling supports URL filter chains and adaptive stopping.

---

## 🧪 Testing and Deployment

### ❓ Why does running tests need Docker?

Integration tests use testcontainers to start a PostgreSQL 16 container automatically (without Docker the relevant cases auto-skip); you can also point `TEST_DATABASE_URL` at an external instance (`docker/docker-compose.test.yml` provides `test-db`). For the full commands see [🧪 Test Scenario Matrix · Runbook](TEST_SCENARIOS.md).

### ❓ How large is the test suite?

As of 0.2.0 (methodology: inline test attributes counted via `grep -rE '^\s*#\[(tokio::)?test\]'`):

| Item | Count |
|---|---|
| `src/` | 476 `.rs` files, 229,992 lines, 6,548 inline test attributes |
| `tests/` | 124 `.rs` files, 994 test functions (of which `tests/unit/` has 71 files) |
| Criterion benchmark groups | 9 (`benches/benchmark.rs`) |
| Python test suites | 5 (`tests/python/`) |
| BDD acceptance | `tests/acceptance/` 61 scenarios / 10 feature files (2 `@external` ones do not run locally) |
| CI line-coverage gate | 90% |

See [🧪 Test Scenario Matrix · Statistics Summary](TEST_SCENARIOS.md#-statistics-summary).

### ❓ How do I deploy?

`docker build -t crawlrs:latest -f docker/Dockerfile .` builds the image; `docker compose -f docker/docker-compose.yml up -d` starts crawlrs + PostgreSQL + FlareSolverr + Chrome + Prometheus in one shot. For the Kubernetes multi-instance topology see [🏗️ Architecture · Deployment](ARCHITECTURE.md).

### ❓ What is `crawlrs worker`?

The second run mode of the same binary: API mode (default) receives requests and enqueues them; Worker mode consumes the task queue and executes scrapes. The two can be deployed separately and scaled independently.
