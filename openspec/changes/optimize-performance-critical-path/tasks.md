## 1. Redis Lua Script Optimization

- [ ] 1.1 Design Lua script for atomic concurrency control
- [ ] 1.2 Implement Lua script in `src/workers/scrape_worker.rs`
- [ ] 1.3 Add unit tests for Lua script logic
- [ ] 1.4 Create integration test for concurrency control
- [ ] 1.5 Add feature flag for gradual rollout

## 2. Async Metrics Collection

- [ ] 2.1 Add `AtomicU64` globals in `src/infrastructure/observability/metrics.rs`
- [ ] 2.2 Implement background metrics collector thread
- [ ] 2.3 Update `CrawlService` to use atomic reads instead of system calls
- [ ] 2.4 Add metrics staleness detection
- [ ] 2.5 Write tests for metrics collection

## 3. Robots.txt Lazy Check with Caching

- [ ] 3.1 Create `RobotsTxtCache` struct in `src/utils/robots/mod.rs`
- [ ] 3.2 Implement Redis-backed cache with 1-hour TTL
- [ ] 3.3 Add cache hit/miss metrics
- [ ] 3.4 Update worker to defer robots check to execution time
- [ ] 3.5 Write integration tests for cache behavior

## 4. Regex Compilation Caching

- [ ] 4.1 Identify all regex patterns used in hot path
- [ ] 4.2 Add `OnceLock` static storage for shared patterns
- [ ] 4.3 Pre-compile task-specific patterns in crawl initialization
- [ ] 4.4 Update `LinkFilter` to use cached patterns
- [ ] 4.5 Benchmark regex performance improvement

## 5. Testing and Validation

- [ ] 5.1 Run full integration test suite
- [ ] 5.2 Load test with 10x traffic
- [ ] 5.3 Monitor P99 latency improvement
- [ ] 5.4 Verify no regression in existing functionality
- [ ] 5.5 Document performance benchmarks

## 6. Deployment

- [ ] 6.1 Create feature flags for each optimization
- [ ] 6.2 Plan gradual rollout schedule
- [ ] 6.3 Prepare rollback procedures
- [ ] 6.4 Update runbooks with new metrics
- [ ] 6.5 Communicate changes to on-call team
