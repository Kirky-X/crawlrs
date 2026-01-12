# Change: Refactor Engines Module - Unified EngineClient API

## Why

The current `src/engines` module has several architectural issues:

1. **Leaky Abstractions**: Internal details like UA rotation, circuit breaker state, and engine selection are exposed through trait methods
2. **Inconsistent API**: Each engine exposes `support_score()` and direct access, allowing callers to bypass smart routing
3. **Tight Coupling**: Callers depend on concrete engine implementations rather than a unified interface
4. **Missing Encapsulation**: Features like retry logic, UA rotation, and fallback are implemented at the caller level

This leads to:
- Difficult maintenance and evolution of engine implementations
- Inconsistent behavior across different callers
- Security risks from exposing internal state
- Poor separation of concerns

## What Changes

- **NEW**: Create unified `EngineClient` as the sole public API for all scraping operations
- **NEW**: Define `ScrapeRequest` and `ScrapeResponse` as the canonical request/response types
- **NEW**: Expose health check functionality through `EngineClient`
- **HIDDEN**: Move UA rotation, circuit breaker, and retry logic to internal implementation
- **REMOVED**: Direct access to `ScraperEngine` trait from public API
- **REMOVED**: `support_score()` method from public interface
- **BREAKING**: Existing code using direct engine access must migrate to `EngineClient`

## Impact

- Affected specs: `engines` (new capability)
- Affected code:
  - `src/engines/` - Complete refactoring
  - `src/application/` - API layer updates
  - `src/presentation/` - HTTP handler updates
- Breaking change for internal consumers of engine trait

## Migration Strategy

1. Create `EngineClient` with unified API
2. Move all callers to use `EngineClient`
3. Mark old API as deprecated during transition
4. Remove deprecated API after migration complete
