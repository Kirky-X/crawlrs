# Change: Refactor Infrastructure - Consolidate Database and Queue Modules

## Why

The current infrastructure module structure has organizational issues:

1. **Scattered Database Code**: `src/infrastructure/repositories` contains database access logic but is separate from `src/infrastructure/database`, creating confusion about where database-related code belongs.

2. **Queue Module at Root Level**: `src/queue` is a top-level module despite being infrastructure code, making it unclear it's part of the infrastructure layer.

3. **Leaky Abstractions in Queue**: `TaskQueue` trait and `TaskScheduler` are public, allowing callers to bypass the unified `QueueClient` API and directly access low-level queue operations.

4. **Inconsistent Module Organization**: Database infrastructure is split across multiple locations (`database/connection`, `database/entities`, `repositories`), making code harder to navigate and maintain.

This leads to:
- Unclear code organization and poor discoverability
- Inconsistent usage patterns (some use `QueueClient`, others use `TaskQueue` directly)
- Difficult maintenance of database-related code
- Poor separation of concerns

## What Changes

- **MOVED**: `src/infrastructure/repositories/` → `src/infrastructure/database/repositories/`
- **MOVED**: `src/queue/` → `src/infrastructure/queue/`
- **HIDDEN**: `TaskQueue` trait and `TaskScheduler` become private/internal
- **PUBLIC**: `QueueClient` becomes the only public interface for queue operations
- **BREAKING**: All imports of `TaskQueue` and `TaskScheduler` must use `QueueClient` instead

## Impact

- Affected specs: `database`, `queue` (new capabilities)
- Affected code:
  - `src/infrastructure/repositories/` → `src/infrastructure/database/repositories/`
  - `src/queue/` → `src/infrastructure/queue/`
  - `src/main.rs` - Update module paths
  - `src/workers/` - Update queue usage
  - All code importing from `queue` or `repositories`

## Migration Strategy

1. Create new directory structure
2. Move files to new locations
3. Update all imports and module declarations
4. Make `TaskQueue` and `TaskScheduler` private
5. Ensure all code uses `QueueClient` only
6. Update documentation