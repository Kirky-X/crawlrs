## ADDED Requirements

### Requirement: Cache Key Naming Convention
The system SHALL enforce a consistent cache key naming convention for all cached data.

The cache key format SHALL be:
```
{namespace}:{layer}:{entity}:{identifier}[:{extra}]
```

Key components:
- `namespace`: Business domain (search, scrape, auth, credits)
- `layer`: Cache tier (hot, warm, cold)
- `entity`: Entity type (result, config, key, balance)
- `identifier`: Unique identifier (UUID, hash, or key)
- `extra`: Optional qualifier (page, variant)

#### Scenario: Search result cache key
- **GIVEN** a search query "rust tutorial"
- **WHEN** caching the result
- **THEN** the key SHALL be `search:warm:result:sha256(query):lang:en`
- **AND** the hash SHALL be consistent for identical queries

#### Scenario: API Key cache key
- **GIVEN** an API Key with ID `key-uuid-123`
- **WHEN** caching the key information
- **THEN** the key SHALL be `auth:hot:key:key-uuid-123`
- **AND** the layer `hot` indicates frequent access

#### Scenario: Cache key namespace collision prevention
- **GIVEN** multiple applications sharing Redis
- **WHEN** storing cache entries
- **THEN** the namespace SHALL include app prefix `crawlrs:`
- **AND** the full key SHALL be `crawlrs:search:warm:result:...`

### Requirement: Cache TTL Layer Strategy
The system SHALL implement a tiered TTL strategy based on data volatility and access patterns.

TTL Layers:
| Layer | TTL Range | Use Case | Refresh Strategy |
|-------|-----------|----------|------------------|
| hot | 1-5 minutes | API Keys, Session | Active invalidation |
| warm | 5-30 minutes | Search results | TTL eviction |
| cold | 30min-2 hours | Aggregated data | TTL eviction |

#### Scenario: Hot layer for frequently accessed data
- **GIVEN** an API Key accessed every 30 seconds
- **WHEN** the key is cached in hot layer
- **THEN** the TTL SHALL be 2 minutes
- **AND** refresh requests SHALL extend TTL
- **AND** stale reads SHALL be prevented

#### Scenario: Warm layer for search results
- **GIVEN** search results that change infrequently
- **WHEN** caching search results
- **THEN** the TTL SHALL be 10 minutes
- **AND** the system SHALL return stale data if cache miss
- **AND** async refresh SHALL update cache after return

#### Scenario: Cold layer for aggregated data
- **GIVEN** usage statistics updated hourly
- **WHEN** caching the statistics
- **THEN** the TTL SHALL be 1 hour
- **AND** the cache SHALL be lazily refreshed
- **AND** stale data SHALL be acceptable

### Requirement: Cache Invalidation Strategy
The system SHALL support multiple cache invalidation strategies.

Supported strategies:
- **Active Invalidation**: Immediate deletion on data change
- **TTL Eviction**: Automatic deletion after TTL expires
- **Capacity Eviction**: LRU/LFU when capacity reached
- **Version Invalidation**: Invalidate by data version

#### Scenario: Active invalidation on data change
- **GIVEN** an API Key cached in hot layer
- **WHEN** the key's scopes are updated
- **THEN** the system SHALL immediately delete the cached key
- **AND** subsequent reads SHALL fetch fresh data
- **AND** the invalidation SHALL be logged

#### Scenario: TTL-based eviction
- **GIVEN** a cache entry with TTL of 10 minutes
- **WHEN** 10 minutes elapse without access
- **THEN** the entry SHALL be automatically evicted
- **AND** the memory SHALL be reclaimed

#### Scenario: Capacity-based eviction
- **GIVEN** the cache is at 90% capacity
- **WHEN** a new entry needs to be stored
- **THEN** the system SHALL evict least recently used entries
- **AND** at least 20% capacity SHALL be freed
- **AND** eviction SHALL be logged

### Requirement: Cache Fallback Strategy
The system SHALL implement fallback strategies when cache is unavailable or degraded.

Fallback levels:
- **Layer Fallback**: Hot → Warm → Cold → Direct
- **Degradation**: Cached data only → Stale allowed → Direct

#### Scenario: Hot layer unavailable
- **GIVEN** the hot layer (memory cache) is unavailable
- **WHEN** a cache request is made
- **THEN** the system SHALL fallback to warm layer
- **AND** the response SHALL indicate cache tier used
- **AND** the hot layer SHALL be logged as unavailable

#### Scenario: All cache layers unavailable
- **GIVEN** all cache layers are unavailable
- **WHEN** a cache request is made
- **THEN** the system SHALL fall back to direct database query
- **AND** the request SHALL still succeed
- **AND** the cache unavailability SHALL be logged
- **AND** metrics SHALL be emitted

