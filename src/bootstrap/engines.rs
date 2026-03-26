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
#[cfg(feature = "engine-flaresolverr")]
use crate::engines::client::flare_solverr::FlareSolverrEngine;
#[cfg(feature = "engine-playwright")]
use crate::engines::client::playwright::PlaywrightEngine;
use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::EngineClient;
use crate::engines::engine_client::ScraperEngine;
use crate::engines::router::EngineRouter;
use std::sync::Arc;

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
/// * `http_client` - Shared HTTP client
/// * `proxy_url` - URL for the HTTP proxy (if enabled)
/// * `engine_config` - Engine-specific configuration settings
///
/// # Returns
///
/// Returns a vector of initialized engines.
#[allow(deprecated)]
#[allow(unused_variables)]
pub fn init_engines(
    http_client: Arc<reqwest::Client>,
    proxy_url: &str,
    engine_config: &EngineSettings,
) -> Vec<Arc<dyn ScraperEngine>> {
    #[allow(unused_mut)]
    let mut engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(ReqwestEngine::with_proxy(
        http_client.clone(),
        proxy_url.to_string(),
    ))];

    #[cfg(feature = "engine-playwright")]
    engines.push(Arc::new(PlaywrightEngine::new()));

    #[cfg(feature = "engine-fire-tls")]
    if engine_config.fire_tls.enabled {
        tracing::info!(
            "Fire Engine TLS enabled with URL: {}",
            engine_config.fire_tls.url
        );
        engines.push(Arc::new(FireEngineTls::with_url_and_proxy(
            http_client.clone(),
            &engine_config.fire_tls.url,
            Some(proxy_url),
        )));
    }

    #[cfg(feature = "engine-fire-cdp")]
    if engine_config.fire_cdp.enabled {
        tracing::info!(
            "Fire Engine CDP enabled with URL: {}",
            engine_config.fire_cdp.url
        );
        engines.push(Arc::new(FireEngineCdp::with_url_and_proxy(
            http_client.clone(),
            &engine_config.fire_cdp.url,
            Some(proxy_url),
        )));
    }

    #[cfg(feature = "engine-flaresolverr")]
    if engine_config.flaresolverr.enabled {
        tracing::info!(
            "FlareSolverr enabled with URL: {}",
            engine_config.flaresolverr.url
        );
        engines.push(Arc::new(FlareSolverrEngine::with_url(
            http_client.clone(),
            &engine_config.flaresolverr.url,
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
/// * `http_client` - Shared HTTP client
/// * `proxy_url` - URL for the HTTP proxy
/// * `engine_config` - Engine-specific configuration
///
/// # Returns
///
/// Returns all engine components.
#[allow(deprecated)]
pub fn init_engine_components(
    http_client: Arc<reqwest::Client>,
    proxy_url: String,
    _engine_config: &EngineSettings,
) -> EngineComponents {
    let engines = init_engines(http_client, &proxy_url, _engine_config);
    let router = Arc::new(EngineRouter::new(engines.clone()));
    let engine_client = Arc::new(EngineClient::with_router(router.clone()));

    EngineComponents {
        engines,
        router,
        engine_client,
    }
}
