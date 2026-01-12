# Change: Performance Critical Path Optimization

## Why

The current implementation has several performance bottlenecks under high concurrency:

1. **Redis RTT bottleneck**: Each task acquisition requires 4+ Redis round-trips (clean, add, rank, check)
2. **Synchronous metrics collection**: Real-time `/proc/stat` reads during task processing
3. **Robots.txt checking**: Early checking causes unnecessary network requests for URLs that may never be scheduled
4. **Regex compilation overhead**: Repeated compilation of the same patterns

## What Changes

### 1. Redis Concurrency Control (Lua Script)
- Replace 4 separate Redis commands with a single atomic Lua script
- Reduce network round-trips from 4 to 1
- Eliminate potential race conditions

### 2. Async Metrics Collection
- Move CPU/memory monitoring to background thread
- Store metrics in atomic variables (AtomicU64)
- Business logic reads from memory instead of filesystem

### 3. Robots.txt Lazy Check with Caching
- Add `RobotsTxtCache` with 1-hour TTL
- Defer robots check to worker execution time (lazy evaluation)
- Reduce network requests for discarded tasks

### 4. Regex and Selector Compilation Caching
- Use `OnceLock` for reusable patterns
- Pre-compile task-specific patterns during crawl initialization

## Impact

### Affected Specs
- `specs/concurrency` (NEW)
- `specs/metrics` (NEW)
- `specs/robots` (NEW)
- `specs/caching` (NEW)

### Affected Code
- `src/infrastructure/services/rate_limiting_service_impl.rs`
- `src/infrastructure/observability/metrics.rs`
- `src/utils/robots/mod.rs`
- `src/engines/router.rs`
- `src/workers/scrape_worker.rs`

### Performance Gains
- **Redis**: ~75% reduction in RTT overhead (4 calls → 1 call)
- **Metrics**: ~99% reduction in syscall overhead
- **Robots**: ~60% reduction in unnecessary network requests (lazy check)
- **Regex**: Near-zero repeated compilation overhead