#### Scenario: Stale data fallback
- **GIVEN** cache returns stale data (TTL exceeded)
- **AND** stale allowed mode is enabled
- **WHEN** direct database query fails
- **THEN** the system SHALL return stale cache data
- **AND** the response SHALL include `X-Cache-Stale: true`
- **AND** async refresh SHALL be triggered

### Requirement: Cache Preloading
The system SHALL support proactive cache preloading for predictable high-traffic data.

Preloading capabilities:
- Scheduled preloading
- Event-driven preloading
- Predictive preloading based on patterns

#### Scenario: Scheduled cache preloading
- **GIVEN** a scheduled job configured to run at 6 AM
- **WHEN** the scheduled time arrives
- **THEN** the system SHALL preload hot data
- **AND** the preload SHALL complete before 6:05 AM
- **AND** the preload completion SHALL be logged

#### Scenario: Event-driven preloading
- **GIVEN** a new API Key is created
- **WHEN** the creation event is published
- **THEN** the system SHALL preload the key in hot cache
- **AND** subsequent reads SHALL hit the hot cache

### Requirement: Cache Metrics and Monitoring
The system SHALL expose cache metrics for observability.

Required metrics:
- Hit rate (percentage)
- Miss rate (percentage)
- Eviction count
- Error rate
- Latency (p50, p95, p99)
- Memory usage
- Capacity utilization

#### Scenario: Cache hit rate monitoring
- **GIVEN** cache requests over a 1-minute window
- **WHEN** calculating metrics
- **THEN** the hit rate SHALL be calculated as hits / total requests
- **AND** the metric SHALL be exposed for Prometheus
- **AND** the metric SHALL be updated every 10 seconds

#### Scenario: Cache eviction monitoring
- **GIVEN** cache eviction events
- **WHEN** monitoring the system
- **THEN** evictions SHALL be counted by layer
- **AND** evictions SHALL be counted by reason (TTL, capacity)
- **AND** high eviction rates SHALL trigger alerts

## MODIFIED Requirements

### Requirement: Cache Key Format Standardization
**MODIFIED FROM**: Cache keys are created ad-hoc with inconsistent formats.

**MODIFIED TO**: All cache keys MUST follow the standardized naming convention.

The system SHALL enforce a consistent cache key format across all caching operations:
- Use the standardized format: `{namespace}:{layer}:{entity}:{identifier}[:{extra}]`
- Include application prefix to prevent namespace collisions
- Ensure hash consistency for query-based keys
- Validate all cache keys against the naming convention
- Provide migration path for legacy cache entries

#### Scenario: Existing cache migration
- **GIVEN** legacy cache entries with old key format
- **WHEN** the system encounters legacy keys
- **THEN** it SHALL serve the legacy data
- **AND** it SHALL write new data with new format
- **AND** legacy keys SHALL be migrated within 30 days

#### Scenario: New cache entry validation
- **GIVEN** a new cache implementation
- **WHEN** storing data with a cache key
- **THEN** the key SHALL be validated against the naming convention
- **AND** invalid keys SHALL be rejected at compile time (for code) or logged (for generated keys)

#### Scenario: Cache key collision prevention
- **GIVEN** multiple applications sharing Redis
- **WHEN** storing cache entries
- **THEN** the namespace SHALL include app prefix `crawlrs:`
- **AND** the full key SHALL be `crawlrs:search:warm:result:...`
- **AND** no collision SHALL occur with other applications

### Requirement: Cache TTL Configuration
**MODIFIED FROM**: TTL is hardcoded per cache type.

**MODIFIED TO**: TTL is configured with defaults that can be overridden per use case.

The system SHALL provide a flexible TTL configuration system:
- Define default TTL values for each cache layer (hot, warm, cold)
- Allow per-use-case TTL overrides through configuration
- Validate TTL values to ensure they are positive
- Apply TTL during cache entry creation
- Support TTL refresh and extension on access

#### Scenario: Configuration override
- **GIVEN** a cache entry type with default TTL of 10 minutes
- **AND** a specific use case requires 5-minute TTL
- **WHEN** the cache is configured for this use case
- **THEN** the specific TTL SHALL override the default
- **AND** the configuration SHALL be validated against schema

#### Scenario: TTL validation
- **GIVEN** a TTL configuration value of 0
- **WHEN** the configuration is validated
- **THEN** the validation SHALL fail
- **AND** TTL MUST be greater than 0

#### Scenario: TTL refresh on access
- **GIVEN** a cache entry with TTL of 10 minutes
- **AND** the entry was accessed at minute 8
- **WHEN** the access occurs
- **THEN** the TTL SHALL be extended to 10 minutes from access time
- **AND** the extended TTL SHALL not exceed maximum configured value
