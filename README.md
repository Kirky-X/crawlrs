English | [中文](README_CN.md)

<div align="center">

<img src="docs/image/crawlrs.png" alt="Crawlrs Logo" width="240">

[![Rust 2021](https://img.shields.io/badge/Rust-2021-edded?logo=rust&style=for-the-badge)](https://www.rust-lang.org/) [![Rustc 1.97.1+](https://img.shields.io/badge/Rustc-1.97.1+-orange.svg?style=for-the-badge)](https://www.rust-lang.org/) [![release](https://img.shields.io/github/v/release/Kirky-X/crawlrs?style=for-the-badge)](https://github.com/Kirky-X/crawlrs/releases) [![CI](https://img.shields.io/github/actions/workflow/status/Kirky-X/crawlrs/ci.yml?branch=main&label=CI&style=for-the-badge)](https://github.com/Kirky-X/crawlrs/actions/workflows/ci.yml) [![license](https://img.shields.io/github/license/Kirky-X/crawlrs?style=for-the-badge)](https://github.com/Kirky-X/crawlrs/blob/main/LICENSE)

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
<td width="50%" style="vertical-align:top; padding: 12px">🔍 <b>Unified Search</b><br><span style="color:#64748B">Aggregated results from Bing, Baidu and Sogou with RRF fusion ranking and SimHash cross-engine deduplication; Exa / Tavily register automatically when credentials are configured; an opt-in SearXNG public-instance pool can be enabled by configuration, with results in a unified format</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🎯 <b>Single-Page Scraping</b><br><span style="color:#64748B">Uniform scraping of static HTML and JS-rendered pages, with screenshots, form interactions, custom headers and sync-wait; the Playwright engine can capture background XHR/Fetch responses</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🕷️ <b>Deep Crawling</b><br><span style="color:#64748B">URL filter chains + composite scorers + priority queues + adaptive stop conditions, with robots.txt compliance (dual-clock Crawl-delay); streaming sitemap request source, soft-404 fingerprinting, resume-crawl seen warm-up</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">📊 <b>Data Extraction</b><br><span style="color:#64748B">Multiple extraction modes: CSS rules, content extractors (Trafilatura / DomSmoothie), LLM and RAG-augmented extraction; degradation-ladder quality gate, LLM long-text chunking, character-level extraction provenance, indirect prompt-injection sanitization (zero-width / hidden nodes / suspicious instructions, <code>[llm.sanitization]</code>)</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🚂 <b>Five-Engine Smart Routing</b><br><span style="color:#64748B">Reqwest / Playwright / FlareSolverr / Wreq (TLS fingerprint) / MLLM (vision-LLM navigation + hybrid AX-tree snapshot) / Moli (optional — the JS-rendering fallback for low-spec machines under memory-watermark scheduling), with SmartHybrid, race and sequential-fallback routing strategies</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🛡️ <b>Anti-Bot Countermeasures</b><br><span style="color:#64748B">Three-tier anti-bot detection, SPA shell probe &amp; upgrade, consistent UA pool masking, smart retry, proxy rotation, request coalescing; session identity trio with score-based retirement, AutoThrottle adaptive rate limiting, 429 exponential backoff</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🏢 <b>Enterprise Platform</b><br><span style="color:#64748B">Multi-tenant isolation (per-tenant proxy pool, browser &amp; DNS budgets), per-tenant compliance profiles (GDPR, ai.txt, PII minimization), garrison authentication (RBAC + JWT + brute-force protection), limiteron rate limiting &amp; circuit breaking, webhook event notifications</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">📈 <b>Observability</b><br><span style="color:#64748B">Prometheus metrics export (queue depth, engine success rate, cache hits, etc.), inklog structured logging, audit logs, failure snapshots (signature-deduplicated archives)</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🧠 <b>Intelligent Enhancements</b><br><span style="color:#64748B">RAG-augmented extraction, knowledge-graph coverage-aware crawling, DRL adaptive policy (ONNX inference with heuristic fallback)</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🧩 <b>Feature Gating</b><br><span style="color:#64748B">20+ Cargo features combinable on demand; <code>agent-lib</code> provides a minimal embedded library surface; every business capability can be switched off with Noop implementations injected</span></td>
</tr>
</table>

Beyond the core capabilities above, crawlrs also provides site mapping, an async task queue (worker mode), multi-level caching and proxy support; for the design and code locations of the engine-level enhancement modules (anti-bot countermeasures, intelligent enhancements, etc.), see the [🏗️ Architecture Document](docs/en/ARCHITECTURE.md).

---

## 🚀 Quick Start

### 📦 Installation

<details>
<summary>📄 Code · bash</summary>

```bash
git clone https://github.com/Kirky-X/crawlrs.git
cd crawlrs

# Default-features build (full platform business capabilities + db-postgres)
cargo build --release
```

</details>

Requires Rust 1.97.1 or later (`rust-version` in `Cargo.toml`, latest stable is fine); the default database backend is PostgreSQL 16+ (`db-postgres`); Docker 20+ is used for integration tests (testcontainers) and containerized deployment. `--features standard` is recommended for production and `--features full` for the all-feature combination — for the full build presets and feature semantics see [🎨 Feature Flags · User Guide](docs/en/USER_GUIDE.md#-feature-flags).

### 💡 Minimal Example

The following example is adapted from the [`config/default.toml`](config/default.toml) template and the startup flow in the [📖 User Guide](docs/en/USER_GUIDE.md):

<details>
<summary>📄 Code · bash</summary>

```bash
# 1. Prepare configuration (full template in config/default.toml, env vars in .env.example)
cat > config/default.toml <<'EOF'
[database]
url = "postgresql://user:password@localhost/crawlrs"

[server]
host = "0.0.0.0"
port = 8899
EOF

# 2. Initialize the database (the server does not auto-migrate; the migrate subcommand selects migrations/<dialect>/ by compiled database feature and applies it in order)
cargo run --bin crawlrs migrate
# Migrations root defaults to ./migrations; override with CRAWLRS_MIGRATIONS_DIR.
# Manual alternative for PostgreSQL: for f in migrations/postgres/*.sql; do psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q -f "$f"; done

# 3. Start the server (API mode; worker mode: cargo run --bin crawlrs worker)
cargo run --bin crawlrs
```

</details>

<details>
<summary>📄 Code · bash</summary>

```bash
# 4. Verify the installation
curl http://localhost:8899/health
# {"status":"healthy","version":"0.2.0"}
```

</details>

Calling the scrape endpoint (authentication and all endpoints: see the [📘 API Reference](docs/en/API_REFERENCE.md)):

<details>
<summary>📄 Code · bash</summary>

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer <garrison_key_id>.<garrison_secret>" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com"}'
```

</details>

### 🧭 Core Concepts

For the core concepts — engines & routing, the four run modes (api / worker / bootstrap / migrate), the DDD four layers, configuration management, SearXNG meta search (opt-in) and feature gating — see the [📖 User Guide](docs/en/USER_GUIDE.md) and the [🏗️ Architecture Document](docs/en/ARCHITECTURE.md).

---

## 📚 Documentation

| Document | Description |
|------|------|
| [📖 User Guide](docs/en/USER_GUIDE.md) | Complete tutorial covering auth, scraping, crawling, search, extraction, webhooks, teams and more |
| [📘 API Reference](docs/en/API_REFERENCE.md) | All REST endpoints, request/response formats, error codes and the SDK |
| [🏗️ Architecture](docs/en/ARCHITECTURE.md) | DDD layers, engine routing, enhancement modules, security model and deployment topologies |
| [⚡ Performance Guide](docs/en/PERFORMANCE.md) | Benchmark suite, performance instrumentation and tuning advice |
| [🔒 Security](docs/en/SECURITY.md) | Security design, supply-chain gates, vulnerability reporting and production hardening checklist |
| [❓ FAQ](docs/en/FAQ.md) | Frequently asked questions |
| [🧪 Test Scenario Matrix](docs/en/TEST_SCENARIOS.md) | Exhaustive test suite inventory, feature-combination matrix and runbook |
| [🤝 Contributing](docs/en/CONTRIBUTING.md) | Environment setup, development workflow and commit conventions |
| [📋 Changelog](docs/en/CHANGELOG.md) | Release-by-release change log |
| [📦 Releases](https://github.com/Kirky-X/crawlrs/releases) | Release page |

---

## 💻 Examples

All 66 runnable examples live in [`examples/`](examples/) (a member crate of the root workspace, `members = ["examples"]` in the root `Cargo.toml`), grouped by domain: search, scrape, crawl, extract, auth, teams, webhooks, cache, config, database, proxy, rate-limiting, sdk, tasks, browser, advanced, etc. (each example is a `[[bin]]` target such as `basic_scrape`). For the categorized list see [examples/README.md](examples/README.md) and [examples/QUICKSTART.md](examples/QUICKSTART.md).

<details>
<summary>📄 Code · bash</summary>

```bash
cd examples
cargo run --bin basic_scrape          # Basic scraping
cargo build                           # Build all examples
```

</details>

---

## 🚢 Deployment

<details>
<summary>📄 Code · bash</summary>

```bash
# One-command startup with Docker Compose (crawlrs + PostgreSQL 16 + FlareSolverr + Chrome + Prometheus)
docker compose -f docker/docker-compose.yml up -d
```

</details>

The server listens on `8899` by default (configurable via `CRAWLRS__SERVER__PORT`); for the image build command and containerized deployment see [🏗️ Architecture · Containerized Deployment Commands](docs/en/ARCHITECTURE.md#-containerized-deployment-commands), for single-instance and Kubernetes multi-instance topologies, the worker pool and external dependencies see the [🏗️ Architecture Document · Deployment Architecture](docs/en/ARCHITECTURE.md), and for the production hardening checklist see [🔒 Security Document · Production Hardening](docs/en/SECURITY.md).

---

## 🏗️ Architecture

crawlrs follows Domain-Driven Design with a four-layer architecture — `presentation → application → domain → infrastructure`. For layer responsibilities, engine routing details, all enhancement modules, queue/cache/rate-limit design and deployment topologies, see the [🏗️ Architecture Document](docs/en/ARCHITECTURE.md); for the consumption surface and gap register of the in-house library matrix, see [🏗️ Architecture · In-House Library Reverse Requirements](docs/en/ARCHITECTURE.md), and for embedding crawlrs as a library dependency see [🔌 Embedding as a Library Dependency](docs/en/ARCHITECTURE.md#-embedding-as-a-library-dependency).

---

## 🧪 Testing

### 🎯 Test Strategy

The test system spans seven layers: inline unit tests in `src/`, module-organized unit tests in `tests/unit/`, the table-driven harness (`tests/main.rs`, mock injection), SDK API tests (`sdk_api_test`), real-PostgreSQL integration tests (`integration_tests`, incl. garrison auth end-to-end), Python API/performance tests, and the E2E quality suite (`tests/e2e/e2e-suite.sh`: 22-combination feature compile matrix → static checks → unit/integration tests → benchmarks → report). For the exhaustive scenario inventory, the CI-identical commands and itemized statistics, see the [🧪 Test Scenario Matrix](docs/en/TEST_SCENARIOS.md); the coverage gate is line coverage ≥ 90%, enforced by CI (measured 90.14%).

---

## 📊 Performance

Performance engineering is built into the architecture: AIMD adaptive concurrency, memory-aware scheduling, TabPool reuse, request coalescing (singleflight), WaitFor conditional waiting and the hedge request-duplication controller; the reproducible benchmarks are the 9 Criterion groups in `benches/benchmark.rs`. For benchmark details and tuning advice see the [⚡ Performance Guide](docs/en/PERFORMANCE.md).

---

## 🔒 Security

### 🛡️ Security Design

Security covers the entire request path: SSRF is validated twice — pre-check in the handler and again in the engine router; authentication is delegated to garrison (HS256 JWT, RBAC, IP-level brute-force protection, persisted audit); webhooks are verified with Standard Webhooks signatures (subtle constant-time comparison); the JWT secret is zeroized to prevent memory residue; security response headers and trusted-proxy middleware are hardened by default. Anti-bot escalation hops emit structured `antibot_escalation` audit events; run `crawlrs doctor --security` for a startup self-check of feature flags, SSRF protection and runtime hardening. For each mechanism see the [🔒 Security Document](docs/en/SECURITY.md) and the [🏗️ Architecture Document · Security Model](docs/en/ARCHITECTURE.md).

### ⛓️ Supply Chain & Gates

CI consistently runs `cargo deny check` (advisories / licenses / bans / sources), CodeQL static analysis, clippy `-D warnings` and the RSA-code-path premise check; the local pre-commit script includes a private-key scan. For the full gate list see [🔒 Security Document · Supply Chain & Gates](docs/en/SECURITY.md).

### 🚨 Reporting a Vulnerability

Please do not report security vulnerabilities through public issues — use the GitHub [Security Advisories](https://github.com/Kirky-X/crawlrs/security/policy) private disclosure channel as the primary route (email [Kirky-X@outlook.com](mailto:Kirky-X@outlook.com), handler Kirky.X, only if that channel is unavailable). For the full handling process see [SECURITY.md](docs/en/SECURITY.md).

---

## 🗺️ Roadmap

For the version direction and in-flight work see [📋 Changelog · Development Roadmap](docs/en/CHANGELOG.md#️-development-roadmap).

---

## 🤝 Contributing

For the detailed contribution process and code conventions, see the [🤝 Contributing Guide](docs/en/CONTRIBUTING.md).

---

## 📋 Changelog

For the full version history see the [📋 Changelog](docs/en/CHANGELOG.md) (following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), semantic versioning).

---

## 📄 License

This project is licensed under the [Apache License 2.0](LICENSE). Copyright © 2025-2026 Kirky.X🌠.

---

## 🙏 Acknowledgments

crawlrs stands on the shoulders of many excellent open-source projects (for core dependencies and their purposes see `Cargo.toml` and the [🏗️ Architecture Document](docs/en/ARCHITECTURE.md)); same-author in-house foundation components: dbnexus (database abstraction), confers (configuration management), garrison (authentication framework), limiteron (rate limiting & circuit breaking), oxcache (caching), inklog (structured logging), sdforge (SDK generation), trait-kit (dependency injection). Thanks to the Rust community and all [contributors](https://github.com/Kirky-X/crawlrs/graphs/contributors).

---

## 📞 Contact & Support

Issues ([report problems and bugs](https://github.com/Kirky-X/crawlrs/issues)) · Email ([Kirky-X@outlook.com](mailto:Kirky-X@outlook.com)) · GitHub ([browse the source](https://github.com/Kirky-X/crawlrs))

---

## ⭐ Star History

[![Star History Chart](https://api.star-history.com/svg?repos=Kirky-X/crawlrs&type=Date)](https://star-history.com/#Kirky-X/crawlrs&Date)

If this project helps you, please consider giving it a ⭐️!

**Built by Kirky.X**

---

<sub>© 2025 Kirky.X. All rights reserved.</sub>
