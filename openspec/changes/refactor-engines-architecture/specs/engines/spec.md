## ADDED Requirements

### Requirement: EngineClient Public API

The system SHALL provide a single public entry point `EngineClient` for all scraping operations.

The `EngineClient` SHALL:
- Expose only `scrape()` and `health_check()` methods
- Hide all internal implementation details (UA rotation, circuit breaker, engine selection)
- Accept a unified `ScrapeRequest` structure
- Return a unified `ScrapeResponse` structure
- Automatically select the optimal engine based on request requirements

#### Scenario: Basic scraping request
- **WHEN** caller invokes `EngineClient::scrape()` with a valid URL
- **THEN** the client SHALL automatically select an appropriate engine
- **AND** SHALL perform the request with automatic retries
- **AND** SHALL handle UA rotation transparently
- **AND** SHALL return a `ScrapeResponse` with content and metadata

#### Scenario: JavaScript rendering request
- **WHEN** caller sets `options.needs_js = true`
- **THEN** the client SHALL select a JavaScript-capable engine (Playwright or Fire CDP)
- **AND** SHALL wait for page load before returning

#### Scenario: Screenshot request
- **WHEN** caller sets `options.needs_screenshot = true`
- **THEN** the client SHALL select an engine that supports screenshots
- **AND** SHALL return base64-encoded screenshot in response

#### Scenario: Health check
- **WHEN** caller invokes `EngineClient::health_check()`
- **THEN** the client SHALL return aggregate health status
- **AND** SHALL NOT expose internal circuit breaker state

---

### Requirement: Unified ScrapeRequest Structure

The system SHALL provide a unified `ScrapeRequest` structure that encapsulates all scraping parameters.

The `ScrapeRequest` SHALL contain:
- `url: String` - Required target URL
- `options: ScrapeOptions` - Optional configuration

The `ScrapeOptions` SHALL support:
- `needs_js: bool` - Whether JavaScript rendering is required (default: false)
- `needs_screenshot: bool` - Whether screenshot is required (default: false)
- `mobile: bool` - Whether to use mobile user agent (default: false)
- `timeout: Duration` - Request timeout (default: 30 seconds)

#### Scenario: Request with all options
- **WHEN** caller creates `ScrapeRequest` with all options specified
- **THEN** the client SHALL respect all provided options
- **AND** SHALL use appropriate engine based on requirements

#### Scenario: Request with defaults
- **WHEN** caller creates `ScrapeRequest` with only URL
- **THEN** the client SHALL use default options (no JS, no screenshot, desktop UA, 30s timeout)

---

### Requirement: Unified ScrapeResponse Structure

The system SHALL provide a unified `ScrapeResponse` structure that contains all scraping results.

The `ScrapeResponse` SHALL contain:
- `status_code: u16` - HTTP status code
- `content: String` - Page content (HTML or extracted text)
- `screenshot: Option<String>` - Base64-encoded screenshot (if requested)
- `content_type: String` - Response content type
- `headers: HashMap<String, String>` - Response headers
- `response_time_ms: u64` - Time taken to complete request

#### Scenario: Response with content
- **WHEN** scraping completes successfully
- **THEN** the response SHALL contain the page content
- **AND** SHALL include HTTP status code
- **AND** SHALL include response headers

#### Scenario: Response with screenshot
- **WHEN** screenshot was requested and succeeded
- **THEN** the response SHALL contain base64-encoded image data
- **AND** the screenshot SHALL match requested format and quality

---

### Requirement: Health Check API

The system SHALL expose health check functionality through `EngineClient`.

The `health_check()` method SHALL return an `EngineHealthStatus` that indicates:
- `Healthy` - All engines operational
- `Degraded` - Some engines unavailable, fallback in use
- `Unavailable` - No engines available

#### Scenario: All engines healthy
- **WHEN** all underlying engines are operational
- **THEN** `health_check()` SHALL return `Healthy`

#### Scenario: Partial degradation
- **WHEN** some engines are unavailable but others are operational
- **THEN** `health_check()` SHALL return `Degraded` with list of failed engines

#### Scenario: Complete failure
- **WHEN** all engines are unavailable
- **THEN** `health_check()` SHALL return `Unavailable`

---

### Requirement: Internal Implementation Encapsulation

The system SHALL hide all internal implementation details from the public API.

The following SHALL NOT be accessible from the public API:
- User-Agent rotation logic and state
- Circuit breaker state and configuration
- Engine selection algorithm and `support_score()`
- Retry logic and backoff configuration
- Direct access to `ScraperEngine` trait

#### Scenario: No direct engine access
- **WHEN** caller attempts to access engine implementations directly
- **THEN** the code SHALL not compile due to visibility restrictions

#### Scenario: Automatic engine selection
- **WHEN** caller makes a scraping request
- **THEN** the engine selection SHALL be performed automatically
- **AND** the caller SHALL NOT be able to override engine selection

---

### Requirement: Error Handling

The system SHALL provide consistent error handling through `EngineClient`.

The `EngineError` enum SHALL contain:
- `RequestFailed(String)` - General request failure with message
- `Timeout(Duration)` - Request timeout with duration
- `NoEnginesAvailable` - All engines failed or unavailable
- `InvalidUrl(String)` - URL validation failed
- `SsrfProtection(String)` - SSRF protection triggered

#### Scenario: Request timeout
- **WHEN** request exceeds configured timeout
- **THEN** the client SHALL return `EngineError::Timeout`

#### Scenario: All engines failed
- **WHEN** all engines fail to process the request
- **THEN** the client SHALL return `EngineError::NoEnginesAvailable`

#### Scenario: Invalid URL
- **WHEN** provided URL fails validation
- **THEN** the client SHALL return `EngineError::InvalidUrl`

---

## MODIFIED Requirements

### Requirement: ScraperEngine Visibility

**MODIFIED FROM**: The `ScraperEngine` trait was publicly accessible and could be used directly.

**MODIFIED TO**: The `ScraperEngine` trait SHALL be hidden as an internal implementation detail.

The `ScraperEngine` trait SHALL:
- Be marked as `pub(crate)` visibility
- Not be re-exported from the engines module public API
- Only be used internally by `EngineClient`

#### Scenario: Trait not exported
- **WHEN** external code attempts to import `ScraperEngine`
- **THEN** the import SHALL fail to compile

---

## REMOVED Requirements

### Requirement: Direct Engine Access

**REASON**: Direct engine access violates the encapsulation principle and allows bypassing smart routing.

**MIGRATION**: Use `EngineClient::scrape()` instead of direct engine calls.

**REMOVED**: The ability to:
- Create engine instances directly
- Call `support_score()` to determine engine selection
- Query circuit breaker state
- Configure UA rotation manually

#### Scenario: No direct engine creation
- **WHEN** caller attempts to create `ReqwestEngine` or other engine directly
- **THEN** the code SHALL not compile (constructors are private)

#### Scenario: No engine selection override
- **WHEN** caller attempts to force a specific engine
- **THEN** the API SHALL not provide such capability
