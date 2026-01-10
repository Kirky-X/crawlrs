## MODIFIED Requirements

### Requirement: Infrastructure Configuration Access

All infrastructure-related configuration structs SHALL be accessible from the centralized configuration entity module.

#### Scenario: Rate limiting config access

- **WHEN** the application needs rate limiting configurations
- **THEN** `RateLimitingConfig` SHALL be importable from `crate::config::entity::rate_limiting`
- **AND** `RateLimitConfig` SHALL be importable from the same module
- **AND** `ConcurrencyConfig` SHALL be importable from the same module
- **AND** all configs SHALL have Default implementations for easy testing

#### Scenario: Cache config access

- **WHEN** the application needs cache configurations
- **THEN** `CacheStrategyConfig` SHALL be importable from `crate::config::entity::cache`
- **AND** `PreheatConfig` SHALL be importable from the same module
- **AND** `LayeredCacheConfig` SHALL be importable from the same module
- **AND** `CacheType` enum SHALL be importable from the same module
- **AND** all configs SHALL support serialization for configuration file persistence

#### Scenario: Search engine config access

- **WHEN** the application needs search engine configurations
- **THEN** `SearchEngineRouterConfig` SHALL be importable from `crate::config::entity::search`
- **AND** `SmartSearchEngineConfig` SHALL be importable from the same module
- **AND** `SearchEngineFactoryConfig` SHALL be importable from the same module
- **AND** all configs SHALL have reasonable production-ready defaults

#### Scenario: Deduplication config access

- **WHEN** the application needs deduplication configurations
- **THEN** `DeduplicationConfig` SHALL be importable from `crate::config::entity::deduplication`
- **AND** `ContentFingerprintConfig` SHALL be importable from the same module
- **AND** `DeduplicationStrategy` enum SHALL be importable from the same module
- **AND** `FingerprintAlgorithm` enum SHALL be importable from the same module
