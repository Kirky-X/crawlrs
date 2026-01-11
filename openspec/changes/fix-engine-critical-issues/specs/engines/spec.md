## ADDED Requirements

### Requirement: Domain-Isolated Search Health Checking
The system SHALL track health state per domain to prevent cross-domain contamination.

The SearchHealthChecker MUST maintain a `HashMap<Domain, HealthState>` where:
- Each domain has independent `consecutive_failures`, `consecutive_successes`, `last_check`, `total_requests`, and `total_failures`
- Health status is calculated independently for each domain
- Failures on one domain do not affect health assessment of other domains

#### Scenario: Single domain failure isolation
- **WHEN** search engine A fails for domain "example.com" 5 times
- **THEN** domain "example.com" is marked as unhealthy for engine A
- **AND** domain "test.org" remains at its previous health status for engine A

#### Scenario: Per-domain recovery
- **WHEN** domain "example.com" has 5 consecutive failures then 3 consecutive successes
- **THEN** domain "example.com" health status is updated to reflect recovery
- **AND** other domains' health statuses are unchanged

### Requirement: Router-Level SSRF Protection
The system SHALL validate all URLs at the routing layer to prevent SSRF bypass vulnerabilities.

The EngineRouter MUST call `validate_url` at the start of the `route` method before any engine selection or request processing. URLs that fail validation MUST be rejected with `EngineError::SsrfProtection` regardless of the entry point.

