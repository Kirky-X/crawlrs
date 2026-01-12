## ADDED Requirements

### Requirement: Regex Pattern Compilation Caching

The system SHALL cache compiled regex patterns using `OnceLock` to avoid repeated compilation overhead.

#### Scenario: Shared pattern caching
- **WHEN** a shared regex pattern is first used
- **THEN** the system SHALL compile it once and store in `OnceLock`
- **AND** subsequent uses SHALL retrieve the compiled pattern

#### Scenario: Hot path performance
- **WHEN** processing 10,000 URLs with the same pattern
- **THEN** the system SHALL compile the pattern exactly once
- **AND** NOT 10,000 times

### Requirement: Task-Specific Pattern Pre-compilation

The system SHALL pre-compile regex patterns specific to a crawl task during initialization.

#### Scenario: Crawl initialization
- **WHEN** a crawl task is initialized
- **THEN** the system SHALL compile all regex patterns defined in the crawl config
- **AND** store them for reuse throughout the crawl

#### Scenario: Pattern reuse within crawl
- **WHEN** processing multiple URLs in the same crawl
- **THEN** the system SHALL use the pre-compiled patterns
- **AND** NOT recompile patterns between URLs

### Requirement: Selector Compilation Caching

The system SHALL cache CSS/XPath selectors used for content extraction.

#### Scenario: Repeated selector usage
- **WHEN** the same selector is used multiple times
- **THEN** the system SHALL cache the compiled selector
- **AND** reuse it on subsequent extractions
