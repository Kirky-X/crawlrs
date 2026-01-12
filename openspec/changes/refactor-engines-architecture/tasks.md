## 1. Core Types Definition

- [ ] 1.1 Define `ScrapeOptions` struct with optional fields
- [ ] 1.2 Define `ScrapeResponse` with content, screenshot, metadata
- [ ] 1.3 Define `EngineHealthStatus` enum
- [ ] 1.4 Define `EngineError` variants for client

## 2. EngineClient Implementation

- [ ] 2.1 Create `EngineClient` struct with internal fields
- [ ] 2.2 Implement `EngineClient::new()` constructor
- [ ] 2.3 Implement `EngineClient::scrape()` method
- [ ] 2.4 Implement `EngineClient::health_check()` method
- [ ] 2.5 Make fields private, hide internal details

## 3. Internal Refactoring

- [ ] 3.1 Create internal `EngineRouter` wrapper
- [ ] 3.2 Move `ScraperEngine` trait to internal module
- [ ] 3.3 Hide `support_score()` from public API
- [ ] 3.4 Make circuit breaker internal-only
- [ ] 3.5 Encapsulate UA rotation in client

## 4. Health Check Integration

- [ ] 4.1 Implement `EngineHealthStatus` conversion
- [ ] 4.2 Add health check to `EngineClient`
- [ ] 4.3 Wire health monitor to client

## 5. Migration Support

- [ ] 5.1 Add `#[deprecated]` to old public API
- [ ] 5.2 Create migration guide documentation
- [ ] 5.3 Add compile-time migration warnings

## 6. Testing

- [ ] 6.1 Write unit tests for `EngineClient`
- [ ] 6.2 Write integration tests for health check
- [ ] 6.3 Write tests for request/response serialization
- [ ] 6.4 Test error scenarios and edge cases

## 7. Documentation

- [ ] 7.1 Add module-level documentation
- [ ] 7.2 Document `ScrapeOptions` fields
- [ ] 7.3 Add examples for common use cases
- [ ] 7.4 Write migration guide for existing callers
