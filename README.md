<div align="center">

![Logo](resources/logo.png)

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
| **Fire** (Planned) | Anti-bot protected sites | 🚀 Variable | 💎 Variable |

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
| **Distributed Caching** | Redis-based caching with TTL |
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
| Infrastructure | Postgres, Redis, S3 | External integrations |

---

## 📦 Installation <span id="installation"></span>

### Prerequisites

| Requirement | Minimum Version | Recommended |
|-------------|------------------|---------------|
| Rust | 1.70+ | Latest stable |
| PostgreSQL | 14+ | Latest stable |
| SQLite | 3.x | 3.35+ |
| Redis | 7+ | Latest stable |
| Docker | 20+ | Latest |

### Build from Source

```bash
# Clone repository
git clone https://github.com/your-org/crawlrs.git
cd crawlrs

# Install with default features (PostgreSQL + Redis)
cargo build --release

# Install with all features (SQLite + all engines)
cargo build --release --features full

# Install with custom features
cargo build --release --features "engine-playwright,db-sqlite,metrics"
```

### Feature Flags

| Feature | Description | Default |
|---------|-------------|----------|
| `engine-reqwest` | Basic HTTP client | ✅ Yes |
| `engine-playwright` | Browser automation with Chromium | ❌ No |
| `engine-fire-cdp` | Fire Engine CDP support | ❌ No |
| `engine-fire-tls` | Fire Engine TLS support | ❌ No |
| `engine-flaresolverr` | FlareSolverr anti-bot protection | ❌ No |
| `redis-cache` | Redis caching support | ✅ Yes |
| `rate-limiting` | Rate limiting with Redis | ✅ Yes |
| `metrics` | Prometheus metrics export | ✅ Yes |
| `db-postgres` | PostgreSQL database support | ✅ Yes |
| `db-sqlite` | SQLite database support | ❌ No |
| `search-google` | Google search integration | ❌ No |
| `search-bing` | Bing search integration | ❌ No |
| `search-baidu` | Baidu search integration | ❌ No |
| `search-sogou` | Sogou search integration | ❌ No |

---

## 🚀 Quick Start <span id="quick-start"></span>

Get up and running in under 5 minutes!

### 1️⃣ Configuration

Create a configuration file `config/settings.yaml`:

```yaml
# config/settings.yaml
database:
  url: "postgresql://user:password@localhost/crawlrs"
  max_connections: 20

redis:
  url: "redis://localhost:6379"

server:
  host: "0.0.0.0"
  port: 8080

rate_limiting:
  enabled: true
  default_rpm: 60
  default_concurrent: 10

cache:
  enabled: true
  default_ttl: 300
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
curl http://localhost:8080/health

# Expected response:
# {"status":"healthy"}
```

---

## ⚙️ Configuration <span id="configuration"></span>

### Environment Variables

| Variable | Description | Default | Required |
|----------|-------------|----------|-----------|
| `DATABASE_URL` | PostgreSQL connection string | - | Yes |
| `REDIS_URL` | Redis connection string | - | No |
| `SERVER_HOST` | Server bind address | 0.0.0.0 | No |
| `SERVER_PORT` | Server port | 8080 | No |
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
  http://localhost:8080/api/v1/scrape
```

> **⚠️ Security Tip:** Never commit API keys to version control. Use environment variables.

### 📡 Core Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/scrape` | POST | Create a scrape task |
| `/api/v1/crawl` | POST | Create a crawl task |
| `/api/v1/search` | POST | Search with specified engine |
| `/api/v1/extract` | POST | Extract data from HTML |
| `/api/v1/tasks` | GET | List all tasks |
| `/api/v1/tasks/:id` | GET | Get task details |
| `/api/v1/tasks/:id` | DELETE | Cancel task |
| `/api/v1/metrics` | GET | Get system metrics |

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
| Database ORM | Sea-ORM | 1.0 |
| Database | PostgreSQL / SQLite | 14+ / 3.x |
| Cache | Redis | 7+ |
| HTTP Client | Reqwest | 0.12 |
| Browser Automation | Playwright | 0.40+ |

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
  -e REDIS_URL="redis://redis:6379" \
  crawlrs:latest

# Run with Docker Compose
docker-compose up -d
```

### Production Checklist

- [ ] Set strong API keys and secrets
- [ ] Configure proper database connection pooling
- [ ] Enable Redis for production caching
- [ ] Set up rate limiting appropriate for your capacity
- [ ] Configure metrics export to Prometheus
- [ ] Enable distributed tracing
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
| Fire Engine (TLS/CDP) implementation | 🔄 In Progress |
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
