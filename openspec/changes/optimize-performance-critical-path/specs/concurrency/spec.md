## ADDED Requirements

### Requirement: Lua Script Concurrency Control

The system SHALL execute Redis concurrency control operations atomically using a single Lua script to reduce network round-trips.

#### Scenario: Single script execution
- **WHEN** a worker attempts to acquire a concurrency permit
- **THEN** the system SHALL execute ZREMRANGEBYSCORE, GET, ZCARD, ZSCORE, and ZADD operations in a single atomic Lua script
- **AND** the system SHALL return immediately after execution completes

#### Scenario: Concurrent workers competing for permits
- **WHEN** multiple workers attempt to acquire permits simultaneously
- **THEN** the Lua script SHALL execute atomically without race conditions
- **AND** the system SHALL grant permits only when current count is below limit

### Requirement: Reduced Redis Round-Trips

The system SHALL reduce Redis network interactions from 4+ calls to 1 call per concurrency permit acquisition.

#### Scenario: Measuring network efficiency
- **WHEN** acquiring 10,000 concurrency permits
- **THEN** the system SHALL make exactly 10,000 Redis calls
- **AND** NOT 40,000+ calls (previous implementation)

#### Scenario: Graceful degradation
- **WHEN** Lua script execution fails
- **THEN** the system SHALL fall back to individual commands
- **AND** log the failure for monitoring
