## Context

The engines module provides web scraping capabilities through multiple engine implementations (reqwest, playwright, fire_cdp, fire_tls). Currently, the architecture exposes too many internal details:

**Current Architecture Problems:**
- `ScraperEngine` trait is public, allowing direct access to engines
- `support_score()` exposes engine selection logic to callers
- Circuit breaker state is queryable via public methods
- UA rotation requires manual handling by callers
- No unified error handling strategy

**Goals:**
- Provide a simple, opinionated API for scraping
- Hide all internal complexity (routing, retries, UA rotation, circuit breaking)
- Make engine selection fully automatic and opaque
- Enable future engine additions without API changes

## Goals / Non-Goals

### Goals
1. Create single entry point: `EngineClient`
2. Unified request/response types with clear semantics
3. Automatic engine selection (no `support_score()` exposure)
4. Built-in retries, UA rotation, circuit breaking
5. Health check endpoint for monitoring

### Non-Goals
- Support for custom engine selection strategies (use defaults)
- Per-request engine override (always use smart routing)
- Direct access to engine internals (maintain encapsulation)
- Manual circuit breaker control (automatic only)

## Decisions

### 1. Unified EngineClient API

**Decision**: Create `EngineClient` as the only public interface

```rust
pub struct EngineClient {
    router: Arc<EngineRouter>,
    health_monitor: Arc<EngineHealthMonitor>,
    // Internal fields hidden
}

impl EngineClient {
    pub async fn scrape(&self, request: ScrapeRequest) -> Result<ScrapeResponse, EngineError>;
    pub async fn health_check(&self) -> EngineHealthStatus;
}
```

**Rationale**: Single entry point simplifies API and enables future optimizations

**Alternative Considered**: Keep trait-based API with builder pattern
- Rejected: Too flexible, allows bypassing smart routing

### 2. Request/Response Structure

**Decision**: Single `ScrapeRequest` and `ScrapeResponse` with optional fields

```rust
pub struct ScrapeRequest {
    pub url: String,
    pub options: ScrapeOptions,  // Optional
}

pub struct ScrapeOptions {
    pub needs_js: bool,          // Default: false
    pub needs_screenshot: bool,  // Default: false
    pub mobile: bool,            // Default: false
    pub timeout: Duration,       // Default: 30s
    // ... other options
}
```

**Rationale**: Clear separation between required and optional fields

### 3. Internal-Only Features

**Decision**: The following are implementation details, not public API:

- User-Agent rotation
- Circuit breaker state
- Engine selection algorithm
- Retry logic and backoff
- Connection pooling

**Rationale**: These are cross-cutting concerns handled automatically

### 4. Health Check API

**Decision**: Expose health status, not detailed metrics

```rust
pub enum EngineHealthStatus {
    Healthy,
    Degraded(Vec<String>),
    Unavailable,
}

impl EngineClient {
    pub async fn health_check(&self) -> EngineHealthStatus;
}
```

**Rationale**: Simple status for monitoring, detailed metrics via telemetry

## Risks / Trade-offs

| Risk | Impact | Mitigation |
|------|--------|------------|
| Breaking existing callers | High | Provide migration period with deprecation warnings |
| Performance overhead | Medium | Benchmark and optimize hot paths |
| Loss of flexibility | Medium | Document that advanced use cases need direct engine access |

## Migration Plan

1. **Phase 1**: Create `EngineClient` alongside existing API (2 days)
2. **Phase 2**: Update all internal callers to use `EngineClient` (3 days)
3. **Phase 3**: Mark old API as `#[deprecated]` (1 day)
4. **Phase 4**: Remove old API after migration complete (1 day)

## Open Questions

1. Should we support async streaming responses for large content?
2. Do we need request batching for high-throughput scenarios?
3. Should health checks include per-engine status or just aggregate?
