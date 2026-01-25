// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Engines module for Shaku dependency injection.
//!
//! This module provides Shaku components for engine layer dependencies
//! including EngineClient and EngineRouter.

use std::sync::Arc;

use shaku::Component;

use crate::engines::engine_client::{EngineClient, EngineClientTrait};
use crate::engines::router::{EngineRouter, EngineRouterTrait};

/// EngineClient component
#[derive(Component)]
#[shaku(interface = crate::engines::engine_client::EngineClientTrait)]
pub struct EngineClientComponent {
    /// Engine router
    #[shaku(inject)]
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
#[derive(Component)]
#[shaku(interface = EngineRouterTrait)]
pub struct EngineRouterComponent {
    /// List of engines
    #[shaku(default)]
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

// Engines module components - for Shaku DI
