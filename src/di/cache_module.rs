// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Cache module for dependency injection.
//!
//! This module provides components for cache layer dependencies
//! including OxCache and concurrency controller.

use std::sync::Arc;

use crate::infrastructure::oxcache::{ConcurrencyController, SearchCache};

// =============================================================================
// OxCache Component (Cache + ConcurrencyController)
// =============================================================================

/// Trait for OxCache component
pub trait OxCacheTrait: Send + Sync {
    fn get_cache(&self) -> Arc<SearchCache>;
    fn get_concurrency_controller(&self) -> Arc<ConcurrencyController>;
}

/// OxCache component
///
/// This component provides the unified oxcache instance for all caching operations,
/// including search cache and concurrency control.
/// Rate limiting is handled by `LimiteronService` via the domain
/// `RateLimitingService` trait.
pub struct OxCacheComponent {
    cache: Arc<SearchCache>,
    concurrency_controller: Arc<ConcurrencyController>,
}

impl OxCacheComponent {
    pub fn new(
        cache: Arc<SearchCache>,
        concurrency_controller: Arc<ConcurrencyController>,
    ) -> Self {
        Self {
            cache,
            concurrency_controller,
        }
    }
}

impl OxCacheTrait for OxCacheComponent {
    fn get_cache(&self) -> Arc<SearchCache> {
        self.cache.clone()
    }

    fn get_concurrency_controller(&self) -> Arc<ConcurrencyController> {
        self.concurrency_controller.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    // ========== OxCacheComponent ==========

    /// 构造一个用于测试的 OxCacheComponent（使用独立的 cache/concurrency_controller）
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
        let concurrency_controller = Arc::new(ConcurrencyController::new(10));
        OxCacheComponent::new(cache, concurrency_controller)
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
