## Context

This optimization targets the critical path in high-throughput scenarios. The system currently processes 5,000+ RPS, and these bottlenecks become significant at scale.

### Constraints
- Must maintain backward compatibility
- Cannot introduce breaking API changes
- Must be deployable without downtime

### Stakeholders
- Platform Engineering Team
- DevOps Team
- Product Team (performance SLAs)

## Goals / Non-Goals

### Goals
- Reduce P99 latency from 300ms to 200ms
- Reduce Redis connection overhead by 75%
- Eliminate redundant system calls
- Improve CPU efficiency for regex operations

### Non-Goals
- Database query optimization (out of scope)
- Network I/O optimization beyond Redis
- Changes to API contracts

## Decisions

### 1. Lua Script for Redis Concurrency Control

**Decision**: Use single Lua script for clean + add + rank + check operations

**Rationale**:
- Atomic execution prevents race conditions
- Single network round-trip minimizes latency
- Redis Lua execution is highly optimized

**Alternatives Considered**:
- Pipeline batching: Not atomic, potential race conditions
- Multi-command transaction: Still 4 round-trips

### 2. Atomic Metrics Storage

**Decision**: Use `std::sync::atomic::AtomicU64` for CPU/memory metrics

**Rationale**:
- Zero-cost reads in hot path
- Thread-safe without locks
- Simple implementation

**Alternatives Considered**:
- `Arc<RwLock<Metrics>>`: Additional locking overhead
- `once_cell::sync::Lazy`: Requires initialization, less flexible for updates

### 3. Robots.txt Lazy Check

**Decision**: Cache robots rules per-domain, check at worker execution time

**Rationale**:
- Reduces network requests for tasks that may fail earlier
- Cache hit rate improves over time
- Backward compatible with existing behavior

**Alternatives Considered**:
- Check at URL discovery: More network requests, higher latency
- Check after scheduling: Still may process tasks that get cancelled

### 4. Regex Caching Strategy

**Decision**: Use `OnceLock` for shared patterns, pre-compile task patterns

**Rationale**:
- `OnceLock` is zero-cost after first use
- Clear separation between shared and task-specific patterns
- No runtime synchronization needed after initialization

**Alternatives Considered**:
- `lazy_static`: Requires macro, slightly more complex
- Manual `Mutex<HashMap>`: Lock contention in hot path

## Risks / Trade-offs

| Risk | Impact | Mitigation |
|------|--------|------------|
| Lua script complexity | Debugging harder | Comprehensive logging, unit tests |
| Cache memory usage | Increased memory | TTL-based eviction, size limits |
| Metrics staleness | Slight delay in scaling | 1-second update interval is acceptable |
| Breaking existing robots behavior | Some URLs may be crawled that shouldn't | Lazy check only defers, not bypasses |

## Migration Plan

### Phase 1: Caching Layer (Week 1)
1. Add `RobotsTxtCache` with Redis backend
2. Implement `OnceLock` for shared regex patterns
3. Update `metrics.rs` with atomic storage

### Phase 2: Redis Optimization (Week 2)
1. Develop and test Lua script
2. Add unit tests for Lua script logic
3. Deploy with feature flag
4. Gradual rollout (10% → 50% → 100%)

### Phase 3: Integration (Week 3)
1. Wire lazy robots check into worker
2. Enable atomic metrics in production
3. Monitor performance metrics
4. Rollback plan ready

## Open Questions

1. **Redis Lua script deployment**: Should we use `SCRIPT LOAD` on startup or inline the script?
2. **Cache size limits**: What's the appropriate max size for `RobotsTxtCache`?
3. **Metrics granularity**: Is 1-second update interval sufficient, or do we need faster?
4. **Rollback strategy**: How do we handle partial rollout failures?
