<div align="center">

<img src="docs/image/logo.png" alt="Crawlrs Logo" width="200">

[![CI](https://img.shields.io/github/actions/workflow/status/Kirky-X/crawlrs/ci.yml?branch=main&label=build)](https://github.com/Kirky-X/crawlrs/actions/workflows/ci.yml) [![Version](https://img.shields.io/github/v/release/Kirky-X/crawlrs)](https://github.com/Kirky-X/crawlrs/releases) [![License](https://img.shields.io/github/license/Kirky-X/crawlrs)](https://github.com/Kirky-X/crawlrs/blob/main/LICENSE) ![Rust](https://img.shields.io/badge/rust-1.95%2B-orange)

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
<td width="50%" style="vertical-align:top; padding: 12px">🔍 <b>Unified Search</b><br><span style="color:#64748B">Aggregated results from Google, Bing, Baidu and Sogou with automatic deduplication, a unified output format and A/B testing support</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🎯 <b>Single-Page Scraping</b><br><span style="color:#64748B">Uniform scraping of static HTML and JS-rendered pages, with screenshots, form interactions, custom headers and sync-wait</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🕷️ <b>Deep Crawling</b><br><span style="color:#64748B">URL filter chains + composite scorers + priority queues + adaptive stop conditions, with robots.txt compliance</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">📊 <b>Data Extraction</b><br><span style="color:#64748B">Multiple extraction modes: CSS rules, content extractors (Trafilatura / DomSmoothie), LLM and RAG-augmented extraction</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🚂 <b>Five-Engine Smart Routing</b><br><span style="color:#64748B">Reqwest / Playwright / FlareSolverr / Wreq (TLS fingerprint) / MLLM (vision-LLM navigation), with SmartHybrid, race and sequential-fallback routing strategies</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🛡️ <b>Anti-Bot Countermeasures</b><br><span style="color:#64748B">Three-tier anti-bot detection, SPA shell probe &amp; upgrade, consistent UA pool masking, smart retry, proxy rotation, request coalescing</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🏢 <b>Enterprise Platform</b><br><span style="color:#64748B">Multi-tenant isolation, garrison authentication (RBAC + JWT + brute-force protection), limiteron rate limiting &amp; circuit breaking, webhook event notifications</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">📈 <b>Observability</b><br><span style="color:#64748B">Prometheus metrics export (queue depth, engine success rate, cache hits, etc.), inklog structured logging, audit logs</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🧠 <b>Intelligent Enhancements</b><br><span style="color:#64748B">RAG-augmented extraction, knowledge-graph coverage-aware crawling, DRL adaptive policy (ONNX inference with heuristic fallback)</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🧩 <b>Feature Gating</b><br><span style="color:#64748B">20+ Cargo features combinable on demand; <code>agent-lib</code> provides a minimal embedded library surface; every business capability can be switched off with Noop implementations injected</span></td>
</tr>
</table>

Beyond the core capabilities above, crawlrs also provides site mapping, an async task queue (worker mode), multi-level caching and proxy support; for the design and code locations of the engine-level enhancement modules (anti-bot countermeasures, intelligent enhancements, etc.), see the [🏗️ Architecture Document · Crawl Capability Enhancement Modules](docs/ARCHITECTURE.md).

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

# Everything (standard + FlareSolverr + content extraction + LLM)
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

[search]
default_engine = "baidu"
EOF

# 2. Initialize the database
cargo run --bin crawlrs -- migrate

# 3. Start the server (API mode; worker mode: cargo run --bin crawlrs worker)
cargo run --bin crawlrs
```

```bash
# 4. Verify the installation
curl http://localhost:8899/health
# {"status":"healthy","version":"0.2.0"}
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
- **Dual run modes**: one binary, two personalities — `crawlrs` starts the API server, `crawlrs worker` consumes the task queue in worker mode.
- **DDD four layers**: `presentation → application → domain → infrastructure`, wired by trait-kit's `AppModule` for DI.
- **Configuration**: powered by confers — TOML files plus `CRAWLRS__`-prefixed environment variables (`__` separates nesting, e.g. `CRAWLRS__DATABASE__URL`).
- **Feature gating**: when business capabilities (teams/auth/rate-limit/webhook) are off, Noop implementations are injected automatically — single-tenant/unauthenticated deployments need zero business-logic changes.

---

## 🎨 Feature Flags

### 📦 Presets

| Preset | Build command | Contents | Use case |
|------|----------|----------|----------|
| Default | `cargo build --release` | `platform` (teams + auth + rate-limit + webhook + metrics + content) + `db-postgres` | Full platform out of the box |
| `standard` | `cargo build --release --features standard` | Default + `engine-playwright` | Recommended for production (JS rendering + metrics + content processing) |
| `full` | `cargo build --release --features full` | `standard` + `engine-flaresolverr` + `extractors` + `llm` | Everything |
| `no-default` | `cargo build --release --no-default-features` | Pure core scraping stack | Single-tenant/unauthenticated deployments, embedding |
| `agent-lib` | `cargo build --release --no-default-features --features agent-lib` | `content` + `trafilatura` + `dom-smoothie` | Minimal agent/embedded library surface |

### 📋 Feature Matrix

The table below mirrors the `[features]` section of `Cargo.toml`, with `default = ["platform", "db-postgres"]`; the core scraping stack (oxcache / dbnexus / confers / sdforge / inklog / trait-kit + scraper / reqwest / robotstxt) is always compiled.

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
| `engine-mllm` | MLLM vision-LLM autonomous navigation engine (implies `engine-playwright` + `llm`) | ❌ |
| `trafilatura` / `dom-smoothie` / `extractors` | Content extraction main path / performance fallback / all enabled | ❌ |
| `llm` | genai LLM extraction | ❌ |
| `test-mocks` | Test-only mock gating (integration tests must enable explicitly) | ❌ |

> For the feature-combination compile matrix (22 combinations) and how it is verified, see the [🧪 Test Scenario Matrix](docs/TEST_SCENARIOS.md); for the gating implementation pattern, see [🏗️ Architecture Document · Feature Gate Architecture](docs/ARCHITECTURE.md).

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

All 65 runnable examples live in the standalone [`examples/`](examples/) workspace, grouped by domain: search, scrape, crawl, extract, auth, teams, webhooks, cache, config, database, proxy, rate-limiting, sdk, tasks, browser, advanced, etc. (each example is a `[[bin]]` target such as `basic_scrape`). For the categorized list see [examples/README.md](examples/README.md) and [examples/QUICKSTART.md](examples/QUICKSTART.md).

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

For layer responsibilities, engine routing details, all enhancement modules, queue/cache/rate-limit design and deployment topologies, see the [🏗️ Architecture Document](docs/ARCHITECTURE.md).

---

## 🧪 Testing

### 🎯 Test Strategy

The test system spans seven layers: inline unit tests in `src/`, module-organized unit tests in `tests/unit/`, the table-driven harness (`tests/main.rs`, mock injection), SDK API tests (`sdk_api_test`), real-PostgreSQL integration tests (`integration_tests`, incl. garrison auth end-to-end), Python API/performance tests, and the E2E quality suite (`tests/e2e/e2e-suite.sh`: 22-combination feature compile matrix → static checks → unit/integration tests → benchmarks → report). For the exhaustive scenario inventory and file mapping, see the [🧪 Test Scenario Matrix](docs/TEST_SCENARIOS.md).

### ▶️ Commands (identical to CI)

```bash
# Unit tests (CI test job: one round each for standard / full; needs PostgreSQL 16,
# auto-provisioned locally via testcontainers)
cargo test --features "standard" --lib
cargo test --features "full" --lib

# Integration + all test targets (CI integration-test job)
cargo test --features "full,test-mocks" --tests --no-fail-fast

# Lint and format gates
cargo fmt --all -- --check
cargo clippy --features "standard" -- -D warnings
cargo clippy --features "full" -- -D warnings

# Coverage gate: line coverage no lower than 80% (CI coverage job, uploaded to Codecov)
cargo llvm-cov --features "full" --fail-under-lines 80

# Supply-chain check (advisories / licenses / bans / sources)
cargo deny check

# Benchmarks (Criterion, 9 groups)
cargo bench

# E2E quality suite (feature matrix + static + tests + integration + bench + report)
./tests/e2e/e2e-suite.sh

# Python API / performance tests (local)
./scripts/run-tests.sh local

# Full pre-commit check (fmt → clippy → check → build → secret scan)
scripts/pre-commit-check.sh all
```

### 📊 Test Scale

As of 0.2.0: roughly 5,700+ inline tests in `src/` (372 source files), about 950 in `tests/` (incl. 71 unit-test files and the real-PostgreSQL integration suite), 9 Criterion benchmark groups and 5 Python suites; the coverage gate is line coverage ≥ 80%, enforced by CI. For itemized statistics see the [🧪 Test Scenario Matrix · Test Scale](docs/TEST_SCENARIOS.md).

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
<tr><td align="center">✅</td><td>Platform hardening & smart engines (0.2.0)</td><td>garrison authentication, anti-bot detection, TLS-fingerprint & MLLM engines, RAG/KG/DRL intelligent enhancements, Prometheus observability, E2E quality suite</td></tr>
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
| Unreleased | - | TLS fingerprint engine (WreqEngine), MLLM vision-navigation engine, RAG-augmented extraction, knowledge-graph coverage awareness, DRL adaptive policy, 5 new Prometheus metrics |
| 0.2.0 | 2026-07-29 | garrison RBAC authentication integration, bootstrap admin key, `DELETE /v1/crawl/{id}`, brute-force protection hardening |
| 0.1.0 | 2026-07-22 | First public release: five-capability APIs, DDD four-layer architecture, multi-tenancy & rate limiting, unified search |

---

## 📄 License

This project is licensed under the [Apache License 2.0](LICENSE). Copyright © 2025 Kirky.X.

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
| [genai](https://crates.io/crates/genai) | Multi-model LLM access |
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
