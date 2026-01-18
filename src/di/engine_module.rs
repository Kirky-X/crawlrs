// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Engine module for Shaku dependency injection.
//!
//! This module provides Shaku components for engine layer dependencies
//! including EngineClient, EngineRouter, and individual engine implementations.

use shaku::Component;
use std::sync::Arc;

use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::EngineClient;
use crate::engines::health_monitor::EngineHealthMonitor;
use crate::engines::router::EngineRouter;
use crate::engines::traits::ScraperEngine;

/// Component parameters for EngineModule
#[derive(shaku::ComponentParameters)]
pub struct EngineModuleParameters {
    /// Proxy URL for engines
    pub proxy_url: String,
}

/// EngineRouter component
#[derive(Component)]
#[shaku(interface = EngineRouter)]
pub struct EngineRouterComponent {
    /// Vector of all scraper engines
    #[shaku(inject)]
    engines: Vec<Arc<dyn ScraperEngine>>,
    
    /// Load balancing strategy
    // strategy: LoadBalancingStrategy, // TODO: Add when needed
}

impl EngineRouter for EngineRouterComponent {
    // Implementation delegated to internal router
}

/// EngineHealthMonitor component
#[derive(Component)]
#[shaku(interface = EngineHealthMonitor)]
pub struct EngineHealthMonitorComponent {
    /// Vector of engines to monitor
    #[shaku(inject)]
    engines: Vec<Arc<dyn ScraperEngine>>,
}

impl EngineHealthMonitor for EngineHealthMonitorComponent {
    // Implementation delegated to internal monitor
}

/// EngineClient component
#[derive(Component)]
#[shaku(interface = EngineClient)]
pub struct EngineClientComponent {
    /// Engine router for selecting appropriate engines
    #[shaku(inject)]
    router: Arc<dyn EngineRouter>,
    
    /// Health monitor for engine status
    #[shaku(inject)]
    health_monitor: Arc<dyn EngineHealthMonitor>,
}

impl EngineClient for EngineClientComponent {
    // Implementation delegated to internal client
}

/// ReqwestEngine component (default HTTP engine)
#[derive(Component)]
#[shaku(interface = dyn ScraperEngine)]
pub struct ReqwestEngineComponent {
    /// Proxy URL
    proxy_url: String,
}

impl ReqwestEngineComponent {
    pub fn new(proxy_url: String) -> Self {
        Self { proxy_url }
    }
}

impl ScraperEngine for ReqwestEngineComponent {
    // Implementation delegated to ReqwestEngine
}

/// Engine module for Shaku DI
///
/// This module provides all engine components including:
/// - EngineClient (main interface for scraping)
/// - EngineRouter (load balancing and engine selection)
/// - EngineHealthMonitor (health checking)
/// - Individual engine implementations (ReqwestEngine, etc.)
shaku::module! {
    pub EngineModule {
        components = [
            EngineRouterComponent,
            EngineHealthMonitorComponent,
            EngineClientComponent,
            ReqwestEngineComponent,
        ],
        providers = []
    }
}
