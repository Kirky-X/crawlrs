## MODIFIED Requirements

### Requirement: Configuration Entity Module

The system SHALL provide a centralized configuration entity module located at `src/config/entity/`.

This module SHALL contain all configuration structs used throughout the application, including:

- **Rate Limiting Configs**: `RateLimitConfig`, `ConcurrencyConfig`, `RateLimitingConfig`
- **Cache Configs**: `CacheStrategyConfig`, `PreheatConfig`, `LayeredCacheConfig`, `CacheType`
- **Engine Configs**: `CircuitConfig`, `HealthCheckConfig`, `ScreenshotConfig`
- **Search Configs**: `SearchEngineRouterConfig`, `SmartSearchEngineConfig`, `SearchEngineFactoryConfig`
- **Deduplication Configs**: `DeduplicationConfig`, `ContentFingerprintConfig`, `DeduplicationStrategy`, `FingerprintAlgorithm`

#### Scenario: Configuration module structure

- **WHEN** the application module structure is examined
- **THEN** a `config/entity/` directory SHALL exist containing all configuration structs
- **AND** all configuration structs SHALL have proper `Default` implementations
- **AND** all configuration structs SHALL derive `Debug`, `Clone`, and relevant serialization traits

#### Scenario: Configuration struct defaults

- **WHEN** a configuration struct is instantiated without explicit values
- **THEN** the struct SHALL use sensible default values for all fields
- **AND** default values SHALL be documented in the struct's doc comments
- **AND** the `Default` trait implementation SHALL be explicit (not derived)

### Requirement: Rate Limiting Configuration

The system SHALL provide rate limiting configuration structs with default values.

#### Scenario: Rate limit config defaults

- **WHEN** `RateLimitConfig::default()` is called
- **THEN** it SHALL return a config with:
  - `strategy`: `TokenBucket`
  - `requests_per_second`: 10
  - `requests_per_minute`: 100
  - `requests_per_hour`: 1000
  - `bucket_capacity`: Some(100)
  - `enabled`: true

#### Scenario: Concurrency config defaults

- **WHEN** `ConcurrencyConfig::default()` is called
- **THEN** it SHALL return a config with:
  - `strategy`: `DistributedSemaphore`
  - `max_concurrent_tasks`: 100
  - `max_concurrent_per_team`: 10
  - `lock_timeout_seconds`: 300
  - `enabled`: true

### Requirement: Cache Configuration

The system SHALL provide cache configuration structs with default values.

#### Scenario: Cache strategy config defaults

- **WHEN** `CacheStrategyConfig::default()` is called
- **THEN** it SHALL return a config with:
  - `cache_type`: `CacheType::Memory`
  - `ttl_seconds`: 300
  - `max_entries`: 10000
  - `enable_compression`: true
  - `enable_preload`: false
  - `preheat_config`: None
  - `layered_config`: None

### Requirement: Engine Configuration

The system SHALL provide engine configuration structs with default values.

#### Scenario: Circuit config defaults

- **WHEN** `CircuitConfig::default()` is called
- **THEN** it SHALL return a config with:
  - `failure_threshold`: 5
  - `recovery_timeout`: 30 seconds
  - `failure_window`: 60 seconds

#### Scenario: Health check config defaults

- **WHEN** `HealthCheckConfig::default()` is called
- **THEN** it SHALL return a config with:
  - `check_interval`: 60 seconds
  - `timeout`: 10 seconds
  - `max_consecutive_failures`: 3
  - `degraded_threshold_ms`: 2000
  - `unhealthy_threshold_ms`: 5000
  - `target_url`: "https://www.google.com"

#### Scenario: Screenshot config defaults

- **WHEN** `ScreenshotConfig::default()` is called
- **THEN** it SHALL return a config with:
  - `full_page`: true
  - `selector`: None
  - `quality`: None
  - `format`: Some("png".to_string())

### Requirement: Search Configuration

The system SHALL provide search engine configuration structs with default values.

#### Scenario: Search engine router config defaults

- **WHEN** `SearchEngineRouterConfig::default()` is called
- **THEN** it SHALL return a config with:
  - `max_retries`: 3
  - `request_timeout`: 30 seconds
  - `health_check_interval`: 60 seconds
  - `unhealthy_recovery_time`: 300 seconds
  - `enable_auto_failover`: true
  - `enable_load_balancing`: true

#### Scenario: Smart search engine config defaults

- **WHEN** `SmartSearchEngineConfig::default()` is called
- **THEN** it SHALL return a config with:
  - `engine_type`: `SearchEngineType::Google`
  - `rate_limiting_enabled`: true
  - `timeout_seconds`: 90
  - `test_data_enabled`: false
  - `max_retries`: 3
  - `retry_delay_ms`: 1000

#### Scenario: Search engine factory config defaults

- **WHEN** `SearchEngineFactoryConfig::default()` is called
- **THEN** it SHALL return a config with:
  - `default_engine`: `SearchEngineType::Smart`
  - `enable_auto_failover`: true
  - `enable_load_balancing`: true
  - `request_timeout`: 30 seconds
  - `max_retries`: 3

### Requirement: Deduplication Configuration

The system SHALL provide deduplication configuration structs with default values.

#### Scenario: Deduplication config defaults

- **WHEN** `DeduplicationConfig::default()` is called
- **THEN** it SHALL return a config with:
  - `strategy`: `DeduplicationStrategy::Smart`
  - `title_similarity_threshold`: 0.85
  - `content_similarity_threshold`: 0.8
  - `fingerprint_config`: `ContentFingerprintConfig::default()`
  - `case_sensitive`: false
  - `ignore_query_params`: true
  - `ignore_fragments`: true

#### Scenario: Content fingerprint config defaults

- **WHEN** `ContentFingerprintConfig::default()` is called
- **THEN** it SHALL return a config with:
  - `enabled`: true
  - `algorithm`: `FingerprintAlgorithm::SimHash`
  - `fingerprint_size`: 64
