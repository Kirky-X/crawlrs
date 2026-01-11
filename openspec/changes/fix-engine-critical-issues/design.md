## Context

This change addresses multiple critical issues in the engine module across security, correctness, performance, and architecture dimensions. The issues are interconnected but can be addressed independently.

### Constraints & Stakeholders
- **Security Team**: SSRF bypass vulnerability is critical and must be fixed
- **Platform Team**: Health checks and routing metrics affect availability
- **Core Team**: API consistency and code maintainability are priorities
- **No Breaking Changes**: All changes must maintain backward compatibility

## Goals / Non-Goals

### Goals
1. Fix security vulnerabilities (SSRF bypass, cross-domain contamination)
2. Correct correctness issues (health check state, response info loss)
3. Improve performance (UA rotation, lock model, metric accuracy)
4. Enhance maintainability (unified APIs, configurable scoring, observability)

### Non-Goals
1. Complete migration to EngineClient (tracked in separate change `refactor-engines-architecture`)
2. Rewrite PlaywrightEngine implementation from scratch (minimal changes for response capture)
3. Add new search engines (out of scope)
4. Modify database schema or storage layer (not needed)

## Decisions

### D1: SearchHealthChecker Domain State
**Decision**: Use `HashMap<Domain, HealthState>` where `HealthState` contains `consecutive_failures`, `last_check`, `consecutive_successes`

**Rationale**:
- Each domain's health is independent and should be tracked separately
- Domain is the natural granularity (not engine name + domain)
- Simple and efficient - O(1) lookup and update
- Existing infrastructure uses domain-based routing

**Alternatives Considered**:
- `HashMap<(engine_name, domain), HealthState>` - More granular but adds engine coupling
- `Vec<HealthState>` with parallel arrays - Less idiomatic Rust, more error-prone
- External cache (Redis) - Overkill for in-memory health tracking

**Location**: `src/engines/search_health.rs`

### D2: SSRF Protection Layer
**Decision**: Add `validate_url` call at the beginning of `EngineRouter::route` method

**Rationale**:
- Centralized validation at the routing layer prevents all bypasses
- Defense in depth - EngineClient still validates, but router provides fallback
- Minimal performance impact (validation is already fast)
- Consistent with security principle: validate at trust boundaries

**Implementation**:
```rust
pub async fn route(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
    // NEW: SSRF validation as safety net
    if let Err(e) = validators::validate_url(&request.url).await {
        return Err(EngineError::SsrfProtection(e.to_string()));
    }
    // ... rest of routing logic
}
```

**Alternatives Considered**:
- Validate in all engine implementations - Code duplication, maintenance burden
- Validate only in EngineClient - Already done, but internal routes bypass it
- Create middleware pattern - Over-engineering for this use case

**Location**: `src/engines/router.rs`

### D3: HttpClient UA Rotation
**Decision**: Rotate User-Agent header per-request in `HttpClient::request` method

**Rationale**:
- Rotating per-request is simpler than maintaining UA-aware client pool
- No additional synchronization complexity
- Works with existing connection pooling (same client, different headers)
- Request overhead is minimal compared to network latency

**Implementation**:
```rust
async fn request(&self, url: &str) -> Result<HttpResponse, HttpError> {
    let ua = self.ua_rotator.next();
    let response = self.client
        .get(url)
        .header("User-Agent", ua)
        .send()
        .await?;
    // ...
}
```

**Alternatives Considered**:
- UA-aware client pool - More complex, unclear benefit for crawlrs use case
- Per-request client creation - High overhead, defeats connection pooling
- Batch rotation (every N requests) - Adds state management complexity

**Location**: `src/engines/client/http_client.rs`

### D4: Playwright Response Capture
**Decision**: Capture response metadata via CDP network events using `network.response_received` and `network.loadingFinished`

**Rationale**:
- CDP provides accurate response information including status, headers, and timing
- Minimal code changes - just add event listeners during page navigation
- Works with existing Playwright setup (already using CDP)
- Captures redirects (final URL after navigation)

