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
            RedisClient::from_settings(&settings.redis).expect("Failed to create Redis client"),
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

#[cfg(test)]
mod tests {
    use super::*;

    // ========== RedisClientComponent ==========

    #[test]
    fn test_redis_client_component_new_stores_client() {
        // RedisClient::new 创建连接池对象但不会立即建立连接，无需运行 Redis 服务器
        let client = Arc::new(RedisClient::new("redis://localhost:6379").unwrap());
        let component =
            RedisClientComponent::new("redis://localhost:6379".to_string(), client.clone());
        let retrieved = component.get_client();
        assert!(Arc::ptr_eq(&retrieved, &client));
    }

    #[test]
    fn test_redis_client_component_get_returns_clone() {
        let client = Arc::new(RedisClient::new("redis://localhost:6379").unwrap());
        let component =
            RedisClientComponent::new("redis://localhost:6379".to_string(), client.clone());
        let first = component.get_client();
        let second = component.get_client();
        // 多次调用应返回指向同一 RedisClient 的 Arc
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&first, &client));
    }

    #[test]
    fn test_redis_client_component_as_trait_object() {
        let client = Arc::new(RedisClient::new("redis://localhost:6379").unwrap());
        let component =
            RedisClientComponent::new("redis://localhost:6379".to_string(), client.clone());
        let trait_obj: &dyn RedisClientTrait = &component;
        let retrieved = trait_obj.get_client();
        assert!(Arc::ptr_eq(&retrieved, &client));
    }

    // ========== OxCacheComponent ==========

    /// 构造一个用于测试的 OxCacheComponent（使用独立的 cache/rate_limiter/concurrency_controller）
    fn make_oxcache_component() -> OxCacheComponent {
        let cache: Arc<SearchCache> = Arc::new(
            block_on(
                oxcache::Cache::builder()
                    .capacity(100)
                    .ttl(std::time::Duration::from_secs(3600))
                    .build(),
            )
            .unwrap(),
        );
        let rate_limiter = Arc::new(RateLimiter::new_with_config(100, 20));
        let concurrency_controller = Arc::new(ConcurrencyController::new(10));
        OxCacheComponent::new(cache, rate_limiter, concurrency_controller)
    }

    #[test]
    fn test_oxcache_component_get_cache_returns_stored_arc() {
        let component = make_oxcache_component();
        let cache1 = component.get_cache();
        let cache2 = component.get_cache();
        // 多次调用应返回同一 Arc
        assert!(Arc::ptr_eq(&cache1, &cache2));
    }

    #[test]
    fn test_oxcache_component_get_rate_limiter_returns_stored_arc() {
        let component = make_oxcache_component();
        let limiter1 = component.get_rate_limiter();
        let limiter2 = component.get_rate_limiter();
        assert!(Arc::ptr_eq(&limiter1, &limiter2));
    }

    #[test]
    fn test_oxcache_component_get_concurrency_controller_returns_stored_arc() {
        let component = make_oxcache_component();
        let controller1 = component.get_concurrency_controller();
        let controller2 = component.get_concurrency_controller();
        assert!(Arc::ptr_eq(&controller1, &controller2));
    }

    #[test]
    fn test_oxcache_component_as_trait_object() {
        let component = make_oxcache_component();
        let trait_obj: &dyn OxCacheTrait = &component;
        // 通过 trait 对象访问，验证动态分发正常工作
        let _cache = trait_obj.get_cache();
        let _limiter = trait_obj.get_rate_limiter();
        let _controller = trait_obj.get_concurrency_controller();
    }

    #[test]
    fn test_oxcache_component_concurrency_controller_has_correct_permits() {
        let component = make_oxcache_component();
        let controller = component.get_concurrency_controller();
        // 构造时设置 max_permits=10，初始可用许可应为 10
        assert_eq!(controller.available_permits(), 10);
    }
}
