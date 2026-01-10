## MODIFIED Requirements

### Requirement: Engine Configuration Access

All engine-related configuration structs SHALL be accessible from the centralized configuration entity module.

#### Scenario: Circuit breaker config access

- **WHEN** the application needs `CircuitConfig`
- **THEN** it SHALL be importable from `crate::config::entity::engines`
- **AND** the config SHALL be movable without lifetime issues
- **AND** the config SHALL clone efficiently due to derived Clone trait

#### Scenario: Health check config access

- **WHEN** the application needs `HealthCheckConfig`
- **THEN** it SHALL be importable from `crate::config::entity::engines`
- **AND** the config SHALL support Duration fields for timing configuration
- **AND** the config SHALL be Debug-derivable for logging purposes

#### Scenario: Screenshot config access

- **WHEN** the application needs `ScreenshotConfig`
- **THEN** it SHALL be importable from `crate::config::entity::engines`
- **AND** the config SHALL support optional fields for flexible configuration
- **AND** the config SHALL be usable in request contexts without modification
