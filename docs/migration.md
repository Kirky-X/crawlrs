# Migration Guide: EngineClient Migration

This guide helps you migrate from the legacy `ScraperEngine` trait-based API to the new unified `EngineClient` API.

## Overview

The old API exposed implementation details like `ScraperEngine`, `support_score()`, and circuit breaker state. The new `EngineClient` provides a simple, opinionated interface that handles all internal complexity automatically.

## Migration Steps

### Before: Old API

```rust
use crawlrs::engines::{ScraperEngine, ReqwestEngine, ScrapeRequest, ScrapeResponse};

// Create engine directly
let engine = ReqwestEngine;

// Create request
let request = ScrapeRequest {
    url: "https://example.com".to_string(),
    headers: HashMap::new(),
    timeout: Duration::from_secs(30),
    needs_js: false,
    needs_screenshot: false,
    screenshot_config: None,
    mobile: false,
    proxy: None,
    skip_tls_verification: false,
    needs_tls_fingerprint: false,
    use_fire_engine: false,
};

// Scrape directly
let response: ScrapeResponse = engine.scrape(&request).await?;
```

### After: New API

```rust
use crawlrs::engine_client::{EngineClient, ScrapeRequest, ScrapeResponse};

// Create client
let client = EngineClient::new();

// Create request with builder pattern
let request = ScrapeRequest::new("https://example.com")
    .needs_js()
    .timeout(Duration::from_secs(30))
    .build();

// Scrape
let response: ScrapeResponse = client.scrape(&request).await?;
```

## Key Changes

| Old API | New API |
|---------|---------|
| `ScraperEngine` trait | `EngineClient` struct |
| Direct engine access | Automatic engine selection |
| Manual UA rotation | Built-in UA rotation |
| Manual circuit breaker | Automatic circuit breaking |
| Complex request construction | Builder pattern |

## Feature Comparison

### Request Options

| Feature | Old API | New API |
|---------|---------|---------|
| URL | `url: String` | `ScrapeRequest::new(url)` |
| JS rendering | `needs_js: bool` | `.needs_js()` |
| Screenshot | `needs_screenshot: bool` | `.needs_screenshot()` |
| Mobile UA | `mobile: bool` | `.mobile()` |
| Timeout | `timeout: Duration` | `.timeout(duration)` |
| Proxy | `proxy: Option<String>` | `.proxy(url)` |
| Custom headers | Manual `HashMap` | `.headers(HashMap)` |
| TLS fingerprint | `needs_tls_fingerprint` | `.needs_tls_fingerprint()` |
| Fire Engine | `use_fire_engine` | `.use_fire_engine()` |

### Response Types

| Field | Old API | New API |
|-------|---------|---------|
| Status code | `response.status` | `response.status_code` |
| Content | `response.content` | `response.content` |
| Screenshot | `response.screenshot` | `response.screenshot` |
| Content type | N/A | `response.content_type` |
| Headers | N/A | `response.headers` |
| Final URL | N/A | `response.final_url` |
| Response time | N/A | `response.response_time_ms` |

## Error Handling

### Before: Old Error Types

```rust
use crawlrs::engines::EngineError;

match engine.scrape(&request).await {
    Ok(response) => { /* success */ }
    Err(EngineError::RequestFailed(msg)) => { /* retryable */ }
    Err(EngineError::Timeout(duration)) => { /* timeout */ }
    Err(EngineError::AllEnginesFailed(msg)) => { /* all failed */ }
    Err(EngineError::SsrfProtection(msg)) => { /* blocked */ }
    Err(EngineError::BrowserError(msg)) => { /* browser issue */ }
    Err(EngineError::Expired) => { /* task expired */ }
    Err(EngineError::Other(msg)) => { /* other */ }
}
```

### After: New Error Types