#### Scenario: SSRF protection on direct router access
- **WHEN** `EngineRouter::route` is called with a private IP URL (e.g., http://192.168.1.1/)
- **THEN** the request is rejected with `EngineError::SsrfProtection`
- **AND** no request is made to the private IP address

#### Scenario: Defense in depth with EngineClient
- **WHEN** `EngineClient::scrape` is called with a valid public URL
- **THEN** the URL is validated at EngineClient level
- **AND** if internal routing occurs, the URL is validated again at router level

### Requirement: EngineClient Health Monitor Synchronization
The system SHALL ensure health monitoring reflects actual registered engines.

EngineClient MUST sync engines from the router when creating the health monitor, ensuring that `health_check()` returns accurate status for all registered engines regardless of how the client was constructed.

#### Scenario: Custom router with engines
- **WHEN** `EngineClient::with_router(router)` is called where router has engines registered
- **THEN** the health monitor is created with those same engines
- **AND** `health_check()` returns status based on actual registered engines

### Requirement: Per-Request User-Agent Rotation
The system SHALL rotate User-Agent headers per HTTP request to prevent fingerprinting.

The HttpClient MUST set a rotating User-Agent header on each request, not just during client construction. The rotation SHOULD use a pool of common browser user agents.

#### Scenario: Multiple requests with different UAs
- **WHEN** HttpClient makes 3 sequential requests to the same endpoint
- **THEN** each request has a different User-Agent header
- **AND** connection pooling is maintained (same underlying client)

### Requirement: Playwright Response Metadata Capture
The system SHALL capture actual HTTP response status, headers, and final URL from browser automation.

The PlaywrightEngine MUST use CDP network events to capture response information instead of returning hardcoded values. The captured metadata MUST include:
- HTTP status code (from responseReceived event)
- Response headers (from responseReceived event)
- Final URL after redirects (from loadingFinished or domContentLoaded event)

#### Scenario: Capturing real HTTP status
- **WHEN** PlaywrightEngine navigates to a page that returns 404
- **THEN** the ScrapeResponse contains `status_code: 404`
- **AND** `content_type` reflects the actual Content-Type header

#### Scenario: Capturing final URL after redirect
- **WHEN** PlaywrightEngine navigates to a URL that redirects (301/302)
- **THEN** the ScrapeResponse contains the final URL after redirect
- **AND** `headers` include the actual response headers

### Requirement: Accurate Router Statistics
The system SHALL record only valid timing measurements to prevent metric pollution.

The EngineRouter MUST skip recording timing metrics when the duration is zero or invalid, ensuring that average latency calculations reflect actual request times.

#### Scenario: Zero duration not recorded
- **WHEN** timing measurement returns 0ms
- **THEN** that measurement is not included in statistics
- **AND** average latency is calculated from valid measurements only

### Requirement: Unified Lock Model
The system SHALL use `parking_lot::RwLock` consistently across all synchronization primitives.

The CircuitBreaker and all other synchronization primitives MUST use `parking_lot` locks for consistency, performance, and to avoid poison-related edge cases.

#### Scenario: Consistent lock behavior
- **WHEN** CircuitBreaker state is accessed under contention
- **THEN** the lock uses `parking_lot` implementation
- **AND** no poison-related errors occur

### Requirement: Extended ScrapeOptions Capabilities
The system SHALL expose advanced request options through the public ScrapeOptions API.

The ScrapeOptions struct MUST support:
- Custom HTTP headers (`headers: HashMap<String, String>`)
- TLS fingerprint resistance (`needs_tls_fingerprint: bool`)
- Force Fire Engine usage (`use_fire_engine: bool`)

These options MUST be accessible via ScrapeOptionsBuilder and passed through to internal request processing.

#### Scenario: Custom headers in public API
- **WHEN** ScrapeOptionsBuilder.headers() is called with custom headers
- **THEN** the resulting ScrapeRequest includes those headers
- **AND** the headers are sent with the HTTP request

#### Scenario: TLS fingerprint option
- **WHEN** ScrapeOptionsBuilder.needs_tls_fingerprint(true) is set
- **THEN** the internal ScrapeRequest has_fingerprint: true`
- **AND** the appropriate TLS `needs_tls fingerprinting is applied

### Requirement: Preserved Timeout Semantics
The system SHALL preserve original timeout durations in error conversion.

The `convert_error` function MUST pass through the actual timeout duration from the request instead of hardcoding a fixed 30s value.

#### Scenario: Custom timeout preserved
- **WHEN** a request with 60s timeout times out
- **THEN** the resulting `EngineError::Timeout` contains 60s duration
- **AND** monitoring systems see the actual timeout value

### Requirement: Configurable Engine Scoring
The system SHALL support configurable engine scoring weights and thresholds.

Engine scoring configuration MUST be loadable from config files with defaults matching current behavior. The configuration MUST include:
- Base scores per engine type
- Statistical weights (success rate, latency, error rate)
- Threshold values for health decisions

#### Scenario: Custom scoring weights
- **WHEN** engine scoring configuration sets `success_rate_weight: 0.6`
- **THEN** engine selection prioritizes success rate over other factors
- **AND** default configuration maintains current behavior

### Requirement: Routing Layer Observability
The system SHALL emit structured metrics for routing decisions and performance.

The EngineRouter MUST emit metrics for:
- Number of candidate engines per request
- Number of engines attempted
- Success/failure outcome
- Per-engine latency (histogram)
- Failure classification (timeout, SSRF, network, etc.)

#### Scenario: Metrics emission on routing
- **WHEN** a scrape request is routed
- **THEN** metrics are emitted with engine selection outcome
- **AND** per-engine latency is recorded for successful attempts

#### Scenario: Failure categorization
- **WHEN** a request fails with timeout
- **THEN** the failure is categorized as "timeout" in metrics
- **AND** different failure types are distinguishable in observability tools

## MODIFIED Requirements

### Requirement: Health Status Reporting
**MODIFIED from**: Health status is tracked globally without domain separation.
**MODIFIED to**: Health status is tracked per domain with independent state.

The SearchHealthChecker's `get_health_status` method MUST return a composite status that reflects the health of all tracked domains, not just a single global state.

#### Scenario: Multiple domains with different health
- **WHEN** domain A is healthy and domain B is unhealthy
- **THEN** `get_health_status` returns a status indicating mixed health
- **AND** the response includes per-domain breakdown

### Requirement: Circuit Breaker State Management
**MODIFIED from**: CircuitBreaker uses `std::sync::RwLock` with poison handling.
**MODIFIED to**: CircuitBreaker uses `parking_lot::RwLock` without poison handling.

All lock acquisitions MUST handle poisoning gracefully by treating a poisoned lock as unlocked, but the `parking_lot` implementation eliminates poison cases entirely.

#### Scenario: Lock acquisition after panic
- **WHEN** a thread panics while holding the lock
- **THEN** `parking_lot` lock is not poisoned
- **AND** subsequent lock acquisition succeeds
