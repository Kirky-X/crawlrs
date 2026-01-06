# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

crawlrs is an enterprise-grade web scraping platform written in Rust, providing search, scraping, crawling, and structured extraction capabilities. It's built with a clean architecture pattern and uses Axum for the web framework, SeaORM for database access, and Tokio for async runtime.

### Key Features

- **Search**: Multi-engine concurrent aggregation (Google/Bing/Baidu/Sogou) with smart deduplication and A/B testing
- **Scrape**: Single-page content acquisition supporting Markdown/HTML/Screenshot/JSON formats
- **Crawl**: Full-site recursive crawling with depth control and path filtering
- **Extract**: LLM-based structured data extraction with CSS selector support
- **Smart Engine Routing**: Automatically selects optimal scraping engine with circuit breaker protection
- **Two-Layer Rate Limiting**: API rate limiting + Team concurrency control
- **Reliable Webhooks**: Outbox pattern with exponential backoff retries

## Build Commands

### Development

```bash
# Development build
cargo build

# Release build
cargo build --release

# Run API server
cargo run -- api

# Run worker server
cargo run -- worker

# Run with custom config
cargo run -- --config config/default.toml

# Format code
cargo fmt

# Lint with strict warnings
cargo clippy -- -D warnings

# Run all tests
cargo test

# Run specific test module
cargo test integration_tests

# Run specific test function
cargo test test_search_aggregation

# Run ignored tests
cargo test -- --include-ignored

# Run benchmark
cargo bench

# Run tests with output
cargo test -- --nocapture

# Run tests with specific logging
RUST_LOG=debug cargo test
```

### Testing with Docker

```bash
# Start test environment (PostgreSQL, Redis, Chrome, FlareSolverr)
docker-compose -f docker/docker-compose.test.yml up -d

# Stop test environment
docker-compose -f docker/docker-compose.test.yml down

# View logs
docker-compose -f docker/docker-compose.test.yml logs -f crawlrs-api

# Check service health
docker-compose -f docker/docker-compose.test.yml ps
```

## Architecture

### Clean Architecture Layers

The project follows a strict clean architecture pattern with clear separation of concerns:

```
┌─────────────────────────────────────────┐
│         Presentation Layer              │
│     (API Handlers, Middleware)          │
└────────────────┬────────────────────────┘
                 │
┌────────────────▼────────────────────────┐
│         Application Layer               │
│       (Use Cases, DTOs)                 │
└────────────────┬────────────────────────┘
                 │
┌────────────────▼────────────────────────┐
│           Domain Layer                  │
│  (Models, Services, Repositories)       │
└────────────────┬────────────────────────┘
                 │
┌────────────────▼────────────────────────┐
│       Infrastructure Layer              │
│  (Database, Cache, External Services)   │
└─────────────────────────────────────────┘
```

### Directory Structure

