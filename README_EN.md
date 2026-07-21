<div align="center">

![Logo](docs/image/logo.png)

### 🚀 Enterprise-grade Web Scraping Platform built with Rust

**High-Performance • Scalable • Type-Safe**

[![CI](https://img.shields.io/github/actions/workflow/status/Kirky-X/crawlrs/ci.yml?branch=main&label=build)](https://github.com/Kirky-X/crawlrs/actions/workflows/ci.yml) [![Version](https://img.shields.io/github/v/release/Kirky-X/crawlrs)](https://github.com/Kirky-X/crawlrs/releases) [![License](https://img.shields.io/github/license/Kirky-X/crawlrs)](https://github.com/Kirky-X/crawlrs/blob/main/LICENSE) ![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)

</div>

## 📖 Table of Contents

- [Overview](#overview)
- [Performance Benchmarks](#performance-benchmarks)
- [Key Features](#key-features)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Configuration](#configuration)
- [API Documentation](#api-documentation)
- [Architecture](#architecture)
- [Deployment](#deployment)
- [Testing](#testing)
- [Contributing](#contributing)
- [License](#license)
- [Support](#support)

---

## 📝 Overview <span id="overview"></span>

**crawlrs** is a high-performance, enterprise-grade web data collection platform for developers:

| Capability | Description |
|------------|-------------|
| 🔍 **Search** | Unified Google, Bing, Baidu, and Sogou search |
| 🎯 **Scrape** | Extract data from single web pages |
| 🕷️ **Crawl** | Automatically discover and crawl multiple pages |
| 📊 **Extract** | Parse and structure data from HTML |
| 🗺️ **Map** | Visualize and organize crawled data |

Built with Rust, crawlrs delivers exceptional performance:

| Metric | Improvement |
|--------|-------------|
| **Throughput** | 3-5x higher than Node.js |
| **P99 Latency** | 50% reduction |
| **Memory Usage** | 75% lower consumption |
| **CPU Usage** | 59% lower utilization |

---

## 📊 Performance Benchmarks <span id="performance-benchmarks"></span>

Compared to Node.js implementations:

| Metric | Node.js | Rust (crawlrs) | Improvement |
|--------|----------|----------------|-------------|
| Throughput | 1,200 req/s | 4,500 req/s | **3.75x** |
| P99 Latency | 450ms | 180ms | **60%** |
| Memory Usage | 512 MB | 128 MB | **75%** |
| CPU Usage | 85% | 35% | **59%** |

---

## ✨ Key Features <span id="key-features"></span>

### 🚀 High Performance

| Feature | Benefit |
|---------|---------|
| 3-5x throughput improvement | Faster data collection |
| 50% reduction in P99 latency | Real-time response times |
| Zero-cost abstractions | Rust's safety guarantees without overhead |
| Memory efficiency | 75% lower memory usage than Node.js |

### 🔍 Multi-Engine Support

| Engine | Use Case | Performance | Cost |
|--------|----------|------------|-------|
| **Reqwest** | Static HTML, API responses | ⚡ Fastest | 💰 Lowest |
| **chromiumoxide** | JavaScript-heavy SPAs, interactions | 🐢 Slower | 💳 Higher |
| **FlareSolverr** | Anti-bot protected sites (Full/Cdp/Tls modes) | 🚀 Variable | 💎 Variable |

### 🔎 Unified Search

| Capability | Description |
|------------|-------------|
| Multi-engine support | Google, Bing, Baidu, Sogou |
| A/B testing | Compare results across engines |
| Auto deduplication | Remove duplicate results |
| Result aggregation | Unified output format |

### 📊 Enterprise Features

| Feature | Description |
|---------|-------------|
| **Rate Limiting** | Per-team concurrency and RPM controls (limiteron-based, distributed rate limiting with circuit breaker) |
| **Caching** | oxcache multi-layer caching (L1 moka memory backend) with per-type TTL (search/dns/regex) |
| **Metrics & Monitoring** | Prometheus-compatible export |
| **Webhooks** | Event-driven task completion notifications |
| **API Key Authentication** | Scoped access control and team isolation |
| **Audit Logging** | Complete request tracking |
| **Proxy Support** | Unified outbound proxy configuration |
| **LLM Extraction** | genai-based LLM content extraction |

### 🏗️ Architecture

| Layer | Technology | Purpose |
|--------|------------|---------|
| Presentation | Axum | HTTP handlers, middleware |
| Application | Use Cases | Business logic orchestration |
| Domain | Traits | Core entities and services |
| Infrastructure | Postgres | External integrations |

---

## 📦 Installation <span id="installation"></span>

### Prerequisites

| Requirement | Minimum Version | Recommended |
|-------------|------------------|---------------|
| Rust | 1.70+ | Latest stable |
| PostgreSQL | 14+ | Latest stable |
| Docker | 20+ | Latest |

### Build from Source

```bash
# Clone repository
git clone https://github.com/YOUR_ORG/crawlrs.git
cd crawlrs

# Install with the `standard` preset (core stack + engine-playwright + metrics)
cargo build --release --features standard

# Install with all features (standard + engine-flaresolverr)
cargo build --release --features full

# Install with custom features
cargo build --release --features "engine-playwright,metrics"
```

### Feature Flags

> **Note:** `default = []` — no features are enabled by default. Use a preset (`standard` / `full`) or list features explicitly.

> **Core stack is non-optional.** Core dependencies (oxcache 0.3 / dbnexus 0.4 / confers 0.4 / limiteron 0.2 / sdforge 0.4 / inklog 0.1 / trait-kit 0.3 + scraper / chardetng / encoding_rs / robotstxt) and the HTTP fetching stack are always compiled; they are no longer exposed as features.

| Feature | Description | Default |
|---------|-------------|----------|
| `engine-playwright` | chromiumoxide-based browser automation | ❌ No |
| `engine-flaresolverr` | FlareSolverr anti-bot protection (FlareSolverrMode enum distinguishes Full/Cdp/Tls modes) | ❌ No |
| `metrics` | Prometheus metrics export | ❌ No |
| `genai-llm` | genai-based LLM extraction | ❌ No |
| `browser-download` | Auto-download Playwright browser | ❌ No |
| `test-mocks` | Test-only mock modules (requires explicit enable for integration tests) | ❌ No |
| `admin-tools` | Ops CLI tools (e.g. add_credits) | ❌ No |

> **Note:** `openapi` is not a Cargo feature — it is a cfg marker generated by `sdforge_macros`'s `#[forge]` macro for OpenAPI spec emission. Users do not need to enable it; sdforge always compiles and openapi auto-activates.

### Presets & Binary Size

The core stack (oxcache / dbnexus / confers / limiteron / sdforge / inklog / trait-kit + scraper / chardetng / encoding_rs / robotstxt + HTTP fetching stack) is always compiled and no longer exposed as features.

| Preset | Feature Set | Binary Size | Use Case |
|-----|---------|-----------|---------|
| standard | `engine-playwright, metrics` | ~35MB | JS rendering needed (core stack included by default) |
| full | `standard + engine-flaresolverr` | ~52MB | All features |

> **Note:** `default = []` is not listed in the preset table because it enables no optional features, compiling only the core stack (~30MB); intended for explicit opt-in scenarios.

### Custom Combinations

```bash
# Custom combination: core stack always compiled, only specify optional features
cargo build --release --features "engine-playwright,metrics,genai-llm"

# Core stack only (no optional features)
cargo build --release --no-default-features
```

### Feature Reference

| Feature | Description | Impact |
|------|------|------|
| `engine-playwright` | chromiumoxide JS rendering engine | +8MB |
| `engine-flaresolverr` | FlareSolverr engine (FlareSolverrMode enum for Full/Cdp/Tls modes) | - |
| `metrics` | Metrics monitoring | - |
| `genai-llm` | genai LLM extraction | - |
| `browser-download` | Auto-download Playwright browser | - |
| `test-mocks` | Test mock modules (`#[cfg(any(test, feature = "test-mocks"))]`) | - |
| `admin-tools` | Ops CLI tools (`cargo run --bin add_credits --features admin-tools`) | - |

---

## 🚀 Quick Start <span id="quick-start"></span>

Get up and running in under 5 minutes!

### 1️⃣ Configuration

Create a configuration file `config/default.toml`:

```toml
# config/default.toml
[database]
url = "postgresql://user:password@localhost/crawlrs"
max_connections = 20

[server]
host = "0.0.0.0"
port = 8899

[cors]
allowed_origins = "*"

[rate_limiting]
enabled = true
default_rpm = 60
default_limit = 60
burst_size = 20

[cache]
enabled = true

[cache.memory]
capacity = 10000
ttl_seconds = 300

[cache.types.search]
ttl_seconds = 300
max_size = 10000

[cache.types.dns]
ttl_seconds = 3600
max_size = 1000

[cache.types.regex]
ttl_seconds = 86400
max_size = 5000

[search]
default_engine = "baidu"
[search.engines]
google_enabled = true
bing_enabled = true
baidu_enabled = true
sogou_enabled = true
```

### 2️⃣ Database Setup

```bash
# Run migrations using built-in CLI
cargo run --bin crawlrs -- migrate

# Or with SQLx CLI
sqlx database create
sqlx migrate run
```

### 3️⃣ Run Server

```bash
# Development mode
cargo run --bin crawlrs

# Production mode
./target/release/crawlrs
```

### 4️⃣ Verify Installation

```bash
# Health check
curl http://localhost:8899/health

# Expected response:
# {"status":"healthy","version":"0.1.0"}
```

---

## ⚙️ Configuration <span id="configuration"></span>

crawlrs uses confers for configuration management, supporting TOML files and `CRAWLRS__`-prefixed environment variables (`__` for nesting). Default config file: `config/default.toml`.

### Environment Variables

| Variable | Description | Default | Required |
|-------------|----------|--------|------|
| `CRAWLRS__DATABASE__URL` | PostgreSQL connection string | - | Yes |
| `CRAWLRS__SERVER__HOST` | Server bind address | 0.0.0.0 | No |
| `CRAWLRS__SERVER__PORT` | Server port | 8899 | No |
| `CRAWLRS__CONCURRENCY__DEFAULT_TEAM_LIMIT` | Default per-team concurrency limit | 10 | No |
| `CRAWLRS__CACHE__MEMORY__CAPACITY` | Memory cache capacity | 10000 | No |
| `CRAWLRS__CACHE__MEMORY__TTL_SECONDS` | Memory cache TTL | 300 | No |
| `CRAWLRS__WEBHOOK__TIMEOUT_SECONDS` | Webhook call timeout | 10 | No |
| `CRAWLRS__WORKERS__COUNT` | Worker count ("auto" or number) | auto | No |
| `CRAWLRS__PROXY__URL` | Outbound proxy URL | - | No |
| `CRAWLRS__LLM__API_KEY` | LLM service API key | - | No |
| `CRAWLRS__ENGINES__FLARESOLVERR__URL` | FlareSolverr service URL | http://localhost:8191/v1 | No |
| `CRAWLRS__LOG_LEVEL` | Log level | info | No |
| `CRAWLRS__DATABASE__PASSWORD` | Database password (Docker mode) | - | No |

### Configuration Reference

| Section | Description | Key Fields |
|--------|------|---------|
| `[server]` | Server bind | `host`, `port`, `enable_port_detection` |
| `[cors]` | CORS cross-origin | `allowed_origins` (comma-separated, `*` wildcard) |
| `[database]` | Database connection | `url`, `max_connections`, `min_connections`, `connect_timeout` |
| `[rate_limiting]` | Rate limiting | `enabled`, `default_rpm`, `default_limit`, `burst_size` |
| `[cache]` | Cache control | `enabled`, `[cache.memory]` (capacity/ttl), `[cache.types.*]` (search/dns/regex) |
| `[concurrency]` | Concurrency control | `default_team_limit`, `task_lock_duration_seconds` |
| `[search]` | Search config | `default_engine`, `ab_test_enabled`, `timeout_seconds` |
| `[webhook]` | Webhook | `timeout_seconds`, `max_retries`, `secret`, `batch_size` |
| `[proxy]` | Outbound proxy | `url`, `enabled` |
| `[llm]` | LLM extraction | `api_key`, `model`, `api_base_url` |
| `[workers]` | Worker pool | `count` (`"auto"` or number) |
| `[engines.flaresolverr]` | FlareSolverr | `enabled`, `url`, `timeout_seconds` |
| `[logging]` | Log output | `[logging.console]`, `[logging.file]` (path/max_file_size/file_count) |
| `[trusted_proxies]` | Trusted proxies | `enabled`, `proxies` (CIDR list) |

---

## 📚 API Documentation <span id="api-documentation"></span>

> **Complete API Reference:** [API_REFERENCE.md](docs/API_REFERENCE.md) | **User Guide:** [USER_GUIDE.md](docs/USER_GUIDE.md)

### 🔑 Authentication

All protected endpoints require an API key in the `Authorization` header:

```bash
# Format
Authorization: Bearer YOUR_API_KEY

# Example curl
curl -H "Authorization: Bearer crawlrs_sk_abc123" \
  http://localhost:8899/v1/scrape
```

> **⚠️ Security Tip:** Never commit API keys to version control. Use environment variables.

### 📡 Public Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check (liveness probe) |
| `/metrics` | GET | Prometheus metrics |
| `/v1/version` | GET | Version number |

### 📡 Core Protected Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/scrape` | POST | Create a scrape task |
| `/v1/scrape/{id}` | GET | Get task details |
| `/v1/scrape/{id}/_cancel` | POST | Cancel scrape task |
| `/v1/crawl` | POST | Create a crawl task |
| `/v1/crawl/{id}` | GET | Get crawl status |
| `/v1/crawl/{id}` | DELETE | Cancel crawl task |
| `/v1/crawl/{id}/_cancel` | POST | Cancel crawl task |
| `/v1/crawl/{id}/results` | GET | Get crawl results |
| `/v1/search` | POST | Search with specified engine |
| `/v1/extract` | POST | Extract data from HTML |
| `/v1/webhooks` | POST | Create webhook |
| `/v1/webhooks` | GET | List webhooks |
| `/v1/teams/me` | GET | Get current team info |
| `/v1/teams/me/usage` | GET | Get team usage |
| `/v1/teams/geo-restrictions` | GET | Get team geo restrictions |
| `/v1/teams/geo-restrictions` | PUT | Update team geo restrictions |
| `/v1/tasks/_query` | POST | Complex query tasks |
| `/v1/tasks/_cancel` | POST | Batch cancel tasks |
| `/v1/audit/logs` | GET | Get audit logs |
| `/v1/audit/denied` | GET | Get denied requests |

### 📡 SDK Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/sdk/search` | POST | SDK search |
| `/api/v1/sdk/tasks` | POST | SDK create task |
| `/api/v1/sdk/scrape` | POST | SDK create scrape |
| `/api/v1/sdk/crawl` | POST | SDK create crawl |

---

## 🏗️ Architecture <span id="architecture"></span>

crawlrs follows Domain-Driven Design (DDD) principles with a clean four-layer architecture:

```mermaid
flowchart TB
    subgraph Presentation [Presentation Layer - Axum]
        A[HTTP Handlers]
        B[Middleware]
        C[Routes]
    end

    subgraph Application [Application Layer]
        D[Use Cases]
        E[DTOs]
        F[Request Validation]
    end

    subgraph Domain [Domain Layer]
        G[Models]
        H[Services]
        I[Repository Interfaces]
    end

    subgraph Infrastructure [Infrastructure Layer]
        J[Database]
        K[Cache]
        L[Storage]
        M[External APIs]
    end

    Presentation --> Application --> Domain --> Infrastructure
```

> **Detailed Architecture:** [ARCHITECTURE.md](docs/ARCHITECTURE.md)

### Engine Architecture

- **EngineClient**: The only public entry point, wrapping all scrape operations in a unified API
- **EngineRouter**: Engine dispatch core, using `Vec<Arc<dyn ScraperEngine>>` to store engine instances, selecting the best engine via configurable strategy
- **Routing Strategies**: Default `SmartHybrid` (intelligent hybrid), optional `RaceMode` (concurrent racing) / `SequentialFallback` (sequential fallback)

### Technology Stack

| Component | Technology | Version |
|-----------|------------|---------|
| Web Framework | Axum | 0.8 |
| Async Runtime | Tokio | 1.52 |
| Database ORM | Sea-ORM 2.0.0-rc.43 (via dbnexus 0.4) | - |
| Database | PostgreSQL | 14+ |
| Cache | oxcache (moka) | 0.3 |
| HTTP Client | Reqwest | 0.13 |
| Browser Automation | chromiumoxide | 0.9 |
| Structured Logging | inklog | 0.1 |
| API SDK | sdforge | 0.4 |
| Multi-backend Cache | oxcache | 0.3 |
| Rate Limiting | limiteron | 0.2 |
| Configuration | confers | 0.4 |
| DI Framework | trait-kit | 0.3 |
| HTML Parser | scraper | 0.27 |

---

## 🚢 Deployment <span id="deployment"></span>

### Docker Deployment

```bash
# Build Docker image
docker build -t crawlrs:latest .

# Run with Docker
docker run -d \
  -p 8899:8899 \
  -e CRAWLRS__DATABASE__URL="postgresql://user:pass@db:5432/crawlrs" \
  crawlrs:latest

# Run with Docker Compose
docker-compose up -d
```

### Production Checklist

- [ ] Set strong API keys and secrets
- [ ] Configure proper database connection pooling
- [ ] Configure oxcache caching for production (per-type TTL for search/dns/regex)
- [ ] Set appropriate rate limits (`default_limit` / `burst_size`)
- [ ] Configure CORS to specific origins (not `*` wildcard)
- [ ] Configure metrics export to Prometheus
- [ ] Enable distributed tracing (inklog HTTP sink)
- [ ] Set up log aggregation (ELK, CloudWatch, etc.)
- [ ] Configure webhook endpoints for task notifications
- [ ] Review and tune concurrency settings (`concurrency.default_team_limit`)
- [ ] Configure trusted proxies (`trusted_proxies`) to prevent IP spoofing
- [ ] Enable SSL/TLS termination
- [ ] Configure health check endpoints
- [ ] Set up backup and disaster recovery

---

## 🧪 Testing <span id="testing"></span>

```bash
# Run unit tests
cargo test --features default --lib --verbose

# Run integration tests (requires Docker: PostgreSQL + Redis via testcontainers)
cargo test --test integration_tests --features full

# Run SDK API tests
cargo test --features test-mocks --test sdk_api_test

# Run full main test entry
cargo test --features standard,test-mocks --test main

# Run coverage tests
cargo tarpaulin --out Html

# Run benchmarks
cargo bench

# Run clippy (linter)
cargo clippy --features default -- -D warnings

# Full clippy check (all features)
cargo clippy --features full -- -D warnings

# Format code
cargo fmt --all -- --check

# Dependency security check
cargo deny check

# Pre-commit full check
scripts/pre-commit-check.sh all
```

---

## 🤝 Contributing <span id="contributing"></span>

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Workflow

1. Fork repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'feat: add amazing feature'`)
4. Push to branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

### Code Style

- Follow Rust naming conventions
- Add doc comments to public APIs
- Write tests for new features
- Keep functions focused and small

---

## 📄 License <span id="license"></span>

This project is licensed under Apache License 2.0 - see [LICENSE](LICENSE) file for details.

```
Copyright 2025 Kirky.X

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

---

## 💬 Support <span id="support"></span>

| Resource | Link |
|----------|------|
| 📖 Documentation | [docs/](docs/) |
| 📚 API Reference | [API_REFERENCE.md](docs/API_REFERENCE.md) |
| 👤 User Guide | [USER_GUIDE.md](docs/USER_GUIDE.md) |
| 🏗️ Architecture | [ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| 🐛 Issue Tracker | [GitHub Issues](https://github.com/YOUR_ORG/crawlrs/issues) |
| 📧 Email | [Kirky-X@outlook.com](mailto:Kirky-X@outlook.com) |

---

## 🙏 Acknowledgments

- Built with [Rust](https://www.rust-lang.org/)
- Web framework powered by [Axum](https://github.com/tokio-rs/axum)
- Database ORM by [Sea-ORM](https://www.sea-ql.org/)
- Inspired by the need for high-performance web scraping solutions

---

<div align="center">

**Built with ❤️ in Rust**

[⬆ Back to Top](#overview)

</div>
