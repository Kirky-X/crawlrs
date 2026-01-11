## Context

The infrastructure layer currently has two organizational issues:

1. **Database Code Scattered**: Repository implementations live in `src/infrastructure/repositories/` while connection and entities are in `src/infrastructure/database/`. This separation is confusing because all three components (connection, entities, repositories) are part of the database infrastructure.

2. **Queue at Root Level**: The `src/queue/` module contains infrastructure code (task queue implementation) but is placed at the root level, making it unclear it's part of the infrastructure layer.

3. **Queue API Leaks Abstraction**: The `TaskQueue` trait and `TaskScheduler` are public, allowing consumers to bypass the `QueueClient` API. This creates inconsistent usage patterns and makes it harder to maintain queue behavior in one place.

**Current Structure:**
```
src/
├── infrastructure/
│   ├── database/
│   │   ├── connection.rs
│   │   └── entities/
│   └── repositories/  ← Should be under database/
├── queue/             ← Should be under infrastructure/
└── workers/           ← Uses queue
```

**Target Structure:**
```
src/
├── infrastructure/
│   ├── database/
│   │   ├── connection.rs
│   │   ├── entities/
│   │   └── repositories/  ← Moved here
│   └── queue/            ← Moved here
└── workers/             ← Uses queue
```

## Goals / Non-Goals

### Goals
1. Consolidate all database infrastructure under `src/infrastructure/database/`
2. Move queue infrastructure to `src/infrastructure/queue/`
3. Make `QueueClient` the only public queue API
4. Improve code organization and discoverability
5. Maintain backward compatibility through proper imports

### Non-Goals
- Change the behavior of queue operations
- Change the behavior of database operations
- Modify the API of `QueueClient`
- Add new features to queue or database

## Decisions

### 1. Database Consolidation

**Decision**: Move all repository implementations to `src/infrastructure/database/repositories/`

**Rationale**:
- Repositories are database access layer, logically belong with database code
- Single location for all database-related infrastructure
- Easier navigation and maintenance

**New Module Path**:
```rust
// Old
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;

// New
use crawlrs::infrastructure::database::repositories::task_repo_impl::TaskRepositoryImpl;
```

### 2. Queue Module Location

**Decision**: Move `src/queue/` to `src/infrastructure/queue/`

**Rationale**:
- Queue is infrastructure code, belongs in infrastructure layer
- Consistent with other infrastructure modules (cache, storage, services)
- Clear separation of concerns

**New Module Path**:
```rust
// Old
use crawlrs::queue::{QueueClient, QueueClientBuilder};

// New
use crawlrs::infrastructure::queue::{QueueClient, QueueClientBuilder};
```

### 3. Queue API Restriction

**Decision**: Make `TaskQueue` trait and `TaskScheduler` private, expose only `QueueClient`

**Rationale**:
- `QueueClient` already provides a unified, high-level API
- Prevents bypassing the client and using low-level operations directly
- Centralizes queue behavior (metrics, error handling, timeouts)
- Easier to maintain and evolve queue implementation

**Public API**:
```rust
// src/infrastructure/queue/mod.rs
pub use self::client::{
    QueueClient,
    QueueClientBuilder,
    EnqueueRequest,
    DequeueRequest,
    StatusUpdateRequest,
    // ... other client types
};

// Private - not exported
mod task_queue;   // TaskQueue trait is private
mod scheduler;    // TaskScheduler is private
```

**Migration Path**:
```rust
// Old (direct use - now disallowed)
let task = queue.dequeue(worker_id).await?;

// New (use QueueClient)
let task = client.dequeue(DequeueRequest::new(worker_id)).await?;
```

### 4. Module Declarations

**Decision**: Update `mod.rs` files to reflect new structure

**src/infrastructure/database/mod.rs**:
```rust
pub mod connection;
pub mod entities;
pub mod repositories;  // Added
```

**src/infrastructure/mod.rs**:
```rust
pub mod database;
pub mod queue;  // Added
pub mod cache;
pub mod services;
// ... other modules
```

**src/lib.rs**:
```rust
pub mod infrastructure;
// Remove: pub mod queue;
```

## Risks / Trade-offs

| Risk | Impact | Mitigation |
|------|--------|------------|
| Breaking imports | High | Systematic update of all imports using search/replace |
| Queue API restriction | Medium | Ensure `QueueClient` has all necessary functionality |
| Documentation out of date | Low | Update all examples and docs |
| Test failures | Medium | Update test imports and run full test suite |

## Migration Plan

1. **Phase 1**: Create new directory structure (30 minutes)
2. **Phase 2**: Move files to new locations (15 minutes)
3. **Phase 3**: Update module declarations (15 minutes)
4. **Phase 4**: Update all imports (1-2 hours)
5. **Phase 5**: Make queue API private (30 minutes)
6. **Phase 6**: Verify compilation and tests (30 minutes)
7. **Phase 7**: Update documentation (30 minutes)

**Total Estimated Time**: 4-5 hours

## Open Questions

1. Should we create a migration script for imports, or do it manually? no.不需要考虑向后兼容。
2. Are there any external dependencies (e.g., in examples) that need updating? no.
3. Should we add deprecation warnings before making `TaskQueue` private? (Decided: No, internal-only usage)