- **src/presentation/** - API handlers, middleware, request extractors, routes
  - `handlers/` - HTTP request handlers (scrape, crawl, search, extract, webhook, metrics)
  - `middleware/` - Authentication middleware, rate limiters, team concurrency control
  - `routes/` - Route definitions and grouping

- **src/application/** - Business logic orchestration
  - `use_cases/` - Business logic implementations (scrape, crawl, extract)
  - `dto/` - Request/response data transfer objects

- **src/domain/** - Core business rules and entities
  - `models/` - Domain models (Task, ScrapeResult, Crawl, WebhookEvent)
  - `repositories/` - Repository trait definitions
  - `services/` - Domain services (rate limiting, team service)
  - `search/` - Search engine abstractions

- **src/infrastructure/** - External service implementations
  - `repositories/` - Repository implementations (PostgreSQL-based)
  - `search/` - Search engine implementations (aggregator, A/B test, smart search)
  - `cache/` - Redis caching
  - `database/` - Database connection pooling
  - `services/` - Infrastructure services (webhook, rate limit)
  - `storage/` - Storage abstraction (local/S3)

- **src/engines/** - Scraping engine implementations
  - `reqwest_engine.rs` - HTTP-based scraping engine
  - `playwright_engine.rs` - Headless Chrome browser automation
  - `fire_engine_tls.rs` - TLS fingerprint bypass
  - `fire_engine_cdp.rs` - CDP protocol engine
  - `router.rs` - Intelligent engine router with load balancing
  - `circuit_breaker.rs` - Circuit breaker for engine failure protection
  - `health_monitor.rs` - Engine health monitoring
  - `validators.rs` - URL validation and SSRF protection

- **src/workers/** - Background task processing
  - `manager.rs` - Worker lifecycle management
  - `scrape_worker.rs` - Async scrape task processing
  - `webhook_worker.rs` - Webhook delivery with retry logic
  - `backlog_worker.rs` - Backlog task processing
  - `expiration_worker.rs` - Expired task cleanup

- **src/queue/** - Task queue management
  - `task_queue.rs` - Task queue trait and PostgreSQL implementation
  - `scheduler.rs` - Task scheduling logic

- **src/utils/** - Utility functions
  - `robots.rs` - Robots.txt parsing and caching
  - `retry_policy.rs` - Exponential backoff retry strategies
  - `url_utils.rs` - URL validation and normalization
  - `text_processing.rs` - Text extraction and cleaning
  - `text_encoding.rs` - Character encoding detection
  - `port_sniffer.rs` - Port availability detection
  - `telemetry.rs` - Logging and metrics initialization

- **src/config/** - Configuration management
  - Settings structure with TOML parsing
  - Environment variable overrides
  - Security validation

### Key Design Patterns

1. **Strategy Pattern**: `ScraperEngine` trait allows pluggable scraping engines
2. **Repository Pattern**: Abstraction over data access (domain repositories vs infrastructure implementations)
3. **Circuit Breaker**: Protects against cascading failures in engines
4. **Outbox Pattern**: Ensures reliable webhook delivery
5. **Load Balancing**: Multiple strategies (round-robin, weighted, fastest, smart hybrid)
6. **Two-Layer Rate Limiting**: API-level (governor) + Team-level (semaphore)

### Data Flow

**Request Processing Flow:**
```
API Request → Middleware (Auth/Rate Limit/Concurrency) → Handler → Use Case → Engine Router → Engine → Response Storage → Database → (optional) Webhook Queue
```

**Async Task Flow:**
```
Task Created → Postgres Task Queue → Worker Polls → Engine Router → Scraping Engine → Result Storage → Webhook Delivery → Task Completion
```

**Search Flow:**
```
Search Request → Search Aggregator → Multiple Search Engines → Result Aggregation → Deduplication → Sorting → Response
```

## Configuration

Configuration is loaded from the `config/` directory with environment variable support:

- `default.toml` - Base configuration (committed to repo)
- `local.toml` - Local development overrides (not committed)

### Key Configuration Sections

- `[database]` - PostgreSQL connection settings (URL, pool size, timeouts)
- `[redis]` - Redis connection URL
- `[server]` - API server host, port, port detection
- `[rate_limiting]` - Rate limiting configuration (enabled/disabled, RPM)
- `[concurrency]` - Team concurrency limits, task lock duration
- `[search]` - Search engine selection, A/B testing settings
- `[webhook]` - Webhook timeout, retry settings, user agent
- `[storage]` - Storage backend (local filesystem or S3)
- `[engines]` - Engine-specific configurations

### Environment Variable Overrides

Configuration values can be overridden using environment variables with prefix `CRAWLRS__` and double underscores for nested keys.

Example:
```bash
export CRAWLRS__DATABASE__URL="postgres://user:pass@host:5432/db"
export CRAWLRS__REDIS__URL="redis://localhost:6379"
export CRAWLRS__SERVER__PORT=8899
```

## Database

Uses SeaORM with PostgreSQL and SQLite support. Migrations are defined in the `migration/` directory.

### Running Migrations

```bash
# Migrations run automatically on application startup
# To run manually:
cargo run --migration
```

### Key Tables

- `tasks` - Scraping task queue with priority and status
- `scrape_results` - Stored scrape results with metadata
- `crawls` - Crawl job tracking and status
- `webhooks` - Webhook configuration
- `webhook_events` - Outbox table for reliable webhook delivery
- `tasks_backlog` - Backlog queue for retry processing
- `credits` - Credit/accounting system
- `team_concurrency` - Team-level concurrency limits

## Testing

### Test Structure

```
tests/
├── unit/              - Unit tests (isolated, no external dependencies)
├── integration/       - Integration tests (with test containers)
│   ├── helpers/      - Test utilities (test_app, test database)
│   ├── repositories/ - Repository tests
│   └── *.rs          - Feature-specific integration tests
├── e2e/              - End-to-end tests (full workflows)
├── handlers/         - Handler tests
└── main.rs           - Test suite entry point
```

### Test Environment

The `docker-compose.test.yml` provides:
- PostgreSQL on port 5443 (database: `crawlrs_test`)
- Redis on port 6382
- Chrome headless on port 9222
- FlareSolverr on port 8191 (for Google search)

### Test Utilities

Located in `tests/integration/helpers/`:
- `test_app.rs` - Creates test application instances with isolated dependencies
- `test_database.rs` - Database setup and teardown
- `mod.rs` - Common test fixtures and utilities

### Running Tests

```bash
# Run all tests
cargo test

# Run unit tests only
cargo test --lib

# Run integration tests
cargo test --test integration_tests

# Run specific test
cargo test test_search_aggregation

# Run tests with detailed output
RUST_LOG=debug cargo test -- --nocapture

# Run tests in parallel
cargo test -- --test-threads=4
```

## Dependencies

### Core Dependencies

- **Web Framework**: Axum 0.8
- **ORM**: SeaORM 1.0
- **Async Runtime**: Tokio 1.48
- **Database**: PostgreSQL 15+, SQLite
- **Cache**: Redis 7+
- **HTTP Client**: reqwest 0.12 (with rustls-tls)
- **Browser Automation**: chromiumoxide 0.8
- **Rate Limiting**: governor 0.10
- **Logging**: tracing 0.1, tracing-subscriber 0.3
- **Metrics**: metrics 0.24, metrics-exporter-prometheus 0.18
- **Serialization**: serde 1.0
- **Error Handling**: thiserror 2.0, anyhow 1.0
- **Text Processing**: scraper 0.25, deunicode 1.6
- **AWS SDK**: aws-sdk-s3 1.118, aws-config 1.8

### Development Dependencies

- **Testing**: testcontainers, wiremock, axum-test
- **Security**: cargo-deny (audit configured in `deny.toml`)
- **Clippy**: Lint rules configured

## Security

- **SSRF Protection**: URL validation in `engines/validators.rs`
- **Auth**: Bearer token authentication with configurable API keys
- **Rate Limiting**: Two-layer protection (API + concurrency)
- **Input Validation**: Using `validator` crate with derive macros
- **Security Audit**: Known vulnerabilities tracked in `deny.toml`

## Performance Considerations

### Engine Selection

The `EngineRouter` automatically selects the optimal engine based on:
- URL characteristics (JavaScript-heavy sites → Playwright)
- Engine health and success rates
- Response time statistics
- Circuit breaker state

### Caching Strategy

- Redis caching for: robots.txt, search results, rate limits
- TTL-based expiration for cached data
- LRU cache for frequently accessed data

### Concurrency Control

- Team-level semaphore prevents resource exhaustion
- Database connection pooling with configurable limits
- Worker pool with configurable concurrency

## Git Workflow

This project follows a strict Git workflow defined in `.trae/rules/git.md`:

### Branch Strategy

- `main` - Production branch, always deployable
- `develop` - Development integration branch
- `feature/<ticket-id>-<description>` - New features
- `bugfix/<ticket-id>-<description>` - Bug fixes
- `hotfix/<version>-<description>` - Production hotfixes

### Commit Guidelines

- **Atomic Commits**: Each commit must be atomic and small
- **Modular/File-level**: Never commit entire codebase at once
- **Conventional Commits**: `type(scope): subject`
  - `feat`: New feature
  - `fix`: Bug fix
  - `docs`: Documentation
  - `refactor`: Code refactoring
  - `test`: Test changes
  - `chore`: Build/tooling changes

**Example:**
```
feat(engines): add adaptive retry policy for Playwright engine

Implement exponential backoff with jitter for transient failures.
- Add configurable max retries and backoff multiplier
- Track success rate per engine for adaptive behavior
- Fix issue where Playwright would exhaust retries too quickly

Closes #123
```

### Using MCP-Git Tools

When AI-assisted, use MCP-Git tools for version control:
1. `git_status` - Check current changes
2. `git_add` - Add files per-module (never use `.` wildcard)
3. `git_commit` - Commit with detailed messages following Conventional Commits
4. Review AI-generated commit messages for accuracy

## Code Standards

Based on `.trae/rules/`:

### Rust Best Practices

- Ownership/Borrowing: Prefer borrowing over cloning, minimize `.clone()`
- Type System: Use `Option<T>` over null, `Result<T, E>` for error handling
- Async: Use channels for inter-task communication, avoid blocking operations
- Performance: Pre-allocate `Vec` capacity, iterator chains over loops

### Code Style

- Format: `cargo fmt` (4 spaces, 120 char line limit)
- Naming: `snake_case` for functions/variables, `PascalCase` for types
- Linting: `cargo clippy -- -D warnings`

### Error Handling

- Library code: Return `Result<T, E>`, use `thiserror` for custom errors
- Application code: Use `anyhow` for simplified error handling
- Never panic in library code, use `?` operator for propagation

### Documentation

- Public APIs: Document with `///` comments including examples
- Complex logic: Explain "why", not "what"
- README: Quick start guide and architecture overview

## Development Workflow

1. **Setup**
   ```bash
   git checkout -b feature/ticket-description
   cargo build
   ```

2. **Make Changes**
   - Write code following the architecture pattern
   - Add tests for new functionality
   - Run `cargo fmt` and `cargo clippy`

3. **Test**
   ```bash
   cargo test
   cargo clippy -- -D warnings
   ```

4. **Commit**
   ```bash
   git add src/module/file.rs  # Add per-module, not all at once
   git commit -m "type(scope): description"
   ```

5. **Push and PR**
   ```bash
   git push origin feature/ticket-description
   # Create PR in GitHub/Gitee
   ```

## Common Tasks

### Adding a New Scraping Engine

1. Implement `ScraperEngine` trait in `src/engines/my_engine.rs`
2. Add engine to `src/engines/mod.rs`
3. Register engine in `src/main.rs` when creating `EngineRouter`
4. Add tests in `tests/unit/engines/` and `tests/integration/`
5. Update health monitor to track engine status

### Adding a New API Endpoint

1. Define request/response DTOs in `src/application/dto/`
2. Create use case in `src/application/use_cases/`
3. Add handler in `src/presentation/handlers/`
4. Register route in `src/presentation/routes/`
5. Add integration tests in `tests/integration/handlers/`
6. Update API documentation

### Adding a New Repository

1. Define trait in `src/domain/repositories/`
2. Implement in `src/infrastructure/repositories/`
3. Register in dependency injection (main.rs)
4. Add tests using test utilities

### Debugging Issues

1. Enable debug logging: `RUST_LOG=debug cargo run`
2. Check logs in `logs/` directory
3. Use `RUST_BACKTRACE=1` for stack traces
4. Inspect database: `psql $DATABASE_URL`
5. Check Redis: `redis-cli -h $REDIS_HOST`
6. Use metrics endpoint for runtime stats: `GET /metrics`

## Important Notes

- **Never commit secrets**: API keys, passwords, tokens should use environment variables
- **Database migrations**: Always test migrations before merging
- **Breaking changes**: Increment major version, update CHANGELOG.md
- **Performance**: Profile with `cargo flamegraph` before optimizing
- **Memory**: Watch for memory leaks in long-running workers
- **Security**: Address CVEs reported by `cargo audit`
- **Testing**: Maintain >80% test coverage on critical paths

## Troubleshooting

### Port Already in Use

The application has built-in port detection (`enable_port_detection = true`). If the default port (8899) is occupied, it will automatically find an available port.

### Database Connection Failures

- Check PostgreSQL is running: `psql $DATABASE_URL`
- Verify connection pool settings in config
- Check `migration/` for pending migrations

### Redis Connection Failures

- Check `redis-cli -h $REDIS_HOST ping`
- Verify Redis URL in config
- Check if Redis is reachable from container/network

### Browser Test Failures

- Verify Chrome container is running and healthy
- Check CDP URL configuration
- Increase timeout for browser operations

### Workers Not Processing Tasks

- Check `TaskQueue` is properly initialized
- Verify workers are spawned: check logs for worker startup
- Check database for stuck tasks in `pending` state
- Verify circuit breaker isn't open