```rust
use crawlrs::engine_client::EngineError;

match client.scrape(&request).await {
    Ok(response) => { /* success */ }
    Err(EngineError::RequestFailed(msg)) => { /* retryable */ }
    Err(EngineError::Timeout(duration)) => { /* timeout */ }
    Err(EngineError::NoEnginesAvailable) => { /* no engines */ }
    Err(EngineError::InvalidUrl(msg)) => { /* invalid URL */ }
    Err(EngineError::SsrfProtection(msg)) => { /* blocked */ }
    Err(EngineError::BrowserError(msg)) => { /* browser issue */ }
    Err(EngineError::Internal(msg)) => { /* internal error */ }
}
```

## Health Checks

### Before: Direct Engine Access

```rust
// Required understanding of circuit breaker state
let health = engine.health();
```

### After: Unified Health Status

```rust
// Simple status check
let status = client.health_check().await;

match status {
    EngineHealthStatus::Healthy => { /* all good */ }
    EngineHealthStatus::Degraded { unhealthy_engines, message } => {
        println!("Degraded: {}", message);
    }
    EngineHealthStatus::Unavailable { message } => {
        println!("Unavailable: {}", message);
    }
}
```

## Advanced Configuration

### Custom Engines

```rust
use crawlrs::engine_client::EngineClient;
use crawlrs::engines::ReqwestEngine;
use std::sync::Arc;

// Register custom engines
let engines = vec![
    Arc::new(ReqwestEngine) as Arc<dyn ScraperEngine>,
];
let client = EngineClient::with_engines(engines);
```

### Builder Pattern

```rust
let options = ScrapeOptions::builder()
    .needs_js(true)
    .needs_screenshot(true)
    .mobile(true)
    .timeout(Duration::from_secs(60))
    .sync_wait_ms(1000)
    .skip_tls_verification(false)
    .proxy("http://proxy.example.com")
    .headers(HashMap::from([
        ("Authorization".to_string(), "Bearer token".to_string()),
    ]))
    .needs_tls_fingerprint(true)
    .use_fire_engine(false)
    .build();

let request = ScrapeRequest::new("https://example.com")
    .with_options(options);
```

## Common Patterns

### Basic Scraping

```rust
let client = EngineClient::new();

let response = client
    .scrape(&ScrapeRequest::new("https://example.com"))
    .await?;

println!("Status: {}", response.status_code);
println!("Content length: {}", response.content.len());
```

### JavaScript Rendering

```rust
let response = client
    .scrape(&ScrapeRequest::new("https://example.com").needs_js())
    .await?;
```

### Screenshot Capture

```rust
let response = client
    .scrape(&ScrapeRequest::new("https://example.com")
        .needs_screenshot())
    .await?;

if let Some(screenshot) = response.screenshot {
    // Save or process base64 screenshot
    std::fs::write("screenshot.jpg", &base64::decode(&screenshot).unwrap())?;
}
```

### Mobile View

```rust
let response = client
    .scrape(&ScrapeRequest::new("https://example.com")
        .mobile()
        .needs_js())
    .await?;
```

### With Custom Timeout

```rust
let response = client
    .scrape(&ScrapeRequest::new("https://example.com")
        .timeout(Duration::from_secs(120)))
    .await?;
```

## FAQ

### Q: Can I still use the old API?
A: The old API is deprecated but still available. We'll remove it in a future version after the migration period.

### Q: How do I access circuit breaker state?
A: Circuit breaker state is now internal. Use `health_check()` to get aggregate status.

### Q: Can I disable automatic retries?
A: Automatic retries are built-in and cannot be disabled. This ensures reliable scraping.

### Q: How do I use a custom proxy?
A: Use `.proxy(url)` in the options builder.

### Q: Can I choose which engine to use?
A: Engine selection is automatic based on request requirements (e.g., `needs_js()` uses Playwright).

## Support

- Issues: https://github.com/Kirky-X/crawlrs/issues
- Discussions: https://github.com/Kirky-X/crawlrs/discussions
- Migration Status: See [CHANGELOG.md](./CHANGELOG.md)
