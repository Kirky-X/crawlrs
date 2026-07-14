// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Engines module for dependency injection.
//!
//! This module provides components for engine layer dependencies
//! including EngineClient and EngineRouter.

use std::sync::Arc;

use crate::engines::engine_client::{EngineClient, EngineClientTrait};
use crate::engines::router::{EngineRouter, EngineRouterTrait};

/// EngineClient component
pub struct EngineClientComponent {
    /// Engine router
    router: Arc<dyn EngineRouterTrait>,
}

impl EngineClientComponent {
    /// Create a new EngineClientComponent with explicit dependencies
    pub fn new(router: Arc<dyn EngineRouterTrait>) -> Self {
        Self { router }
    }
}

#[async_trait::async_trait]
impl EngineClientTrait for EngineClientComponent {
    async fn scrape(
        &self,
        request: &crate::engines::engine_client::ScrapeRequest,
    ) -> Result<
        crate::engines::engine_client::ScrapeResponse,
        crate::engines::engine_client::EngineError,
    > {
        // Create EngineClient with the injected router
        let client = EngineClient::with_router(self.router.clone());
        client.scrape(request).await
    }

    async fn health_check(&self) -> crate::engines::engine_client::EngineHealthStatus {
        let client = EngineClient::with_router(self.router.clone());
        client.health_check().await
    }

    fn engine_count(&self) -> usize {
        let client = EngineClient::with_router(self.router.clone());
        client.engine_count()
    }

    fn registered_engines(&self) -> Vec<String> {
        let client = EngineClient::with_router(self.router.clone());
        client.registered_engines()
    }
}

/// EngineRouter component
pub struct EngineRouterComponent {
    /// List of engines
    engines: Vec<Arc<dyn crate::engines::engine_client::ScraperEngine>>,
}

impl EngineRouterComponent {
    /// Create a new EngineRouterComponent with explicit dependencies
    pub fn new(engines: Vec<Arc<dyn crate::engines::engine_client::ScraperEngine>>) -> Self {
        Self { engines }
    }

    /// Create with default engines (empty)
    pub fn with_defaults() -> Self {
        Self {
            engines: Vec::new(),
        }
    }
}

#[async_trait::async_trait]
impl EngineRouterTrait for EngineRouterComponent {
    async fn route(
        &self,
        request: &crate::engines::engine_client::InternalScrapeRequest,
    ) -> Result<
        crate::engines::engine_client::InternalScrapeResponse,
        crate::engines::engine_client::EngineError,
    > {
        let router = EngineRouter::new(self.engines.clone());
        router._route_impl(request).await
    }

    async fn aggregate(
        &self,
        request: &crate::engines::engine_client::InternalScrapeRequest,
    ) -> Result<
        crate::engines::engine_client::InternalScrapeResponse,
        crate::engines::engine_client::EngineError,
    > {
        let router = EngineRouter::new(self.engines.clone());
        // For now, delegate to route
        router._route_impl(request).await
    }

    fn get_engine_stats(
        &self,
    ) -> std::collections::HashMap<String, crate::engines::router::EngineStats> {
        let router = EngineRouter::new(self.engines.clone());
        router.get_engine_stats()
    }

    fn reset_engine_stats(&self, engine_name: &str) {
        let router = EngineRouter::new(self.engines.clone());
        router.reset_engine_stats(engine_name);
    }

    fn registered_engines(&self) -> Vec<String> {
        let router = EngineRouter::new(self.engines.clone());
        router.registered_engines()
    }
}

// Engines module components

#[cfg(test)]
mod tests {
    use super::*;

    // ========== EngineRouterComponent ==========

    #[test]
    fn test_engine_router_component_with_defaults_has_no_engines() {
        let component = EngineRouterComponent::with_defaults();
        // 空引擎列表应返回 0 个已注册引擎
        let engines = component.registered_engines();
        assert!(engines.is_empty());
    }

    #[test]
    fn test_engine_router_component_new_with_empty_engines() {
        let component = EngineRouterComponent::new(Vec::new());
        let engines = component.registered_engines();
        assert!(engines.is_empty());
    }

    #[test]
    fn test_engine_router_component_get_engine_stats_empty() {
        let component = EngineRouterComponent::with_defaults();
        let stats = component.get_engine_stats();
        // 空引擎列表应返回空的统计 HashMap
        assert!(stats.is_empty());
    }

    #[test]
    fn test_engine_router_component_reset_engine_stats_no_panic() {
        let component = EngineRouterComponent::with_defaults();
        // 重置不存在的引擎统计不应 panic
        component.reset_engine_stats("nonexistent_engine");
    }

    // ========== EngineClientComponent ==========

    #[test]
    fn test_engine_client_component_new_with_empty_router() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouterComponent::with_defaults());
        let component = EngineClientComponent::new(router);
        // 空路由器应返回 0 个引擎
        assert_eq!(component.engine_count(), 0);
        assert!(component.registered_engines().is_empty());
    }

    #[test]
    fn test_engine_client_component_as_trait_object() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouterComponent::with_defaults());
        let component = EngineClientComponent::new(router);
        let trait_obj: &dyn crate::engines::engine_client::EngineClientTrait = &component;
        // 通过 trait 对象访问，验证动态分发正常工作
        assert_eq!(trait_obj.engine_count(), 0);
    }
}
