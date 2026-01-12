## ADDED Requirements

### Requirement: Robots.txt Cache with TTL

The system SHALL cache robots.txt rules per domain with a configurable TTL (default: 1 hour).

#### Scenario: Cache miss
- **WHEN** a domain's robots.txt is not in cache
- **THEN** the system SHALL fetch the robots.txt from the source
- **AND** store the result in cache with 1-hour TTL

#### Scenario: Cache hit
- **WHEN** a domain's robots.txt is in cache and not expired
- **THEN** the system SHALL return the cached rules
- **AND** NOT make a network request

#### Scenario: Cache expiration
- **WHEN** cached robots.txt exceeds TTL
- **THEN** the next request SHALL trigger a fresh fetch
- **AND** update the cache with new rules

### Requirement: Lazy Robots Check

The system SHALL defer robots.txt checking to worker execution time rather than URL discovery time.

#### Scenario: URL discovery without robots check
- **WHEN** discovering new URLs during crawl
- **THEN** the system SHALL only perform deduplication
- **AND** NOT check robots.txt rules at this stage

#### Scenario: Robots check at execution time
- **WHEN** a worker picks up a task for execution
- **THEN** the system SHALL check robots.txt rules before scraping
- **AND** skip the task if robots.txt disallows the URL

#### Scenario: Disallowed URL handling
- **WHEN** robots.txt rules indicate the URL is disallowed
- **THEN** the system SHALL mark the task as skipped
- **AND** log the robots.txt denial
