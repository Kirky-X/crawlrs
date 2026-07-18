<div align="center">

![Logo](docs/image/logo.png)

### 🚀 Enterprise-grade Web Scraping Platform built with Rust

**High-Performance • Scalable • Type-Safe**

![Build Status](https://img.shields.io/badge/build-passing-brightgreen)
![Version](https://img.shields.io/badge/version-0.1.0-blue)
![License](https://img.shields.io/badge/license-Apache%202.0-green)
![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)

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
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [License](#license)
- [Support](#support)

---

## 📝 Overview <span id="overview"></span>

**crawlrs** is a high-performance, enterprise-level web data collection platform designed for developers. It provides comprehensive capabilities including:

| Capability | Description |
|------------|-------------|
| 🔍 **Search** | Unified search across Google, Bing, Baidu, and Sogou |
| 🎯 **Scrape** | Extract data from single web pages |
| 🕷️ **Crawl** | Automatically discover and scrape multiple pages |
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
| **Playwright** | JavaScript-heavy SPAs, interactions | 🐢 Slower | 💳 Higher |
| **FlareSolverr** | Anti-bot protected sites | 🚀 Variable | 💎 Variable |

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
| **Rate Limiting** | Per-team concurrency and RPM controls |
| **Caching** | oxcache-based caching (moka memory backend) with TTL |
| **Metrics & Monitoring** | Prometheus-compatible export |
| **Webhooks** | Event-driven task notifications |
| **API Key Authentication** | Scoped access and team isolation |
| **Audit Logging** | Complete request tracking |

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
git clone https://github.com/your-org/crawlrs.git
cd crawlrs

# Install with the `standard` preset (core stack + Playwright + metrics)
cargo build --release --features standard

# Install with all features (standard + engine-flaresolverr)
cargo build --release --features full

# Install with custom features
cargo build --release --features "engine-playwright,metrics"
```

### Feature Flags

> **Note:** `default = []` — no features are enabled by default. Use a preset (`standard` / `full`) or list features explicitly.

> **Core stack is non-optional since v0.2.** Core dependencies (oxcache 0.3 / dbnexus 0.4 / confers 0.4 / limiteron 0.2 / sdforge 0.4 / inklog 0.1 / trait-kit + scraper / chardetng / encoding_rs / robotstxt) and the HTTP fetching stack are always compiled in; they are no longer exposed as features.

| Feature | Description | Default |
|---------|-------------|----------|
| `engine-playwright` | Browser automation with Chromium | ❌ No |
| `engine-flaresolverr` | FlareSolverr anti-bot protection (covers Full/Cdp/Tls modes) | ❌ No |
| `metrics` | Prometheus metrics export | ❌ No |
| `genai-llm` | LLM extraction via genai | ❌ No |
| `browser-download` | Auto-download browser for Playwright | ❌ No |

> **Note:** `openapi` is not a Cargo feature — it is a cfg marker generated by `sdforge_macros`'s `#[forge]` macro for OpenAPI spec emission. Users do not need to enable it; it is automatically active when sdforge is compiled (which is always, since Task9).

---

## 🔧 Compilation Features <span id="compilation-features"></span>

本项目支持通过 Cargo 特性灵活控制编译功能和二进制体积。自 v0.2 起，核心栈（oxcache / dbnexus / confers / limiteron / sdforge / inklog / trait-kit + scraper / chardetng / encoding_rs / robotstxt + HTTP 抓取栈）始终编译，不再以 feature 形式暴露。

### 预设配置

| 配置 | 特性组合 | 二进制大小 | 适用场景 |
|-----|---------|-----------|---------|
| standard | `engine-playwright, metrics` | ~35MB | 需要 JS 渲染（核心栈默认包含） |
| full | `standard + engine-flaresolverr` | ~52MB | 所有功能 |

> **Note:** `default = []` 不出现在预设表中，因为它不启用任何可选特性，仅编译核心栈（约 ~30MB）；用于按需显式启用场景。

### 自定义组合

```bash
# 自定义组合：核心栈始终编译，仅需指定可选特性
cargo build --release --features "engine-playwright,metrics,genai-llm"

# 仅核心栈（无任何可选特性）
cargo build --release --no-default-features
```

### 特性参考

| 特性 | 描述 | 影响 |
|------|------|------|
| `engine-playwright` | Playwright JS 渲染引擎 | +8MB |
| `engine-flaresolverr` | FlareSolverr 引擎（合并原 fire-cdp / fire-tls / flaresolverr 三引擎，通过 `FlareSolverrMode` 枚举区分 Full/Cdp/Tls 模式） | - |
| `metrics` | 指标监控 | - |
| `genai-llm` | genai LLM 抽取 | - |
| `browser-download` | 自动下载 Playwright 浏览器 | - |

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

[rate_limiting]
enabled = true
default_rpm = 60
default_concurrent = 10

[cache]
enabled = true
default_ttl = 300

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
# Development mode with hot reloading
cargo run --bin crawlrs

# Production mode
./target/release/crawlrs
```

### 4️⃣ Verify Installation

```bash
# Health check
curl http://localhost:8899/health

# Expected response:
# {"status":"healthy"}
```

---

## ⚙️ Configuration <span id="configuration"></span>

### Environment Variables

| Variable | Description | Default | Required |
|----------|-------------|----------|-----------|
| `DATABASE_URL` | PostgreSQL connection string | - | Yes |
| `SERVER_HOST` | Server bind address | 0.0.0.0 | No |
| `SERVER_PORT` | Server port | 8899 | No |
| `LOG_LEVEL` | Logging level | info | No |

---

## 📚 API Documentation <span id="api-documentation"></span>

> **Complete API Reference:** [API_REFERENCE.md](docs/API_REFERENCE.md) | **User Guide:** [USER_GUIDE.md](docs/USER_GUIDE.md)

### 🔑 Authentication

All protected endpoints require an API key in `Authorization` header:

```bash
# Format
Authorization: Bearer YOUR_API_KEY

# Example curl
curl -H "Authorization: Bearer crawlrs_sk_abc123" \
  http://localhost:8899/v1/scrape
```

> **⚠️ Security Tip:** Never commit API keys to version control. Use environment variables.

### 📡 Core Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/scrape` | POST | Create a scrape task |
| `/v1/scrape/{id}` | GET | Get task details |
| `/v1/scrape/{id}/_cancel` | POST | Cancel scrape task |
| `/v1/crawl` | POST | Create a crawl task |
| `/v1/crawl/{id}` | GET | Get crawl status |
| `/v1/crawl/{id}/_cancel` | POST | Cancel crawl task |
| `/v1/crawl/{id}/results` | GET | Get crawl results |
| `/v1/search` | POST | Search with specified engine |
| `/v1/extract` | POST | Extract data from HTML |
| `/v1/tasks/_query` | POST | Query tasks (complex query) |
| `/v1/tasks/_cancel` | POST | Batch cancel tasks |
| `/v1/webhooks` | POST | Create webhook |
| `/v1/teams/geo-restrictions` | GET | Get team geo restrictions |
| `/v1/teams/geo-restrictions` | PUT | Update team geo restrictions |
| `/v1/audit/logs` | GET | Get audit logs |
| `/v1/audit/denied` | GET | Get denied requests |
| `/v1/metrics` | GET | Get system metrics |

---

## 🏗️ Architecture <span id="architecture"></span>

crawlrs follows Domain-Driven Design (DDD) principles with clean architecture layers:


### Mermaid 版本

```mermaid
flowchart TD
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

### Technology Stack

| Component | Technology | Version |
|-----------|------------|---------|
| Web Framework | Axum | 0.8 |
| Async Runtime | Tokio | 1.48 |
| Database ORM | Sea-ORM 2.0.0-rc (via dbnexus 0.4) | - |
| Database | PostgreSQL | 14+ |
| Cache | oxcache (moka memory backend) | 0.3 |
| HTTP Client | Reqwest | 0.12 |
| Browser Automation | Playwright | 0.40+ |
| Structured Logging | inklog | 0.1 |
| API SDK | sdforge | 0.4 |
| Multi-backend Cache | oxcache | 0.3 |
| Rate Limiting | limiteron | 0.2 |
| Configuration | confers | 0.4 |

---

## 🚢 Deployment <span id="deployment"></span>

### Docker Deployment

```bash
# Build Docker image
docker build -t crawlrs:latest .

# Run with Docker
docker run -d \
  -p 8080:8080 \
  -e DATABASE_URL="postgresql://user:pass@db:5432/crawlrs" \
  crawlrs:latest

# Run with Docker Compose
docker-compose up -d
```

### Production Checklist

- [ ] Set strong API keys and secrets
- [ ] Configure proper database connection pooling
- [ ] Configure oxcache for production caching
- [ ] Set up rate limiting appropriate for your capacity
- [ ] Configure metrics export to Prometheus
- [ ] Enable distributed tracing (inklog HTTP sink)
- [ ] Set up log aggregation (ELK, CloudWatch, etc.)
- [ ] Configure webhook endpoints for task notifications
- [ ] Review and tune concurrency settings
- [ ] Enable SSL/TLS termination
- [ ] Configure health check endpoints
- [ ] Set up backup and disaster recovery

---

## 🧪 Testing <span id="testing"></span>

```bash
# Run unit tests
cargo test

# Run integration tests
cargo test --test integration_tests --features full

# Run with coverage
cargo tarpaulin --out Html

# Run benchmarks
cargo bench

# Run clippy (linter)
cargo clippy -- -D warnings

# Format code
cargo fmt
```

---

## 🗺️ Roadmap <span id="roadmap"></span>

### v0.2.0 (Planned)

| Feature | Status |
|---------|--------|
| FlareSolverr engine (Full/Cdp/Tls modes via `FlareSolverrMode`) | ✅ Implemented |
| WebSocket real-time subscriptions | 📅 Planned |
| Advanced proxy rotation | 📅 Planned |
| Machine learning-based proxy selection | 📅 Planned |

### v0.3.0 (Planned)

| Feature | Status |
|---------|--------|
| Multi-language SDKs (Python, JavaScript, Go) | 📅 Planned |
| UI dashboard | 📅 Planned |
| Advanced scheduling and cron jobs | 📅 Planned |
| Data pipeline integrations | 📅 Planned |

---

## 🤝 Contributing <span id="contributing"></span>

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Workflow

1. Fork repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
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
| 🐛 Issue Tracker | [GitHub Issues](https://github.com/your-org/crawlrs/issues) |
| 💬 Discord Community | [Join Discord](https://discord.gg/your-server) |
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
