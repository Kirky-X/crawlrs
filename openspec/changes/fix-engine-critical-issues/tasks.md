## 1. P0 - Security & Correctness Fixes

### 1.1 SearchHealthChecker Domain State Fix
- [ ] 1.1.1 Read `src/engines/search_health.rs` to understand current implementation
- [ ] 1.1.2 Define `HealthState` struct with `consecutive_failures`, `consecutive_successes`, `last_check`, `total_requests`, `total_failures`
- [ ] 1.1.3 Change `SearchHealthChecker` to use `HashMap<String, HealthState>` for per-domain tracking
- [ ] 1.1.4 Update `check_engine` method to use domain-specific state
- [ ] 1.1.5 Update `get_health_status` to aggregate domain states correctly
- [ ] 1.1.6 Run tests to verify health check behavior

### 1.2 SSRF Protection Bypass Fix
- [ ] 1.2.1 Read `src/engines/router.rs` to understand `route` method
- [ ] 1.2.2 Add `use crate::engines::validators::validate_url;` import
- [ ] 1.2.3 Add `validate_url` call at start of `route` method
- [ ] 1.2.4 Propagate validation errors as `EngineError::SsrfProtection`
- [ ] 1.2.5 Test that SSRF protection works for direct `EngineRouter::route` calls

### 1.3 EngineClient Health Monitor Sync Fix
- [ ] 1.3.1 Read `src/engines/engine_client.rs` to understand `with_router` implementation
- [ ] 1.3.2 Modify `EngineClient::with_router` to extract engines from router
- [ ] 1.3.3 Pass engines list to `EngineHealthMonitor::new`
- [ ] 1.3.4 Verify `health_check()` returns correct status for custom routers
- [ ] 1.3.5 Run tests to verify no regressions

## 2. P1 - Performance & Availability Improvements

### 2.1 HttpClient UA Rotation
- [ ] 2.1.1 Read `src/engines/client/http_client.rs` to understand current UA handling
- [ ] 2.1.2 Implement `UserAgentRotator` struct with pool of user agents
- [ ] 2.1.3 Modify `HttpClient::request` to call `ua_rotator.next()` and set header
- [ ] 2.1.4 Add unit tests for UA rotation behavior
- [ ] 2.1.5 Verify connection pooling still works (same client, different headers)

### 2.2 PlaywrightEngine Response Capture
- [ ] 2.2.1 Read `src/engines/client/playwright.rs` to understand current implementation
- [ ] 2.2.2 Add CDP network event listener setup before page navigation
- [ ] 2.2.3 Store response status, headers, and final URL from network events
- [ ] 2.2.4 Update `ScrapeResponse` construction to use captured data
- [ ] 2.2.5 Handle redirect chains to get final URL
- [ ] 2.2.6 Add tests for response metadata accuracy

### 2.3 Router Statistics Pollution Fix
- [ ] 2.3.1 Read `src/engines/router.rs` to find `aggregate` method
- [ ] 2.3.2 Change timing logic to only record valid durations (duration > 0)
- [ ] 2.3.3 Add debug logging when 0ms duration is detected (for debugging)
- [ ] 2.3.4 Update statistics calculation to skip zero values
- [ ] 2.3.5 Verify average latency metrics are now accurate

### 2.4 CircuitBreaker Lock Model统一
- [ ] 2.4.1 Read `src/engines/circuit_breaker.rs` to understand current implementation
- [ ] 2.4.2 Change import from `std::sync::RwLock` to `parking_lot::RwLock`
- [ ] 2.4.3 Remove any poison handling code (parking_lot doesn't poison)
- [ ] 2.4.4 Verify all lock operations still compile
- [ ] 2.4.5 Run tests to verify circuit breaker behavior unchanged

## 3. P2 - Architecture & Maintainability Enhancements

### 3.1 ScrapeOptions Capability Extension
- [ ] 3.1.1 Read `src/engines/engine_client.rs` to understand current `ScrapeOptions`
- [ ] 3.1.2 Add `headers: HashMap<String, String>` field to `ScrapeOptions`
- [ ] 3.1.3 Add `needs_tls_fingerprint: bool` field to `ScrapeOptions`
- [ ] 3.1.4 Add `use_fire_engine: bool` field to `ScrapeOptions`
- [ ] 3.1.5 Update `ScrapeOptionsBuilder` with methods for new fields
- [ ] 3.1.6 Update `from_public` conversion in traits.rs to map new fields
- [ ] 3.1.7 Run tests to verify no regressions

### 3.2 Error Semantics Precision
- [ ] 3.2.1 Read `src/engines/engine_client.rs` `convert_error` function
- [ ] 3.2.2 Change timeout conversion to preserve original duration from request
- [ ] 3.2.3 Pass request timeout to `convert_error` or extract from error context
- [ ] 3.2.4 Update `EngineError::Timeout` to include original duration
- [ ] 3.2.5 Verify monitoring tools can now see actual timeout values

### 3.3 Engine Scoring Configuration
- [ ] 3.3.1 Create `src/config/engine.rs` with `EngineScoringConfig` struct
- [ ] 3.3.2 Define configuration fields: base scores, weights, thresholds
- [ ] 3.3.3 Load configuration from config file with defaults
- [ ] 3.3.4 Update `EngineRouter` to use configurable scoring
- [ ] 3.3.5 Add tests for configurable scoring behavior

### 3.4 Routing Layer Metrics
- [ ] 3.4.1 Read `src/engines/router.rs` to identify metric emission points
- [ ] 3.4.2 Add metrics for candidate count (before routing)
- [ ] 3.4.3 Add metrics for attempt count and success/failure
- [ ] 3.4.4 Add per-engine latency histogram metric
- [ ] 3.4.5 Add failure classification metric (timeout, SSRF, network, etc.)
- [ ] 3.4.6 Verify metrics don't impact performance significantly

## 4. Testing & Validation

### 4.1 Unit Tests
- [ ] 4.1.1 Add tests for SearchHealthChecker per-domain behavior
- [ ] 4.1.2 Add tests for UA rotation
- [ ] 4.1.3 Add tests for Playwright response capture
- [ ] 4.1.4 Add tests for router statistics accuracy
- [ ] 4.1.5 Add tests for error timeout preservation

### 4.2 Integration Tests
- [ ] 4.2.1 Test SSRF protection bypass fix end-to-end
- [ ] 4.2.2 Test EngineClient health check with custom router
- [ ] 4.2.3 Test full scrape flow with new response metadata

### 4.3 Validation
- [ ] 4.3.1 Run `cargo clippy -- -D warnings` for linting
- [ ] 4.3.2 Run `cargo test --lib` for all tests
- [ ] 4.3.3 Verify `openspec validate fix-engine-critical-issues --strict` passes

## 5. Documentation

- [ ] 5.1 Update inline comments for new fields in ScrapeOptions
- [ ] 5.2 Document UA rotation behavior in HttpClient
- [ ] 5.3 Document engine scoring configuration options
- [ ] 5.4 Add CHANGELOG entry for security fixes
