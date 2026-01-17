// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scraper engines initialization and configuration.

use crate::config::engines::EngineSettings;
#[cfg(feature = "engine-fire-cdp")]
use crate::engines::client::fire_cdp::FireEngineCdp;
#[cfg(feature = "engine-fire-tls")]
use crate::engines::client::fire_tls::FireEngineTls;
#[cfg(feature = "engine-playwright")]
use crate::engines::client::playwright::PlaywrightEngine;
use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::EngineClient;
use crate::engines::router::EngineRouter;
use crate::engines::traits::ScraperEngine;
use std::sync::Arc;
use tracing::info;

/// All engine-related components.
#[derive(Clone)]
pub struct EngineComponents {
    /// Vector of all configured scraper engines.
    pub engines: Vec<Arc<dyn ScraperEngine>>,
    /// Engine router for selecting appropriate engines.
    pub router: Arc<EngineRouter>,
    /// Engine client for making requests.
    pub engine_client: Arc<EngineClient>,
}

/// Initialize all scraper engines.
///
/// This function creates and configures all available scraper engines
/// based on the enabled feature flags and configuration.
///
/// # Arguments
///
/// * `proxy_url` - URL for the HTTP proxy (if enabled)
/// * `engine_config` - Engine-specific configuration settings
///
/// # Returns
///
/// Returns a vector of initialized engines.
#[allow(deprecated)]
pub fn init_engines(
    proxy_url: &str,
    engine_config: &EngineSettings,
) -> Vec<Arc<dyn ScraperEngine>> {
    #[allow(unused_mut)]
    let mut engines: Vec<Arc<dyn ScraperEngine>> =
        vec![Arc::new(ReqwestEngine::with_proxy(proxy_url.to_string()))];

    #[cfg(feature = "engine-playwright")]
    engines.push(Arc::new(PlaywrightEngine));

    #[cfg(feature = "engine-fire-tls")]
    if engine_config.fire_tls.enabled {
        info!(
            "Fire Engine TLS enabled with URL: {}",
            engine_config.fire_tls.url
        );
        engines.push(Arc::new(FireEngineTls::with_url_and_proxy(
            engine_config.fire_tls.url.clone(),
            proxy_url.to_string(),
        )));
    }

    #[cfg(feature = "engine-fire-cdp")]
    if engine_config.fire_cdp.enabled {
        info!(
            "Fire Engine CDP enabled with URL: {}",
            engine_config.fire_cdp.url
        );
        engines.push(Arc::new(FireEngineCdp::with_url_and_proxy(
            engine_config.fire_cdp.url.clone(),
            proxy_url.to_string(),
        )));
    }

    engines
}

/// Initialize engine components including router and client.
///
/// This function combines engine initialization with router and client
/// creation.
///
/// # Arguments
///
/// * `proxy_url` - URL for the HTTP proxy
/// * `engine_config` - Engine-specific configuration
///
/// # Returns
///
/// Returns all engine components.
#[allow(deprecated)]
pub fn init_engine_components(
    proxy_url: &str,
    _engine_config: &EngineSettings,
) -> EngineComponents {
    let engines = init_engines(proxy_url, _engine_config);
    let router = Arc::new(EngineRouter::new(engines.clone()));
    let engine_client = Arc::new(EngineClient::with_router(router.clone()));

    EngineComponents {
        engines,
        router,
        engine_client,
    }
}
