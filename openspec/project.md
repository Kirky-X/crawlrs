# Project Context

## Purpose

crawlrs is an enterprise-grade web scraping platform written in Rust, providing search, scraping, crawling, mapping, and structured extraction capabilities. It delivers 3-5x performance improvement compared to traditional Node.js solutions with P99 latency reduced by 50%.

### Core Features
- **Search**: Multi-engine concurrent aggregation (Google/Bing/Baidu/Sogou) with smart deduplication and A/B testing
- **Scrape**: Single-page content acquisition supporting Markdown/HTML/Screenshot/JSON formats
- **Crawl**: Full-site recursive crawling with depth control and path filtering
- **Extract**: LLM-based structured data extraction with CSS selector support

### Technical Highlights
- 10,000+ RPS per node, P99 latency < 200ms
- Smart engine routing (Fetch/Playwright/Fire Engine TLS/Fire Engine CDP)
- Circuit breaker protection for engine failures
- Two-layer rate limiting (API + team concurrency)
- Outbox pattern for reliable webhooks
- SSRF protection and robots.txt compliance

## Tech Stack

### Core Technologies
- **Language**: Rust 1.75+ (Edition 2021)
- **Web Framework**: Axum 0.8
- **ORM**: SeaORM 1.0
- **Async Runtime**: Tokio 1.48
- **Database**: PostgreSQL 15+, SQLite
- **Cache**: Redis 7+

### Key Dependencies
- **HTTP Client**: reqwest 0.12 (with rustls-tls, http2, charset)
- **Browser Automation**: chromiumoxide 0.8
- **Rate Limiting**: governor 0.10
- **Logging**: tracing 0.1, tracing-subscriber 0.3
- **Metrics**: metrics 0.24, metrics-exporter-prometheus 0.18
- **Serialization**: serde 1.0
- **Error Handling**: thiserror 2.0, anyhow 1.0
- **AWS SDK**: aws-sdk-s1 1.118, aws-config 1.8

### Build & Quality Tools
- **Formatter**: cargo fmt
- **Linter**: cargo clippy (strict warnings)
- **Security**: cargo-deny, cargo-audit
- **Testing**: cucumber, wiremock, axum-test, testcontainers
- **Benchmark**: criterion

## Project Conventions

### Code Style

**Formatting**
- 4 spaces indentation, 120 char line limit
- Run `cargo fmt` before committing

**Naming Conventions**
- Modules, functions, variables: `snake_case`
- Types, traits: `PascalCase`
- Constants, statics: `SCREAMING_SNAKE_CASE`

**Rust Best Practices**
- Prefer borrowing over cloning, minimize `.clone()`
- Use `Option<T>` over null values
- Use `Result<T, E>` for error handling
- Never panic in library code, use `?` operator
- Pre-allocate `Vec` capacity, use iterator chains
- Use `&str` over `String` for function parameters

**Documentation**
- Public APIs: Document with `///` comments including examples
- Complex logic: Explain "why", not just "what"

### Architecture Patterns

**Clean Architecture Layers**
```
src/
├── presentation/     (API Handlers, Middleware, Routes)
├── application/      (Use Cases, DTOs)
├── domain/           (Models, Services, Repositories)
├── infrastructure/   (Database, Cache, External Services)
├── engines/          (Scraping Engine Implementations)
├── workers/          (Background Task Processing)
├── queue/            (Task Queue Management)
└── utils/            (Utility Functions)
```

**Design Patterns Used**
- Strategy Pattern: `ScraperEngine` trait for pluggable engines
- Repository Pattern: Abstraction over data access
- Circuit Breaker: Protection against cascading failures
- Outbox Pattern: Reliable webhook delivery
- Load Balancing: Round-robin, weighted, fastest, smart hybrid
- Two-Layer Rate Limiting: API-level + Team-level

**Dependency Injection**
- Manual dependency injection in `main.rs`
- Traits defined in domain layer, implementations in infrastructure

### Testing Strategy

**Test Structure**
```
tests/
├── unit/              - Unit tests (isolated, no external dependencies)
├── integration/       - Integration tests (with test containers)
│   ├── helpers/      - Test utilities (test_app, test database)
│   ├── repositories/ - Repository tests
│   └── *.rs          - Feature-specific tests
├── e2e/              - End-to-end tests
└── handlers/         - Handler tests
```

