# Change: Fix Engine Module Critical Issues

## Why

The engine module has several critical security, correctness, and performance issues that need immediate attention:

### P0 - Security & Correctness
1. **SearchHealthChecker Cross-Domain Contamination**: The current implementation uses a single `consecutive_failures`/`last_check` state shared across all domains. This causes incorrect health assessments when one domain's failures affect another domain's health status, leading to false positives/negatives in search health checks.

2. **SSRF Protection Bypass Vulnerability**: `EngineClient::scrape` validates URLs via `validate_url`, but internal code paths (like `ScrapeWorker`) directly call `EngineRouter::route` without validation. FireEngineTls/FireEngineCdp engines also don't perform SSRF checks before making requests to external Fire Engine services, creating potential SSRF attack vectors.

3. **EngineClient Health Monitor Inconsistency**: `EngineClient::with_router` creates an `EngineHealthMonitor` with an empty engine list, while `with_engines` properly syncs engines from the router. This inconsistency means custom routers with registered engines will have non-functional health checks.

### P1 - Performance & Availability
4. **HttpClient UA Rotation Broken**: The `HttpClient::new()` sets a User-Agent header once during client construction, but subsequent requests reuse the same client without rotating UA. This defeats the purpose of UA rotation for anti-fingerprinting.

5. **PlaywrightEngine Response Information Loss**: The engine returns hardcoded `status_code: 200` and empty `headers`, making it impossible to get the actual HTTP response status, headers, or final URL after redirects. This breaks上层 logic for billing, retry strategies, content-type detection, and error classification.

6. **Router Statistics Pollution**: `EngineRouter::aggregate` updates statistics using `Duration::from_millis(0)` when timing fails, which artificially lowers average response times and corrupts metrics used for engine selection.

7. **CircuitBreaker Lock Model Inconsistency**: The `CircuitBreaker` uses `std::sync::RwLock` (with poison handling), while `EngineRouter` uses `parking_lot::RwLock`. This inconsistency adds complexity, potential poison-related bugs, and performance overhead.

### P2 - Architecture & Maintainability
8. **Deprecated Traits Still Used in Core Path**: Despite `engines::traits` being marked deprecated, the core scraping path (`CreateScrapeUseCase`, `ScrapeWorker`) still uses `EngineRouter` + `traits::ScrapeRequest` directly, maintaining two parallel APIs and increasing maintenance burden.

9. **EngineClient Missing Capabilities**: Public `ScrapeOptions` lacks headers, TLS fingerprinting, and Fire Engine forcing options that are available in the internal `ScrapeRequest`. This limits the public API's expressiveness and forces users to use internal types.

10. **Error Semantics Lost in Conversion**: `convert_error` maps all internal timeouts to a fixed 30s duration, losing the actual timeout value from the request and breaking monitoring/retry logic.

11. **Unconfigurable Engine Scoring**: `support_score()` is hardcoded with opaque weights, making it impossible to tune for different business scenarios (high throughput vs. high success rate vs. low cost).

12. **Missing Observability**: The routing layer lacks key metrics (candidate count, attempt count, per-engine latency histograms, failure classification) that are needed to diagnose slow/broken engines.

## What Changes

### P0 - Security & Correctness
- **FIXED**: Refactor `SearchHealthChecker` to use `HashMap<domain, HealthState>` for per-domain state tracking
- **FIXED**: Add `validate_url` call at the start of `EngineRouter::route` to prevent SSRF bypasses
- **FIXED**: Update `EngineClient::with_router` to sync engines from router when creating health monitor

### P1 - Performance & Availability
- **IMPROVED**: Modify `HttpClient` to rotate UA per-request or maintain UA-aware client pool
- **IMPROVED**: Update `PlaywrightEngine` to capture real response status, headers, and final URL via CDP events
- **FIXED**: Only record timing metrics when valid duration is obtained, skip 0ms entries
- **IMPROVED**:统一 `CircuitBreaker` to use `parking_lot::RwLock` for consistency and performance

### P2 - Architecture & Maintainability
- **MIGRATED**: Update `CreateScrapeUseCase` and `ScrapeWorker` to use `EngineClient` instead of direct `EngineRouter` access
- **EXTENDED**: Add headers, `needs_tls_fingerprint`, `use_fire_engine` as advanced options to `ScrapeOptions`
- **IMPROVED**: Preserve original timeout duration in error conversion instead of hardcoding 30s
- **CONFIGURABLE**: Extract engine scoring weights and thresholds to configuration
- **ADDED**: Add routing layer metrics (candidate count, attempts, per-engine latency, failure breakdown)

## Impact

- Affected specs: `engines` (MODIFIED)
- Affected code:
  - `src/engines/search_health.rs` - Domain state tracking fix
  - `src/engines/router.rs` - SSRF validation, metrics, lock model
  - `src/engines/engine_client.rs` - Health monitor sync, error semantics
  - `src/engines/client/http_client.rs` - UA rotation per-request
  - `src/engines/client/playwright.rs` - Response info capture
  - `src/engines/circuit_breaker.rs` - Lock model统一
  - `src/application/use_cases/create_scrape.rs` - Migration to EngineClient
  - `src/workers/scrape_worker.rs` - Migration to EngineClient
- Breaking changes: None (all changes maintain backward compatibility)
- Performance impact: Positive (better metrics, consistent locking, proper UA rotation)

## Migration Strategy

1. Changes are applied incrementally without breaking existing functionality
2. New behavior (per-domain health, SSRF validation, metrics) is transparent to callers
3. Lock model change is internal and doesn't affect public API
4. Playwright response capture may change behavior for callers relying on hardcoded 200 status (unlikely but possible)
5. Configuration for engine scoring is additive - defaults match current behavior
