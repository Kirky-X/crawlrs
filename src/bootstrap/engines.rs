// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scraper engines initialization and configuration.

use crate::config::engines::EngineSettings;
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
/// * `proxy_url` - URL for the HTTP proxy (if enabled)。`None` 表示未配置代理，
///   替代之前的空字符串 sentinel（架构 MEDIUM 5：API 语义明确化）
/// * `engine_config` - Engine-specific configuration settings
/// * `timeout_seconds` - 请求超时（秒），从 `settings.timeouts.engines.default_timeout_seconds`
///   注入 ReqwestEngine，避免硬编码 30 秒（架构 MEDIUM 2）
///
/// # Returns
///
/// Returns a vector of initialized engines.
#[allow(deprecated)]
#[allow(unused_variables)]
pub fn init_engines(
    http_client: Arc<reqwest::Client>,
    proxy_url: Option<&str>,
    engine_config: &EngineSettings,
    timeout_seconds: u64,
) -> Vec<Arc<dyn ScraperEngine>> {
    // 将 Option<&str> 转换为 ReqwestEngine::with_proxy_and_timeout 接受的 String。
    // None → 空字符串（ReqwestEngine 内部会将空字符串视为未配置代理）
    let proxy_url_str = proxy_url.unwrap_or("");
    #[allow(unused_mut)]
    let mut engines: Vec<Arc<dyn ScraperEngine>> =
        vec![Arc::new(ReqwestEngine::with_proxy_and_timeout(
            http_client.clone(),
            proxy_url_str.to_string(),
            timeout_seconds,
        ))];

    #[cfg(feature = "engine-playwright")]
    engines.push(Arc::new(PlaywrightEngine::new()));

    #[cfg(feature = "engine-flaresolverr")]
    if engine_config.fire_tls.enabled {
        log::info!(
            "Fire Engine TLS enabled with URL: {}",
            engine_config.fire_tls.url
        );
        engines.push(Arc::new(FlareSolverrEngine::with_tls_mode_and_url(
            http_client.clone(),
            &engine_config.fire_tls.url,
            proxy_url,
        )));
    }

    #[cfg(feature = "engine-flaresolverr")]
    if engine_config.fire_cdp.enabled {
        log::info!(
            "Fire Engine CDP enabled with URL: {}",
            engine_config.fire_cdp.url
        );
        engines.push(Arc::new(FlareSolverrEngine::with_cdp_mode_and_url(
            http_client.clone(),
            &engine_config.fire_cdp.url,
            proxy_url,
        )));
    }

    #[cfg(feature = "engine-flaresolverr")]
    if engine_config.flaresolverr.enabled {
        log::info!(
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
/// * `proxy_url` - URL for the HTTP proxy。`None` 表示未配置代理（架构 MEDIUM 5）
/// * `engine_config` - Engine-specific configuration
/// * `timeout_seconds` - 请求超时（秒），从 `settings.timeouts.engines.default_timeout_seconds`
///   注入 ReqwestEngine，避免硬编码 30 秒（架构 MEDIUM 2）
///
/// # Returns
///
/// Returns all engine components.
#[allow(deprecated)]
pub fn init_engine_components(
    http_client: Arc<reqwest::Client>,
    proxy_url: Option<String>,
    _engine_config: &EngineSettings,
    timeout_seconds: u64,
) -> EngineComponents {
    let engines = init_engines(
        http_client,
        proxy_url.as_deref(),
        _engine_config,
        timeout_seconds,
    );
    let router = Arc::new(EngineRouter::new(engines.clone()));
    let engine_client = Arc::new(EngineClient::with_router(router.clone()));

    EngineComponents {
        engines,
        router,
        engine_client,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_http_client() -> Arc<reqwest::Client> {
        Arc::new(reqwest::Client::new())
    }

    // ========== init_engines tests ==========

    #[test]
    fn test_init_engines_returns_non_empty_vec() {
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let engines = init_engines(
            http_client,
            Some("http://localhost:10808"),
            &engine_config,
            30,
        );
        assert!(
            !engines.is_empty(),
            "init_engines should return at least one engine"
        );
    }

    #[test]
    fn test_init_engines_default_contains_reqwest_engine() {
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let engines = init_engines(
            http_client,
            Some("http://localhost:10808"),
            &engine_config,
            30,
        );
        let engine_names: Vec<&str> = engines.iter().map(|e| e.name()).collect();
        assert!(
            engine_names.contains(&"reqwest"),
            "init_engines should include the reqwest engine by default, got: {:?}",
            engine_names
        );
    }

    #[test]
    fn test_init_engines_default_has_at_least_one_engine() {
        // With default features, only reqwest engine is available.
        // Other engines (playwright, flaresolverr) are behind feature flags.
        // flaresolverr 引擎通过 FlareSolverrMode 枚举区分 Full / Cdp / Tls 三种模式。
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let engines = init_engines(
            http_client,
            Some("http://localhost:10808"),
            &engine_config,
            30,
        );
        assert!(
            !engines.is_empty(),
            "Should have at least 1 engine with default features"
        );
    }

    #[test]
    fn test_init_engines_with_empty_proxy_url() {
        // Verify init_engines works with None proxy URL.
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let engines = init_engines(http_client, None, &engine_config, 30);
        assert!(!engines.is_empty());
    }

    // ========== init_engine_components tests ==========

    #[test]
    fn test_init_engine_components_populates_all_fields() {
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let components = init_engine_components(
            http_client,
            Some("http://localhost:10808".to_string()),
            &engine_config,
            30,
        );
        assert!(
            !components.engines.is_empty(),
            "engines vec should be non-empty"
        );
        // router and engine_client should be valid Arcs
        let _router = &components.router;
        let _engine_client = &components.engine_client;
    }

    #[test]
    fn test_init_engine_components_engines_non_empty() {
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let components = init_engine_components(
            http_client,
            Some("http://localhost:10808".to_string()),
            &engine_config,
            30,
        );
        assert!(
            !components.engines.is_empty(),
            "EngineComponents.engines should be non-empty"
        );
    }

    #[test]
    fn test_init_engine_components_router_created() {
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let components = init_engine_components(
            http_client,
            Some("http://localhost:10808".to_string()),
            &engine_config,
            30,
        );
        // The router should have registered engines matching the engines vec
        let registered = components.router.registered_engines();
        assert!(
            !registered.is_empty(),
            "router should have registered engines"
        );
    }

    #[test]
    fn test_init_engine_components_engine_client_created() {
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let components = init_engine_components(
            http_client,
            Some("http://localhost:10808".to_string()),
            &engine_config,
            30,
        );
        // EngineClient should report at least 1 engine
        assert!(
            components.engine_client.engine_count() >= 1,
            "engine_client should report at least 1 engine"
        );
    }

    #[test]
    fn test_init_engine_components_clone() {
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let components = init_engine_components(
            http_client,
            Some("http://localhost:10808".to_string()),
            &engine_config,
            30,
        );
        // EngineComponents derives Clone; verify clone produces equivalent field counts
        let cloned = components.clone();
        assert_eq!(components.engines.len(), cloned.engines.len());
    }
}
