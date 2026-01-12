## ADDED Requirements

### Requirement: Redis Cell Rate Limiter
The system SHALL use Redis Cell for efficient, atomic rate limiting with a single Redis call per check.

Redis Cell rate limiter SHALL provide:
- Atomic increment and check operations
- Configurable rate and burst limits
- Per-key tracking
- Response with remaining count and reset time

#### Scenario: Rate limit check succeeds
- **GIVEN** a rate limit of 100 requests per minute for key `key-123`
- **AND** the key has made 50 requests in the current window
- **WHEN** the rate limiter checks the request
- **THEN** the system SHALL allow the request
- **AND** the response SHALL include `X-RateLimit-Remaining: 49`
- **AND** the response SHALL include `X-RateLimit-Reset` with window close time

#### Scenario: Rate limit exceeded
- **GIVEN** a rate limit of 100 requests per minute for key `key-123`
- **AND** the key has made 100 requests in the current window
- **WHEN** the rate limiter checks the request
- **THEN** the system SHALL return HTTP 429 Too Many Requests
- **AND** the response SHALL include `X-RateLimit-Limit: 100`
- **AND** the response SHALL include `X-RateLimit-Remaining: 0`
- **AND** the response SHALL include `Retry-After` with seconds to reset

#### Scenario: Redis Cell atomic operation
- **GIVEN** 10 concurrent requests to the same rate-limited key
- **WHEN** all requests check the rate limit simultaneously
- **THEN** exactly 10 atomic increments SHALL occur
- **AND** exactly 10 responses SHALL be returned
- **AND** no race conditions SHALL occur in the counter

### Requirement: Sliding Window Algorithm
The system SHALL implement sliding window rate limiting to prevent boundary spikes.

The sliding window algorithm SHALL:
- Track request timestamps with second precision
- Slide the window continuously (not fixed to wall-clock)
- Weight recent requests more heavily
- Prevent the double-counting issue in fixed windows

#### Scenario: Sliding window smooths traffic
- **GIVEN** a rate limit of 10 requests per 10 seconds
- **AND** the window is at second 9 (9 requests in window)
- **WHEN** 2 new requests arrive at second 10
- **THEN** the first request SHALL be allowed (9 weighted < 10)
- **AND** the second request SHALL be allowed (weighted = 9.1)
- **AND** no request SHALL be denied due to boundary spike

#### Scenario: Window slide removes old requests
- **GIVEN** a rate limit of 10 requests per 10 seconds
- **AND** 10 requests were made at second 0
- **WHEN** 2 new requests arrive at second 10.1
- **THEN** the system SHALL consider requests from second 0.1 to 10.0
- **AND** the oldest requests SHALL be evicted from the window
- **AND** new requests SHALL be allowed based on current window

### Requirement: Multi-Dimensional Rate Limiting
The system SHALL support rate limiting across multiple dimensions simultaneously.

Supported dimensions:
- Per-API-Key: Individual key limits
- Per-Endpoint: Endpoint-specific limits
- Per-Team: Team-level aggregate limits
- Global: System-wide limits

#### Scenario: Per-key and global limits
- **GIVEN** per-key limit of 100 rpm
- **AND** global limit of 10000 rpm
- **AND** a key has made 100 requests
- **WHEN** the key makes another request
- **THEN** the system SHALL check per-key limit first
- **AND** the request SHALL be denied due to per-key limit
- **AND** the global limit SHALL not be affected

#### Scenario: Endpoint-specific limits
- **GIVEN** `/api/v1/search` has limit of 50 rpm
- **AND** `/api/v1/scrape` has limit of 20 rpm
- **WHEN** 51 requests are made to `/api/v1/search` in one minute
- **THEN** the 51st request SHALL be denied
- **AND** requests to `/api/v1/scrape` SHALL still be allowed up to 20

### Requirement: Adaptive Rate Limiting
The system SHALL dynamically adjust rate limits based on system load and health metrics.

Adaptive rate limiting SHALL:
- Monitor system latency and error rates
- Adjust limits proportionally to load
- Provide fallback to conservative limits on degradation
- Recover limits when system health improves

#### Scenario: Automatic limit reduction under load
- **GIVEN** system latency exceeds 500ms threshold
- **AND** error rate exceeds 5%
- **WHEN** adaptive adjustment is triggered
- **THEN** rate limits SHALL be reduced by 20%
- **AND** the adjustment SHALL be logged
- **AND** the new limits SHALL be communicated via headers

#### Scenario: Limit recovery
- **GIVEN** rate limits were previously reduced by 20%
- **AND** system latency is below 200ms for 5 minutes
- **AND** error rate is below 1%
- **WHEN** recovery check executes
- **THEN** limits SHALL be increased by 10%
- **AND** gradual recovery SHALL continue until baseline

### Requirement: Rate Limit Response Headers
The system SHALL include informative headers in all rate-limited responses.

Required headers:
- `X-RateLimit-Limit`: Maximum requests allowed
- `X-RateLimit-Remaining`: Requests remaining in window
- `X-RateLimit-Reset`: Unix timestamp of window reset
- `Retry-After`: Seconds to wait before retrying (on 429)

#### Scenario: Standard rate limit response
- **GIVEN** a rate-limited request
- **WHEN** the response is returned
- **THEN** the response SHALL include all four headers
- **AND** `X-RateLimit-Reset` SHALL be within the current window

#### Scenario: Retry-After header format
- **GIVEN** a rate-limited request at second 30 of a 60-second window
- **WHEN** the response is returned
- **THEN** `Retry-After` SHALL indicate seconds until reset
- **AND** `Retry-After` SHALL be approximately 30 seconds

## MODIFIED Requirements

### Requirement: Rate Limit Configuration
**MODIFIED FROM**: Rate limits are configured per-endpoint in configuration files.

**MODIFIED TO**: Rate limits are configured via API and stored in the database with support for multiple dimensions.

The system SHALL provide a comprehensive rate limit configuration system that supports:
- Database-stored rate limit rules with API management
- Multi-dimensional rate limiting (per-key, per-team, per-endpoint, global)
- Dynamic configuration updates without service restart
- Hierarchical rate limit inheritance and overrides
- Graceful propagation of configuration changes

#### Scenario: Create rate limit configuration
- **GIVEN** an authenticated admin user
- **WHEN** creating a new rate limit rule via `POST /api/v1/rate-limits`
- **THEN** the rule SHALL be validated against schema
- **AND** the rule SHALL be stored in the database
- **AND** the rule SHALL be propagated to rate limiter instances

#### Scenario: Update rate limit at runtime
- **GIVEN** an existing rate limit configuration
- **WHEN** an admin updates the limits via `PUT /api/v1/rate-limits/{id}`
- **THEN** the change SHALL take effect within 5 seconds
- **AND** existing requests SHALL use the new limits

#### Scenario: Hierarchical rate limits
- **GIVEN** a team with global limit of 1000 rpm
- **AND** an API Key within the team with limit of 100 rpm
- **WHEN** the key makes requests
- **THEN** the per-key limit of 100 rpm SHALL apply
- **AND** the global limit SHALL aggregate all key requests
