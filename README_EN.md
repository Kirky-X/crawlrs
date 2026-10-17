<div align="center">

<img src="docs/image/logo.png" alt="Crawlrs Logo" width="200">

[![CI](https://img.shields.io/github/actions/workflow/status/Kirky-X/crawlrs/ci.yml?branch=main&label=build)](https://github.com/Kirky-X/crawlrs/actions/workflows/ci.yml) [![Version](https://img.shields.io/github/v/release/Kirky-X/crawlrs)](https://github.com/Kirky-X/crawlrs/releases) [![License](https://img.shields.io/github/license/Kirky-X/crawlrs)](https://github.com/Kirky-X/crawlrs/blob/main/LICENSE) ![Rust](https://img.shields.io/badge/rust-1.97%2B-orange)

[**中文**](README.md) | English

**Enterprise-grade web scraping platform built with Rust**

[✨ Features](#-features) • [🚀 Quick Start](#-quick-start) • [📚 Documentation](#-documentation) • [💻 Examples](#-examples) • [🤝 Contributing](#-contributing)

</div>

---

<div align="center">

### 🎯 Submit a Task in One Line, Five Engines Relay to Deliver

Dispatch any task through one unified REST API — engine scheduling, anti-bot and rate limiting are handled by crawlrs:

<table style="width:100%; border-collapse: collapse">
<tr>
<td align="center" width="25%">📮<br><b>Task Intake</b><br><span style="color:#64748B">Sync · Async · Batch</span></td>
<td align="center" width="25%">🚂<br><b>Auto Routing</b><br><span style="color:#64748B">Hybrid · Race · Fallback</span></td>
<td align="center" width="25%">🛡️<br><b>Anti-Bot Escalation</b><br><span style="color:#64748B">Probe · Mask · Retry · Proxy</span></td>
<td align="center" width="25%">🧠<br><b>AI Augmentation</b><br><span style="color:#64748B">RAG · Graph · DRL</span></td>
</tr>
</table>

</div>

---

## 📋 Table of Contents

- [✨ Features](#-features)
- [🚀 Quick Start](#-quick-start)
- [🎨 Feature Flags](#-feature-flags)
- [🔌 As a Library Dependency](#-as-a-library-dependency)
- [📚 Documentation](#-documentation)
- [💻 Examples](#-examples)
- [🚢 Deployment](#-deployment)
- [🏗️ Architecture](#️-architecture)
- [🧪 Testing](#-testing)
- [📊 Performance](#-performance)
- [🔒 Security](#-security)
- [🗺️ Roadmap](#️-roadmap)
- [🤝 Contributing](#-contributing)
- [📋 Changelog](#-changelog)
- [📄 License](#-license)
- [🙏 Acknowledgments](#-acknowledgments)
- [📞 Contact & Support](#-contact--support)
- [⭐ Star History](#-star-history)

---

## ✨ Features

<table style="width:100%; border-collapse: collapse">
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🔍 <b>Unified Search</b><br><span style="color:#64748B">Aggregated results from Bing, Baidu and Sogou with RRF fusion ranking and SimHash cross-engine deduplication; Exa / Tavily register automatically when credentials are configured; an opt-in SearXNG public-instance pool can be enabled by configuration</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🎯 <b>Single-Page Scraping</b><br><span style="color:#64748B">Uniform scraping of static HTML and JS-rendered pages, with screenshots, form interactions, custom headers and sync-wait; the Playwright engine can capture background XHR/Fetch responses</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🕷️ <b>Deep Crawling</b><br><span style="color:#64748B">URL filter chains + composite scorers + priority queues + adaptive stop conditions, with robots.txt compliance (dual-clock Crawl-delay); streaming sitemap request source, soft-404 fingerprinting, resume-crawl seen warm-up</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">📊 <b>Data Extraction</b><br><span style="color:#64748B">Multiple extraction modes: CSS rules, content extractors (Trafilatura / DomSmoothie), LLM and RAG-augmented extraction; degradation-ladder quality gate, LLM long-text chunking, character-level extraction provenance</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🚂 <b>Five-Engine Smart Routing</b><br><span style="color:#64748B">Reqwest / Playwright / FlareSolverr / Wreq (TLS fingerprint) / MLLM (vision-LLM navigation + hybrid AX-tree snapshot), with SmartHybrid, race and sequential-fallback routing strategies</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🛡️ <b>Anti-Bot Countermeasures</b><br><span style="color:#64748B">Three-tier anti-bot detection, SPA shell probe &amp; upgrade, consistent UA pool masking, smart retry, proxy rotation, request coalescing; session identity trio with score-based retirement, AutoThrottle adaptive rate limiting, 429 exponential backoff</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🏢 <b>Enterprise Platform</b><br><span style="color:#64748B">Multi-tenant isolation, garrison authentication (RBAC + JWT + brute-force protection), limiteron rate limiting &amp; circuit breaking, webhook event notifications</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">📈 <b>Observability</b><br><span style="color:#64748B">Prometheus metrics export (queue depth, engine success rate, cache hits, etc.), inklog structured logging, audit logs, failure snapshots (signature-deduplicated archives)</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🧠 <b>Intelligent Enhancements</b><br><span style="color:#64748B">RAG-augmented extraction, knowledge-graph coverage-aware crawling, DRL adaptive policy (ONNX inference with heuristic fallback)</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🧩 <b>Feature Gating</b><br><span style="color:#64748B">20+ Cargo features combinable on demand; <code>agent-lib</code> provides a minimal embedded library surface; every business capability can be switched off with Noop implementations injected</span></td>
</tr>
</table>

Beyond the core capabilities above, crawlrs also provides site mapping, an async task queue (worker mode), multi-level caching and proxy support; for the design and code locations of the engine-level enhancement modules (anti-bot countermeasures, intelligent enhancements, etc.), see the [🏗️ Architecture Document](docs/ARCHITECTURE.md).

---

## 🚀 Quick Start

### 📦 Installation

```bash
git clone https://github.com/Kirky-X/crawlrs.git
cd crawlrs

# Default-features build (full platform business capabilities + db-postgres)
cargo build --release

# Recommended for production: default + Playwright engine + metrics + content processing
cargo build --release --features standard

# Everything except vision/MLLM (standard + FlareSolverr + content extraction + LLM boundary)
cargo build --release --features full
```

Requires Rust 1.97 or later (`rust-version` in `Cargo.toml`, latest stable is fine); the default database backend is PostgreSQL 16+ (`db-postgres`); Docker 20+ is used for integration tests (testcontainers) and containerized deployment.

### 💡 Minimal Example

The following example is adapted from the [`config/default.toml`](config/default.toml) template and the startup flow in the [📖 User Guide](docs/USER_GUIDE.md):

```bash
# 1. Prepare configuration (full template in config/default.toml, env vars in .env.example)
cat > config/default.toml <<'EOF'
[database]
url = "postgresql://user:password@localhost/crawlrs"

[server]
host = "0.0.0.0"
port = 8899
EOF

# 2. Initialize the database (the server does not auto-migrate; the dedicated subcommand applies migrations/*.sql in order)
cargo run --bin crawlrs migrate
# Migrations directory defaults to ./migrations; override with CRAWLRS_MIGRATIONS_DIR.
# Manual alternative: for f in migrations/*.sql; do psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q -f "$f"; done

# 3. Start the server (API mode; worker mode: cargo run --bin crawlrs worker)
cargo run --bin crawlrs
```

```bash
# 4. Verify the installation
curl http://localhost:8899/health
# {"status":"healthy","version":"0.2.0-rc.2"}
```

Calling the scrape endpoint (authentication and all endpoints: see the [📘 API Reference](docs/API_REFERENCE.md)):

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer <garrison_key_id>.<garrison_secret>" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com"}'
```

### 🧭 Core Concepts

- **Engines & routing**: `EngineClient` is the single public entry point for scraping; internally `EngineRouter` holds `Vec<Arc<dyn ScraperEngine>>` and picks an engine via the `SmartHybrid` (default) / `RaceMode` / `SequentialFallback` strategies.
- **Four run modes**: one binary, dispatched by subcommand (`src/main.rs`) — `crawlrs` or `crawlrs api` (default) starts the API server, `crawlrs worker` consumes the task queue in worker mode, `crawlrs bootstrap` initializes the default team and an admin API key, and `crawlrs migrate` applies `migrations/*.sql` in order.
- **DDD four layers**: `presentation → application → domain → infrastructure`, wired by trait-kit's capability Modules (`SettingsModule`/`CacheModule`/`EngineModule` etc.) for DI.
- **Configuration**: powered by confers — TOML files plus `CRAWLRS__`-prefixed environment variables (`__` separates nesting, e.g. `CRAWLRS__DATABASE__URL`).
- **SearXNG meta search (opt-in)**: `search.searxng.enabled = false` by default. When enabled, public instances are collected from searx.space into the `searxng_instances` registry, periodically probed to maintain an in-process pool (target: 3 usable instances), and failed requests fail over to another instance up to `max_failover_attempts` times.

  ```toml
  [search.searxng]
  enabled = true                                # spawns collector + health workers
  pool_target = 3
  seed_urls = ["https://searx.example.org"]     # fallback when the dataset is unreachable
  ```

  **Privacy disclosure**: once enabled, user queries are sent in plaintext to third-party public SearXNG instances that this deployment does not control. Being disabled by default is the mitigation; governance views are `GET /v1/searxng/instances` and `GET /v1/searxng/pool`.
- **Feature gating**: when business capabilities (teams/auth/rate-limit/webhook) are off, Noop implementations are injected automatically — single-tenant/unauthenticated deployments need zero business-logic changes.

---

## 🎨 Feature Flags

### 📦 Presets

| Preset | Build command | Contents | Use case |
|------|----------|----------|----------|
| Default | `cargo build --release` | `platform` (teams + auth + rate-limit + webhook + metrics + content + sitemap) + `db-postgres` | Full platform out of the box |
| `standard` | `cargo build --release --features standard` | Default + `engine-playwright` + `sitemap` | Recommended for production (JS rendering + metrics + content processing + sitemap) |
| `full` | `cargo build --release --features full` | `standard` + `engine-flaresolverr` + `extractors` + `llm` | Everything except vision/MLLM (add `engine-mllm`) |
| `no-default` | `cargo build --release --no-default-features` | Pure core scraping stack | Single-tenant/unauthenticated deployments, embedding |
| `agent-lib` | `cargo build --release --no-default-features --features agent-lib` | `content` + `trafilatura` + `dom-smoothie` | Minimal agent/embedded library surface |

### 📋 Feature Matrix

The table below is a major subset of the `[features]` section of `Cargo.toml` (full list: see `Cargo.toml`), with `default = ["platform", "db-postgres"]`; the core scraping stack (oxcache / dbnexus / confers / sdforge / inklog / trait-kit + scraper / reqwest / robotstxt) is always compiled.

| Feature | Description | Default |
|------|------|------|
| `teams` | Multi-tenant isolation (implies `auth`); when off, all requests belong to the single-tenant `DEFAULT_TEAM_ID` | ✅ |
| `auth` | garrison v0.9 authentication (RBAC + JWT + brute-force protection); when off, a fixed identity is injected | ✅ |
| `rate-limit` | limiteron rate limiting & circuit breaking; when off, `NoopRateLimitingService` is injected | ✅ |
| `webhook` | Webhook delivery; when off, `/v1/webhooks` routes are removed and a Noop is injected | ✅ |
| `metrics` | Prometheus metrics export (incl. sysinfo memory awareness) | ✅ (platform) |
| `content` | Anti-bot detection (aho-corasick) + HTML→Markdown (htmd) | ✅ (platform) |
| `db-postgres` / `db-sqlite` / `db-mysql` | Database driver, pick exactly one (compile-time exclusivity enforced by dbnexus) | ✅ postgres |
| `engine-playwright` | chromiumoxide browser automation engine | ❌ |
| `engine-flaresolverr` | FlareSolverr anti-bot engine (Full/Cdp/Tls modes) | ❌ |
| `engine-tls-fingerprint` | WreqEngine TLS fingerprint masking (BoringSSL JA3/JA4) | ❌ |
| `engine-mllm` | MLLM vision-LLM autonomous navigation engine (implies `engine-playwright` + `llm`; keeps `genai` for the vision surface) | ❌ |
| `trafilatura` / `dom-smoothie` / `extractors` | Content extraction main path / performance fallback / all enabled | ❌ |
| `llm` | LLM extraction boundary (trait + orchestration; provider access injected by the host carrier) | ❌ |
| `sitemap` | Sitemap generation (`/v1/map` endpoint) | ✅ (platform/standard) |
| `rag-local` | Local RAG embedding and reranking (vecboost) | ❌ |
| `rag-remote` | Remote RAG embedding (rig-core) | ❌ |
| `rag` | RAG bundle (`rag-local` + `rag-remote`) | ❌ |
| `openapi` | OpenAPI 3.1 spec generation (`GET /openapi.json`) | ❌ |
| `test-mocks` | Test-only mock gating (integration tests must enable explicitly) | ❌ |

> For the feature-combination compile matrix (22 combinations) and how it is verified, see the [🧪 Test Scenario Matrix](docs/TEST_SCENARIOS.md); for the gating implementation pattern, see [🏗️ Architecture Document · Feature Gate Architecture](docs/ARCHITECTURE.md).

---

## 🔌 As a Library Dependency

crawlrs can be embedded by downstream projects (e.g. gateway applications) as a library via a **path dependency**, gaining scraping engines / content extraction / RAG capabilities. The supported embedding path is described below.

### Embedding feature combination

When pulling crawlrs in via a path dependency, declare the feature combination below (the default engine set comes along with `default-features`, bringing in `platform` + `db-postgres`):

```toml
[dependencies]
crawlrs = { path = "../crawlrs", features = ["db-postgres", "extractors", "llm"] }
```

| feature | Description | In default |
|---------|-------------|:----------:|
| `db-postgres` | PostgreSQL backend (primary embedding storage path) | ✅ already in default |
| `extractors` | Content extraction (`trafilatura` + `dom-smoothie`) | Explicit |
| `llm` | LLM structured extraction boundary (trait + orchestration, no provider access; carrier injected by the host) | Explicit |

> RAG (`vecboost`, via `rag-local`/`rag-remote`) and the TLS-fingerprint engine (`wreq`, via `engine-tls-fingerprint`) are **not** part of the supported default embedding combination—they pull extra local sibling path dependencies or the BoringSSL build chain and must be validated per their own features.

### LLM carrier injection model

crawlrs no longer bundles any LLM provider access (the former `LLMService` / `provider_adapter` / `llm_provider_strategy` are deleted); `LLMServiceTrait` is the only LLM boundary. The host injects its own implementation (e.g. fluxgate's `FluxgateLLMCarrier`) through any of these public assembly paths:

1. assign the `Services.llm_service` public field directly;
2. replace `init_llm_service` with a host factory;
3. inject through the `CrawlRsState::from_kit` assembly surface.

Without injection the default assembly is `ExplicitErrorCarrier`: every LLM call returns an i18n bilingual structured error (`domain-error-llm-carrier-not-injected`) — no panic, no silent degradation. The `prompt_builder` / `chunker` (F8) / `provenance` (F9) orchestration modules remain available for injected carriers to reuse.

### Unified dependency version baseline

The embedding side and crawlrs share the same nexus-family dependencies; cargo unifies an `x.y.z-rc.N` requirement to the highest rc within the same minor. crawlrs has raised its manifest minimum versions to the baseline below, making "single-repo CI baseline = embedding compile target" and preventing "green in one repo, red when embedded" from recurring on upstream upgrades:

| Dependency | Unified baseline | Embedding role |
|-----------|-----------------|---------------|
| `dbnexus` | `0.6.0-rc.9` | Database abstraction (over sea-orm) |
| `inklog` | `0.3.0-rc.9` | Logging (`log::` facade backend) |
| `limiteron` | `0.3.0-rc.7` | Rate limiting / circuit breaking / backoff |
| `confers` | `0.6.0-rc.10` | Configuration loading |
| `trait-kit` | `0.5.0-rc.9` | Async DI (AsyncKit) |
| `oxcache` | `0.5.0-rc.8` | Multi-tier caching |
| `garrison` | `0.9.0-rc.3` | Authentication engine (pulled via default's `auth`) |

> The embedding side must not rely on `[patch.crates-io]` to force version alignment—patch is a local development-state mechanism and is disallowed on the published surface (a path version coexisting with a registry version causes trait identity splitting). Raising versions only guarantees "equivalent compile and test pass under the same version" and does not change runtime behavior.

### Embedding probe gate

`scripts/embed-probe.sh` reproduces the embedding compile path via a path dependency in a temporary directory (`cargo new` a probe crate → `cargo add --path <repo-root> --features "db-postgres,extractors,llm"` → `cargo check`), so embedding breakage is caught inside the repo; the CI `embed-probe` job runs this script serially and blocks merging on failure.

```bash
scripts/embed-probe.sh              # auto-locate repo root (derived from the script's own path)
scripts/embed-probe.sh <repo-root>  # specify repo root explicitly (cross-machine / CI)
```

Exit code `0` means the embedding compile surface is healthy; a non-zero exit prints the failing step name to stderr (`root` / `new` / `add` / `check`). Probe build artifacts land in the `mktemp` temporary directory, never polluting the repo working tree, and it is safe to run repeatedly.

---

## 📚 Documentation

| Document | Description |
|------|------|
| [📖 User Guide](docs/USER_GUIDE.md) | Complete tutorial covering auth, scraping, crawling, search, extraction, webhooks, teams and more |
| [📘 API Reference](docs/API_REFERENCE.md) | All REST endpoints, request/response formats, error codes and the SDK |
| [🏗️ Architecture](docs/ARCHITECTURE.md) | DDD layers, engine routing, enhancement modules, security model and deployment topologies |
| [⚡ Performance Guide](docs/PERFORMANCE.md) | Benchmark suite, performance instrumentation and tuning advice |
| [🔒 Security](docs/SECURITY.md) | Security design, supply-chain gates, vulnerability reporting and production hardening checklist |
| [❓ FAQ](docs/FAQ.md) | Frequently asked questions |
| [🧪 Test Scenario Matrix](docs/TEST_SCENARIOS.md) | Exhaustive test suite inventory, feature-combination matrix and runbook |
| [🤝 Contributing](docs/CONTRIBUTING.md) | Environment setup, development workflow and commit conventions |
| [📋 Changelog](docs/CHANGELOG.md) | Release-by-release change log |
| [📦 Releases](https://github.com/Kirky-X/crawlrs/releases) | Release page |

---

## 💻 Examples

All 66 runnable examples live in [`examples/`](examples/) (a member crate of the root workspace, `members = ["examples"]` in the root `Cargo.toml`), grouped by domain: search, scrape, crawl, extract, auth, teams, webhooks, cache, config, database, proxy, rate-limiting, sdk, tasks, browser, advanced, etc. (each example is a `[[bin]]` target such as `basic_scrape`). For the categorized list see [examples/README.md](examples/README.md) and [examples/QUICKSTART.md](examples/QUICKSTART.md).

```bash
cd examples
cargo run --bin basic_scrape          # Basic scraping
cargo run --bin api_key_auth          # garrison API Key authentication
cargo run --bin async_batch           # Async batch scraping
cargo build                           # Build all examples
```

---

## 🚢 Deployment

```bash
# Build the image (primary Dockerfile lives in docker/)
docker build -t crawlrs:latest -f docker/Dockerfile .

# One-command startup with Docker Compose (crawlrs + PostgreSQL 16 + FlareSolverr + Chrome + Prometheus)
docker compose -f docker/docker-compose.yml up -d
```

The server listens on `8899` by default (configurable via `CRAWLRS__SERVER__PORT`); for single-instance and Kubernetes multi-instance topologies, the worker pool and external dependencies see the [🏗️ Architecture Document · Deployment Architecture](docs/ARCHITECTURE.md), and for the production hardening checklist see [🔒 Security Document · Production Hardening](docs/SECURITY.md).

---

## 🏗️ Architecture

crawlrs follows Domain-Driven Design with a four-layer architecture — `presentation → application → domain → infrastructure`: Axum handles HTTP and middleware, use cases orchestrate business logic, the domain layer defines capability contracts via traits such as `ScraperEngine`, and the infrastructure layer reaches PostgreSQL through dbnexus (Sea-ORM) and multi-level caching through oxcache. The scraping data path is: request passes pre-check SSRF validation → `EngineRouter` selects an engine by strategy (anti-bot detection can dynamically re-dispatch to a browser engine) → worker pool executes → results are persisted and webhooks fire.

The consumption surface of the in-house library matrix (same-author shared rc chain): garrison carries authentication and health probes (gated by `auth`); limiteron carries rate limiting and the engine circuit-breaker state machine (a thin `ManualCircuitBreaker` wrapper plus an `AtomicU8` synchronous mirror); oxcache carries the cache itself (memory + `serialization` + `metrics`) and the URL-deduplication Bloom filter (`bloom` feature, replacing the in-house MmapBloom); sdforge carries DTO validation and HTTP error status-code mapping (`unified::http_status_for`). The in-house library gap register is documented in the [🏗️ Architecture Document · In-House Library Reverse Requirements](docs/ARCHITECTURE.md).

For layer responsibilities, engine routing details, all enhancement modules, queue/cache/rate-limit design and deployment topologies, see the [🏗️ Architecture Document](docs/ARCHITECTURE.md).

---

## 🧪 Testing

### 🎯 Test Strategy

The test system spans seven layers: inline unit tests in `src/`, module-organized unit tests in `tests/unit/`, the table-driven harness (`tests/main.rs`, mock injection), SDK API tests (`sdk_api_test`), real-PostgreSQL integration tests (`integration_tests`, incl. garrison auth end-to-end), Python API/performance tests, and the E2E quality suite (`tests/e2e/e2e-suite.sh`: 22-combination feature compile matrix → static checks → unit/integration tests → benchmarks → report). For the exhaustive scenario inventory and file mapping, see the [🧪 Test Scenario Matrix](docs/TEST_SCENARIOS.md).

### ▶️ Commands

**Identical to CI** (see `.github/workflows/ci.yml`):

```bash
# Unit tests (CI test job: one round each for standard / full; needs PostgreSQL 16,
# auto-provisioned locally via testcontainers)
cargo test --features "standard" --lib
cargo test --features "full" --lib

# Integration + all test targets (CI integration-test job)
cargo test --features "full,test-mocks" --tests --no-fail-fast

# Lint, format and doc gates (fmt / clippy / doc jobs)
cargo fmt --all -- --check
cargo clippy --features "standard" -- -D warnings
cargo clippy --features "full" -- -D warnings
cargo doc --features "full" --no-deps          # CI doc job (RUSTDOCFLAGS=-D warnings)

# Coverage gate: line coverage no lower than 80% (CI coverage job, uploaded to Codecov)
cargo llvm-cov --features "full" --fail-under-lines 80

# Supply-chain check (advisories / licenses / bans / sources)
cargo deny check
```

**Local extras** (not run by CI):

```bash
# Benchmarks (Criterion, 9 groups)
cargo bench

# E2E quality suite (feature matrix + static + tests + integration + bench + report)
./tests/e2e/e2e-suite.sh

# Python API / performance tests (local mode runs 3 suites: api_endpoints / performance / error_handling)
./scripts/run-tests.sh local

# Full pre-commit check (fmt → clippy → check → build → secret scan)
scripts/pre-commit-check.sh all
```

### 📊 Test Scale

As of 0.2.0-rc.2: 6,310 inline test attributes in `src/` (408 source files, 206,781 lines), 967 tests in `tests/` (116 `.rs` files, incl. 70 unit-test files under `tests/unit/` and the real-PostgreSQL integration suite), 9 Criterion benchmark groups, and 5 Python suites under `tests/python/` (`scripts/run-tests.sh local` mode runs 3 of them: api_endpoints / performance / error_handling); the coverage gate is line coverage ≥ 80%, enforced by CI. For itemized statistics see the [🧪 Test Scenario Matrix · Statistics Summary](docs/TEST_SCENARIOS.md).

---

## 📊 Performance

Performance engineering is built into the architecture: AIMD adaptive concurrency, memory-aware scheduling, TabPool reuse, request coalescing (singleflight), WaitFor conditional waiting and the hedge request-duplication controller. The reproducible benchmarks are the 9 Criterion groups in `benches/benchmark.rs` (task creation/status transitions, JSON serialization, URL parsing & validation, SSRF detection, RegexCache, engine routing, etc.), and E2E suite Stage 5 compares against `e2e-baseline` for regression detection. The legacy Node.js comparison figures were published without a measurement methodology and remain to be re-measured. For benchmark details and tuning advice see the [⚡ Performance Guide](docs/PERFORMANCE.md).

---

## 🔒 Security

### 🛡️ Security Design

Security covers the entire request path: SSRF is validated twice — pre-check in the handler and again in the engine router; authentication is delegated to garrison (HS256 JWT, RBAC, IP-level brute-force protection, persisted audit); webhooks are verified with Standard Webhooks signatures (subtle constant-time comparison); the JWT secret is zeroized to prevent memory residue; security response headers and trusted-proxy middleware are hardened by default. For each mechanism see the [🔒 Security Document](docs/SECURITY.md) and the [🏗️ Architecture Document · Security Model](docs/ARCHITECTURE.md).

### ⛓️ Supply Chain & Gates

CI consistently runs `cargo deny check` (advisories / licenses / bans / sources), CodeQL static analysis, clippy `-D warnings` and the RSA-code-path premise check; the local pre-commit script includes a private-key scan. For the full gate list see [🔒 Security Document · Supply Chain & Gates](docs/SECURITY.md).

### 🚨 Reporting a Vulnerability

Please do not report security vulnerabilities through public issues — email [Kirky-X@outlook.com](mailto:Kirky-X@outlook.com) instead. For the full handling process see [SECURITY.md](docs/SECURITY.md).

---

## 🗺️ Roadmap

<table style="width:100%; border-collapse: collapse">
<tr><th style="text-align:center">Status</th><th style="text-align:left">Area</th><th style="text-align:left">Items</th></tr>
<tr><td align="center">✅</td><td>Core platform (0.1.0)</td><td>Five-capability APIs (search/scrape/crawl/extract/map), DDD four layers, multi-tenancy & rate limiting, webhook notifications</td></tr>
<tr><td align="center">✅</td><td>Platform hardening & smart engines (0.2.0-rc.2)</td><td>garrison authentication, anti-bot detection, TLS-fingerprint & MLLM engines, RAG/KG/DRL intelligent enhancements, Prometheus observability, E2E quality suite</td></tr>
<tr><td align="center">🚧</td><td>Multi-driver databases</td><td><code>db-sqlite</code> / <code>db-mysql</code> covered at compile level; runtime schema provisioning pending (current <code>migrations/*.sql</code> is PG-only DDL)</td></tr>
<tr><td align="center">📋</td><td>Architecture evolution</td><td>Event-driven internal bus, WebSocket real-time task status, Redis shared cache layer (multi-instance deployments)</td></tr>
<tr><td align="center">📋</td><td>Performance stewardship</td><td>Routine Criterion baseline (e2e-baseline) regression comparisons, re-measurement of performance figures</td></tr>
</table>

---

## 🤝 Contributing

For the detailed contribution process and code conventions, see the [🤝 Contributing Guide](docs/CONTRIBUTING.md).

### 🛠️ Development Environment

See `rust-version` in `Cargo.toml` (1.97) for the toolchain requirement; protoc is needed to build sdforge. Before committing run `scripts/pre-commit-check.sh all` (fmt → clippy → check → build → secret scan) and follow Conventional Commits (`type(scope): subject`). For the TDD workflow, branching and testing requirements see [🤝 Contributing Guide · Development Workflow](docs/CONTRIBUTING.md).

### 💖 Ways to Contribute

<table style="width:100%; border-collapse: collapse">
<tr>
<td width="33%" align="center" style="padding: 16px">

### 🐛 Report a Bug

Found an issue?<br>
<a href="https://github.com/Kirky-X/crawlrs/issues/new">Open an Issue</a>

</td>
<td width="33%" align="center" style="padding: 16px">

### 💡 Suggest a Feature

Have an idea?<br>
<a href="https://github.com/Kirky-X/crawlrs/issues/new">Start a Discussion</a>

</td>
<td width="33%" align="center" style="padding: 16px">

### 🔧 Submit a PR

Want to contribute code?<br>
<a href="https://github.com/Kirky-X/crawlrs/pulls">Fork &amp; open a PR</a>

</td>
</tr>
</table>

---

## 📋 Changelog

For the full version history see the [📋 Changelog](docs/CHANGELOG.md) (following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), semantic versioning).

| Version | Date | Highlights |
|------|------|------|
| 0.2.0-rc.2 | 2026-09-21 | TLS fingerprint engine (WreqEngine), MLLM vision-navigation engine, RAG-augmented extraction, knowledge-graph coverage awareness, DRL adaptive policy, 5 new Prometheus metrics; feature absorption F1–F12 (AutoThrottle, session identity, failure snapshots, sitemap request source, XHR capture, extraction quality gate, LLM chunking & provenance, AIMD memory coupling, soft 404, AX-tree snapshot) |
| Unreleased | - | auth-off protected routes restore default 401, `web-axum` build surface restored, `crawlrs migrate` subcommand, production route for `POST /v1/scrape/{id}/_cancel`, ScrapeOptionsDto exposing `block_ads`/`block_media`/`needs_mllm`, forge-full staging feature, i18n/Fluent integration, etc. — full list in the [📋 Changelog](docs/CHANGELOG.md) |
| 0.2.0 | 2026-07-29 | garrison RBAC authentication integration, bootstrap admin key, `DELETE /v1/crawl/{id}`, brute-force protection hardening |
| 0.1.0 | 2026-07-22 | First public release: five-capability APIs, DDD four-layer architecture, multi-tenancy & rate limiting, unified search |

---

## 📄 License

This project is licensed under the [Apache License 2.0](LICENSE). Copyright © 2025-2026 Kirky.X🌠.

---

## 🙏 Acknowledgments

### 🌟 Core Dependencies

crawlrs stands on the shoulders of these excellent open-source projects:

| Dependency | Purpose |
|------|------|
| [tokio](https://crates.io/crates/tokio) | Async runtime |
| [axum](https://crates.io/crates/axum) | Web framework |
| [reqwest](https://crates.io/crates/reqwest) | HTTP client |
| [chromiumoxide](https://crates.io/crates/chromiumoxide) | Chrome CDP browser automation |
| [wreq](https://crates.io/crates/wreq) | BoringSSL TLS fingerprint masking |
| [scraper](https://crates.io/crates/scraper) | HTML parsing |
| [genai](https://crates.io/crates/genai) | Vision-model access (`engine-mllm` MLLM engine) |
| [htmd](https://crates.io/crates/htmd) | HTML→Markdown conversion |
| [rs-trafilatura](https://crates.io/crates/rs-trafilatura) / [dom_smoothie](https://crates.io/crates/dom_smoothie) | Content extraction |
| [criterion](https://crates.io/crates/criterion) | Benchmarking |
| [testcontainers](https://crates.io/crates/testcontainers) | Integration test infrastructure |

Same-author in-house foundation components: dbnexus (database abstraction), confers (configuration management), garrison (authentication framework), limiteron (rate limiting & circuit breaking), oxcache (caching), inklog (structured logging), sdforge (SDK generation), trait-kit (dependency injection).

### 💝 Special Thanks

Thanks to the Rust community and all [contributors](https://github.com/Kirky-X/crawlrs/graphs/contributors).

---

## 📞 Contact & Support

<table style="width:100%; max-width: 600px">
<tr>
<td align="center" width="33%">
<a href="https://github.com/Kirky-X/crawlrs/issues"><b style="color:#991B1B">Issues</b></a><br>
<span style="color:#64748B">Report problems and bugs</span>
</td>
<td align="center" width="33%">
<a href="mailto:Kirky-X@outlook.com"><b style="color:#1E40AF">Email</b></a><br>
<span style="color:#64748B">Kirky-X@outlook.com</span>
</td>
<td align="center" width="33%">
<a href="https://github.com/Kirky-X/crawlrs"><b style="color:#1E293B">GitHub</b></a><br>
<span style="color:#64748B">Browse the source</span>
</td>
</tr>
</table>

---

## ⭐ Star History

[![Star History Chart](https://api.star-history.com/svg?repos=Kirky-X/crawlrs&type=Date)](https://star-history.com/#Kirky-X/crawlrs&Date)

If this project helps you, please consider giving it a ⭐️!

**Built by Kirky.X**

---

<sub>© 2025 Kirky.X. All rights reserved.</sub>
