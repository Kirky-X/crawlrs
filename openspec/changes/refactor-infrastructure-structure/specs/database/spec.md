## ADDED Requirements

### Requirement: Database Infrastructure Consolidation

All database-related infrastructure code SHALL be consolidated under `src/infrastructure/database/` module.

#### Scenario: Repository Implementation Location
- **WHEN** accessing database repository implementations
- **THEN** they SHALL be located at `src/infrastructure/database/repositories/`

#### Scenario: Database Module Structure
- **WHEN** examining the database module
- **THEN** it SHALL contain: `connection.rs`, `entities/`, and `repositories/`

#### Scenario: Import Path for Repositories
- **WHEN** importing repository implementations
- **THEN** the import path SHALL be `crawlrs::infrastructure::database::repositories::*`

### Requirement: Database Module Organization

The database module SHALL provide a single, cohesive interface for all database operations.

#### Scenario: Connection Pool Creation
- **WHEN** creating a database connection pool
- **THEN** it SHALL be accessed via `crawlrs::infrastructure::database::connection::create_pool()`

#### Scenario: Entity Definitions
- **WHEN** using database entity models
- **THEN** they SHALL be located at `src/infrastructure/database/entities/`

#### Scenario: Repository Access
- **WHEN** accessing repository implementations
- **THEN** they SHALL be imported from `crawlrs::infrastructure::database::repositories::*`