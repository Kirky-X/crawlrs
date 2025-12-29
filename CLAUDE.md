# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

crawlrs is an enterprise-grade web scraping platform written in Rust, providing search, scraping, crawling, and structured extraction capabilities. It's built with a clean architecture pattern and uses Axum for the web framework, SeaORM for database access, and Tokio for async runtime.

## Build Commands

```bash
# Development build
cargo build

# Release build
cargo build --release

# Run with custom config
cargo run -- --config config/default.toml

# Format code
cargo fmt

# Lint with strict warnings
cargo clippy -- -D warnings

# Run all tests
cargo test

# Run specific test
cargo test --test integration_tests

# Run benchmark
cargo bench
```

## Architecture

The project follows a clean architecture pattern with the following layers:

### Directory Structure

- **src/application** - Use cases and DTOs for business logic
- **src/domain** - Domain models and entities
- **src/infrastructure** - Database, web framework, external services
- **src/presentation** - API handlers and request/response handling
- **src/engines** - Scraping engines (Fetch, Playwright, Fire Engine TLS/CDP)
- **src/workers** - Background task workers (scrape, webhook, expiration, backlog)
- **src/utils** - Utility functions (text processing, encoding, telemetry)
- **src/queue** - Task queue management
- **src/config** - Configuration loading

### Key Components

- **Main entry point**: `src/main.rs` - Initializes the application with config loading and worker spawning
- **Engines**: Strategy pattern in `src/engines/` - Selects optimal scraping engine (Fetch/Playwright/Fire Engine TLS/Fire Engine CDP)
- **Workers**: Background workers in `src/workers/` handle async task processing with circuit breaker protection
- **Use cases**: Located in `src/application/use_cases/` - Business logic for crawl, scrape, and extract operations
- **Configuration**: TOML-based config in `config/` directory with environment variable support

### Data Flow

API requests flow through Axum handlers → Use cases → Engine router → Workers → Database/Storage

### Database

Uses SeaORM with PostgreSQL and SQLite support. Migrations are in the `migration/` directory.

## Configuration

Configuration is loaded from `config/` directory with support for environment-specific overrides:
- `default.toml` - Base configuration
- `local.toml` - Local development overrides
- Environment variables can override config values

Key configuration sections:
- `server` - Host, port, and API keys
- `auth` - Bearer token authentication
- `concurrency` - Team concurrency limits
- `rate_limiting` - API rate limiting
- `webhook` - Webhook settings
- `engines` - Engine-specific configurations

## Testing

Integration tests are located in `tests/` directory with Cucumber framework support. The project uses:
- WireMock for HTTP mocking
- Testcontainers for database testing
- axum-test for API testing

## Dependencies

- **Web Framework**: Axum 0.8
- **ORM**: SeaORM 1.0
- **Async Runtime**: Tokio 1.48
- **HTTP Client**: reqwest 0.12
- **Browser Automation**: chromiumoxide 0.8
- **Database**: PostgreSQL 15+ and SQLite
- **Cache**: Redis 7+

## Feature Flags

- `playwright` - Enables Playwright browser automation support