**Testing Requirements**
- Maintain >80% test coverage on critical paths
- All PRs must pass unit and integration tests
- Use `cargo test` for running tests
- Use `RUST_LOG=debug cargo test -- --nocapture` for debug output

**Test Environment**
- PostgreSQL on port 5443 (database: `crawlrs_test`)
- Redis on port 6382
- Chrome headless on port 9222
- FlareSolverr on port 8191 (for Google search)

### Git Workflow

**Branch Strategy**
- `main` - Production branch, always deployable
- `develop` - Development integration branch
- `feature/<ticket-id>-<description>` - New features
- `bugfix/<ticket-id>-<description>` - Bug fixes
- `hotfix/<version>-<description>` - Production hotfixes

**Commit Conventions (Conventional Commits)**
```
<type>(<scope>): <subject>

<body>

<footer>
```

**Types**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation
- `style`: Code formatting (no functional changes)
- `refactor`: Restructuring (not new feature or fix)
- `perf`: Performance optimization
- `test`: Testing related
- `chore`: Build tools, dependencies
- `ci`: CI/CD configuration

**Commit Requirements**
- Atomic commits (one logical change per commit)
- Modular/file-level commits (never commit entire codebase)
- Detailed descriptions explaining "why"
- Subject ≤ 50 characters, body lines ≤ 72 characters
- Use present tense ("add" not "added")
- Associate with relevant issue/ticket numbers

**Example Commit**
```
feat(engines): add adaptive retry policy for Playwright engine

Implement exponential backoff with jitter for transient failures.
- Add configurable max retries and backoff multiplier
- Track success rate per engine for adaptive behavior

Closes #123
```

**Version Management**
- Semantic versioning: `MAJOR.MINOR.PATCH`
- Tag format: `v1.2.3`

## Domain Context

**Web Scraping Concepts**
- Engine selection based on URL characteristics (JS-heavy sites → Playwright)
- Circuit breaker states: Closed, Open, Half-Open
- Two-layer rate limiting: Global API rate + Team concurrency
- Task queue with priority scheduling
- Webhook delivery with exponential backoff

**Multi-Tenancy**
- Team-based resource isolation
- Credit/accounting system
- API key authentication

**Scraping Engines**
- `reqwest_engine`: Basic HTTP scraping
- `playwright_engine`: Headless Chrome for JavaScript-heavy sites
- `fire_engine_tls`: TLS fingerprint bypass
- `fire_engine_cdp`: CDP protocol for advanced browser control

## Important Constraints

**Performance Requirements**
- API throughput: 5,000+ RPS
- P50 latency: < 100ms
- P99 latency: < 500ms
- Task processing: 500+ tasks/min
- Success rate: > 99.5%

**Security Requirements**
- SSRF protection mandatory for all scraping requests
- No hardcoded secrets (use environment variables)
- Input validation using `validator` crate
- Regular security audits with `cargo audit`

**Operational Constraints**
- PostgreSQL 15+ required for migrations
- Redis 7+ for caching and rate limiting
- Chrome container required for Playwright tests
- Memory management for long-running workers

**Code Quality Constraints**
- `cargo clippy -- -D warnings` must pass
- All warnings treated as errors in CI
- Test coverage > 80% on critical paths

## External Dependencies

**Infrastructure**
- **PostgreSQL 15**: Primary database (port 5432)
- **Redis 7**: Caching and rate limiting (port 6379)
- **Chrome Headless**: Browser automation (port 9222)
- **FlareSolverr**: Bypass Cloudflare protection (port 8191)

**Optional Services**
- **AWS S3**: Storage backend for screenshots and files
- **LLM Providers**: OpenAI, Anthropic for structured extraction

**Search Engines**
- Google Custom Search API
- Bing Search API
- Baidu Search API
- Sogou Search API

**Configuration**
- Environment variables with prefix `CRAWLRS__` for overrides
- TOML configuration files in `config/` directory
- Local overrides in `config/local.toml` (not committed)
