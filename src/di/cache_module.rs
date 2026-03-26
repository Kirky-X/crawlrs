// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Cache module for Shaku dependency injection.
//!
//! This module provides Shaku components for cache layer dependencies
//! including Redis client, OxCache, rate limiter, and concurrency controller.

use std::sync::Arc;

use futures::executor::block_on;
use shaku::{Component, HasComponent, Interface, Module, ModuleBuildContext};

use crate::infrastructure::cache::redis_client::RedisClient;
use crate::infrastructure::oxcache::{
    create_cache, ConcurrencyController, RateLimiter, SearchCache,
};

use super::database_module::SettingsTrait;

// =============================================================================
// Redis Client Component
// =============================================================================

/// Trait for RedisClient component
pub trait RedisClientTrait: Interface + Send + Sync {
    fn get_client(&self) -> Arc<RedisClient>;
}

/// RedisClient component for Shaku DI
#[allow(dead_code)]
pub struct RedisClientComponent {
    /// Redis URL
    redis_url: String,
    /// Redis client
    client: Arc<RedisClient>,
}

impl<M: Module + HasComponent<dyn SettingsTrait>> Component<M> for RedisClientComponent {
    type Interface = dyn RedisClientTrait;
    type Parameters = ();

    fn build(context: &mut ModuleBuildContext<M>, _: Self::Parameters) -> Box<Self::Interface> {
        let settings_component = M::build_component(context);
        let settings = settings_component.get();
        let client = Arc::new(
            RedisClient::from_settings(&settings.redis)
                .expect("Failed to create Redis client"),
        );
        Box::new(Self::new(settings.redis.url().to_string(), client))
    }
}

impl RedisClientComponent {
    /// Create a new RedisClientComponent with explicit dependencies
    pub fn new(redis_url: String, client: Arc<RedisClient>) -> Self {
        Self { redis_url, client }
    }
}

impl RedisClientTrait for RedisClientComponent {
    fn get_client(&self) -> Arc<RedisClient> {
        self.client.clone()
    }
}

// =============================================================================
// OxCache Component (Extended with RateLimiter and ConcurrencyController)
// =============================================================================

/// Trait for OxCache component
pub trait OxCacheTrait: Interface + Send + Sync {
    fn get_cache(&self) -> Arc<SearchCache>;
    fn get_rate_limiter(&self) -> Arc<RateLimiter>;
    fn get_concurrency_controller(&self) -> Arc<ConcurrencyController>;
}

/// OxCache component for Shaku DI
///
/// This component provides the unified oxcache instance for all caching operations,
/// including search cache, rate limiting, and concurrency control.
pub struct OxCacheComponent {
    cache: Arc<SearchCache>,
    rate_limiter: Arc<RateLimiter>,
    concurrency_controller: Arc<ConcurrencyController>,
}

impl<M: Module + HasComponent<dyn SettingsTrait>> Component<M> for OxCacheComponent {
    type Interface = dyn OxCacheTrait;
    type Parameters = ();

    fn build(context: &mut ModuleBuildContext<M>, _: Self::Parameters) -> Box<Self::Interface> {
        let settings_component: Arc<dyn SettingsTrait> = M::build_component(context);
        let settings = settings_component.get();

        // Create search cache
        let cache = block_on(create_cache(&settings.cache)).expect("Failed to create oxcache");

        // Create rate limiter with config from settings
        let rate_limiter = Arc::new(RateLimiter::new_with_config(
            settings.rate_limiting.default_limit as u64,
            settings.rate_limiting.burst_size as u64,
        ));

        // Create concurrency controller
        let concurrency_controller = Arc::new(ConcurrencyController::new(
            settings.concurrency.default_team_limit as usize,
        ));

        Box::new(Self {
            cache,
            rate_limiter,
            concurrency_controller,
        })
    }
}

impl OxCacheComponent {
    pub fn new(
        cache: Arc<SearchCache>,
        rate_limiter: Arc<RateLimiter>,
        concurrency_controller: Arc<ConcurrencyController>,
    ) -> Self {
        Self {
            cache,
            rate_limiter,
            concurrency_controller,
        }
    }
}

impl OxCacheTrait for OxCacheComponent {
    fn get_cache(&self) -> Arc<SearchCache> {
        self.cache.clone()
    }

    fn get_rate_limiter(&self) -> Arc<RateLimiter> {
        self.rate_limiter.clone()
    }

    fn get_concurrency_controller(&self) -> Arc<ConcurrencyController> {
        self.concurrency_controller.clone()
    }
}

// Cache module components - for Shaku DI
