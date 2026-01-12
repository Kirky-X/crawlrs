## ADDED Requirements

### Requirement: API Key Scopes
The system SHALL support fine-grained API Key permissions through a scope-based model.

The API Key scope model SHALL include:
- `read`: Permission to access read-only endpoints (search, scrape GET)
- `write`: Permission to access write endpoints (config, upload)
- `admin`: Permission to access administrative endpoints (team, billing)
- `search_limit`: Maximum number of search requests per hour
- `scrape_limit`: Maximum number of scrape requests per hour

#### Scenario: API Key with read-only scope
- **GIVEN** an API Key with `read=true`, `write=false`, `admin=false`
- **WHEN** the user requests a write endpoint
- **THEN** the system SHALL return HTTP 403 Forbidden
- **AND** the response SHALL include error code `SCOPE_FORBIDDEN`

#### Scenario: API Key with custom request limits
- **GIVEN** an API Key with `search_limit=100`, `scrape_limit=50`
- **WHEN** the user makes 101 search requests within one hour
- **THEN** the system SHALL return HTTP 429 Too Many Requests
- **AND** the response SHALL include error code `QUOTA_EXCEEDED`

#### Scenario: Scope inheritance from team to API Key
- **GIVEN** a team with default scope `read=true, write=true`
- **AND** an API Key with no explicit scopes set
- **WHEN** the API Key is used for authorization
- **THEN** the system SHALL inherit the team's default scopes
- **AND** the inherited scopes SHALL be logged in audit

### Requirement: Feature Flags
The system SHALL support feature flags to enable/disable functionality at runtime.

Feature flags SHALL support:
- Global toggle for system-wide features
- Per-API-Key enable/disable
- Percentage-based rollout (canary releases)
- Time-based activation/deactivation

#### Scenario: Feature flag disabled globally
- **GIVEN** a feature flag `new_search_v2` with `enabled=false`
- **WHEN** any user requests the new search endpoint
- **THEN** the system SHALL return HTTP 404 Not Found
- **AND** the response SHALL include error code `FEATURE_DISABLED`

#### Scenario: Feature flag enabled for specific API Key
- **GIVEN** a feature flag `beta_feature` with `enabled=true` for key `key-123`
- **AND** the feature flag is `enabled=false` for other keys
- **WHEN** key `key-123` requests the beta feature
- **THEN** the system SHALL allow access
- **WHEN** other keys request the beta feature
- **THEN** the system SHALL return HTTP 404 Not Found

#### Scenario: Percentage-based rollout
- **GIVEN** a feature flag with `rollout_percentage=25`
- **WHEN** 100 unique API Keys request the feature
- **THEN** approximately 25 keys SHALL receive access
- **AND** the distribution SHALL be deterministic per key

### Requirement: Enhanced Audit Logging
The system SHALL maintain comprehensive audit logs for all authentication and authorization decisions.

The audit log SHALL record:
- Timestamp (ISO 8601 with millisecond precision)
- API Key ID (anonymized for privacy)
- Team ID
- Requested action
- Decision (allow/deny)
- Reason for denial (if applicable)
- Scope used for authorization
- IP address (if available)
- Request trace ID

#### Scenario: Successful authentication
- **GIVEN** a valid API Key with scopes `read=true, write=true`
- **WHEN** the user makes an authorized request
- **THEN** the system SHALL log an audit entry with decision `ALLOW`
- **AND** the log SHALL include the scopes used

#### Scenario: Failed authorization
- **GIVEN** an API Key with `write=false`
- **WHEN** the user requests a write endpoint
- **THEN** the system SHALL log an audit entry with decision `DENY`
- **AND** the log SHALL include reason `SCOPE_FORBIDDEN`
- **AND** the log SHALL include the required scope `write`

#### Scenario: Audit log retention
- **GIVEN** audit log entries older than 90 days
- **WHEN** the retention period expires
- **THEN** the system SHALL archive entries to cold storage
- **AND** the original entries SHALL be deleted from the primary store

### Requirement: Scope Validation Middleware
The system SHALL validate API Key scopes at the middleware level before request processing.

The middleware SHALL:
- Extract API Key from Authorization header
- Load associated scopes from cache or database
- Validate required scopes for the requested endpoint
- Short-circuit with 403 if scope validation fails
- Include scope information in request context

#### Scenario: Middleware passes valid scope
- **GIVEN** an API Key with scope `read=true`
- **AND** a GET request to `/api/v1/search`
- **WHEN** the request passes through the auth middleware
- **THEN** the middleware SHALL allow the request
- **AND** the request context SHALL include `scopes: {read: true}`

#### Scenario: Middleware blocks missing scope
- **GIVEN** an API Key with scope `write=false`
- **AND** a POST request to `/api/v1/config`
- **WHEN** the request passes through the auth middleware
- **THEN** the middleware SHALL return HTTP 403
- **AND** the response body SHALL include scope requirements

## MODIFIED Requirements

### Requirement: API Key Authentication
**MODIFIED FROM**: The system SHALL authenticate API Keys at the team level, accepting any valid key within a team.

**MODIFIED TO**: The system SHALL authenticate API Keys at both team and individual key levels, supporting scope-based permissions.

The system SHALL validate API Keys through a multi-level authentication process:
- Validate key format and structure
- Verify key exists and is active in the database
- Load associated scopes and permissions
- Validate scope requirements for the requested endpoint
- Store authentication context for downstream use

#### Scenario: API Key authentication with scopes
- **GIVEN** a valid API Key with scopes defined
- **WHEN** authentication is performed
- **THEN** the system SHALL validate the key exists and is active
- **AND** the system SHALL load the key's associated scopes
- **AND** the scopes SHALL be stored in the request context

#### Scenario: Inactive API Key
- **GIVEN** an API Key marked as inactive
- **WHEN** authentication is attempted
- **THEN** the system SHALL return HTTP 401 Unauthorized
- **AND** the response SHALL include error code `KEY_INACTIVE`
- **AND** no scope information SHALL be loaded