**Implementation**:
- Add network event listener before navigation
- Store response data in local context
- Return captured data in ScrapeResponse

**Alternatives Considered**:
- HTTP response interception - Requires browser extension or proxy
- Multiple page.goto() calls - High overhead
- Rely on Playwright's built-in response object - Not available in CDP mode

**Location**: `src/engines/client/playwright.rs`

### D5: CircuitBreaker Lock Model
**Decision**: Change from `std::sync::RwLock` to `parking_lot::RwLock` for consistency

**Rationale**:
- Already using `parking_lot` in `EngineRouter` - consistency across modules
- `parking_lot` is faster (no poisoning, optimized synchronization)
- No breaking changes - lock API is compatible
- Standard practice in high-performance Rust code

**Migration**:
```rust
// Before
use std::sync::RwLock;
let state = RwLock::new(initial_state);

// After
use parking_lot::RwLock;
let state = RwLock::new(initial_state);
```

**Location**: `src/engines/circuit_breaker.rs`

### D6: Engine Scoring Configuration
**Decision**: Create configuration struct with weighted factors, loaded from config file with defaults matching current behavior

**Rationale**:
- Current hardcoded values are opaque and hard to tune
- Configuration allows tuning for different business priorities
- Defaults ensure no behavior change for existing deployments
- YAML/HOCON format integrates with existing config system

**Configuration Structure**:
```yaml
[engine.scoring]
# Base scores (0-100)
reqwest_base_score = 10
playwright_base_score = 10
fire_engine_base_score = 60

# Statistical weights
success_rate_weight = 0.4
latency_weight = 0.3
error_rate_weight = 0.3

# Thresholds
min_success_rate = 0.5
max_latency_ms = 10000
```

**Location**: `src/config/engine.rs` (new), `src/engines/router.rs`

### D7: Routing Layer Metrics
**Decision**: Add structured metrics using existing `tracing` crate with explicit metric names

**Metrics**:
- `engine.router.candidates` - Number of candidate engines per request
- `engine.router.attempts` - Number of engines attempted
- `engine.router.success` - Boolean success indicator
- `engine.router.latency_ms` - Per-engine latency histogram
- `engine.router.fail_reason` - Categorized failure reason

**Implementation**:
- Add metric emission in `route_internal` method
- Use existing metrics infrastructure
- Tags: engine_name, fail_category

**Location**: `src/engines/router.rs`

## Risks / Trade-offs

| Risk | Impact | Mitigation |
|------|--------|------------|
| Playwright CDP event timing | May miss events if page loads too fast | Add timeout, use existing page.wait_for_load_state |
| Lock model change introduces bugs | Medium | Tests cover lock behavior, lock API compatible |
| Metrics performance overhead | Low | Only emitted on hot path, structured logging exists |
| Configuration migration | Low | Defaults match current behavior |

## Migration Plan

1. **Phase 1** (P0 fixes):
   - Fix SearchHealthChecker domain state
   - Add SSRF validation to router
   - Fix EngineClient health monitor sync

2. **Phase 2** (P1 improvements):
   - Implement UA rotation per-request
   - Capture Playwright response info
   - Fix router statistics (skip 0ms)
   -统一 lock model

3. **Phase 3** (P2 enhancements):
   - Migrate core paths to EngineClient (partial)
   - Extend ScrapeOptions capabilities
   - Improve error semantics
   - Add engine scoring configuration
   - Add routing metrics

Each phase is independent and can be tested/deployed separately.

## Open Questions

1. **Q1**: Should engine scoring configuration support per-engine overrides?
   - Current thought: No, keep simple weighted average first
   - Revisit after seeing usage patterns

2. **Q2**: Should routing metrics use Prometheus format or generic structured logs?
   - Current thought: Structured logs, Prometheus exporter can parse
   - Keep options open for future metric backend flexibility

3. **Q3**: How to handle Fire Engine SSRF validation?
   - Current thought: Fire Engine URLs are fixed (configured), only validate target URL
   - Need to verify with security team
