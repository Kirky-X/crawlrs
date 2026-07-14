// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! RateLimiter 实现
//!
//! 从 mod.rs 拆出的 RateLimiter impl 块。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{RateLimitConfig, RateLimiter, TokenBucket};

impl RateLimiter {
    /// Create a new rate limiter with default config
    #[allow(clippy::new_ret_no_self)]
    pub fn new_default() -> Self {
        let config = RateLimitConfig {
            max_requests_per_second: 60,
            burst_capacity: 20,
            block_duration_secs: 1,
        };
        Self::new(config)
    }

    /// Create a new rate limiter with custom config
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            inner: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Create a new rate limiter with individual parameters
    pub fn new_with_config(max_requests_per_second: u64, burst_capacity: u64) -> Self {
        let config = RateLimitConfig {
            max_requests_per_second,
            burst_capacity,
            block_duration_secs: 1,
        };
        Self::new(config)
    }

    /// Check if a request is allowed for the given client
    pub async fn check_rate_limit(&self, client_id: &str) -> Result<(), Duration> {
        self.check_rate_limit_n(client_id, 1).await
    }

    /// Check if N requests are allowed
    pub async fn check_rate_limit_n(&self, client_id: &str, n: u64) -> Result<(), Duration> {
        let mut buckets = self.inner.lock().await;
        let refill_rate = self.config.max_requests_per_second as f64;
        let capacity = self.config.burst_capacity as f64;

        let bucket = buckets.entry(client_id.to_string()).or_insert(TokenBucket {
            tokens: capacity,
            last_refill: Instant::now(),
        });

        // Refill tokens based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * refill_rate).min(capacity);
        bucket.last_refill = now;

        let requested = n as f64;
        if bucket.tokens >= requested {
            bucket.tokens -= requested;
            Ok(())
        } else {
            let needed = requested - bucket.tokens;
            let retry_after = needed / refill_rate;
            Err(Duration::from_secs_f64(retry_after))
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }
}
