## ADDED Requirements

### Requirement: Async Metrics Collection

The system SHALL collect CPU and memory metrics in a background thread and store results in atomic variables for lock-free reads.

#### Scenario: Background collection
- **WHEN** the metrics collector is started
- **THEN** it SHALL spawn a background thread that updates metrics every 1 second
- **AND** store values in `AtomicU64` variables for thread-safe reads

#### Scenario: Hot path reads from memory
- **WHEN** the crawl service checks system load
- **THEN** it SHALL read from atomic variables instead of reading /proc/stat
- **AND** the read operation SHALL NOT block

### Requirement: Metrics Freshness Guarantee

The system SHALL ensure metrics are no more than 2 seconds stale during normal operation.

#### Scenario: Detecting stale metrics
- **WHEN** metrics have not been updated for more than 2 seconds
- **THEN** the system SHALL log a warning
- **AND** continue using the last known values

#### Scenario: Collector failure recovery
- **WHEN** the background collector thread panics
- **THEN** the system SHALL attempt to restart the collector
- **AND** log the restart event
