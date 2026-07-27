// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scraper engines initialization and configuration.

use crate::config::engines::EngineSettings;
use crate::config::settings::ProxyStrategy;
#[cfg(feature = "engine-flaresolverr")]
use crate::engines::client::flare_solverr::FlareSolverrEngine;
#[cfg(feature = "engine-playwright")]
use crate::engines::client::playwright::PlaywrightEngine;
use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::EngineClient;
use crate::engines::engine_client::ScraperEngine;
use crate::engines::provider::ProxyProvider;
use crate::engines::router::EngineRouter;
use std::sync::Arc;
#[cfg(test)]
use crate::engines::proxy_pool::ProxyPool;

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
/// * `proxy_provider` - 代理提供者（design.md §12，T054/T056/R-identity-003 + H2 修复）。
///   `None` 时 ReqwestEngine 不使用代理；`Some` 时按 `proxy_strategy` 调度策略取代理。
///   `Arc<dyn ProxyProvider>` 可被多 worker 共享，并依赖抽象 trait 而非具体 `ProxyPool`。
/// * `proxy_strategy` - 代理调度策略（H1 修复：RoundRobin / Sticky）。
///   `proxy_provider` 为 `None` 时此参数被忽略。
/// * `proxy_url` - FlareSolverr 引擎使用的单代理 URL（FlareSolverr 服务自身访问代理）。
///   `None` 表示未配置代理。注意：FlareSolverr 暂未接入 ProxyProvider（T056 范围外）。
/// * `engine_config` - Engine-specific configuration settings
/// * `timeout_seconds` - 请求超时（秒），从 `settings.timeouts.engines.default_timeout_seconds`
///   注入 ReqwestEngine，避免硬编码 30 秒（架构 MEDIUM 2）
///
/// # Returns
///
/// Returns a vector of initialized engines.
#[allow(unused_variables)]
pub fn init_engines(
    http_client: Arc<reqwest::Client>,
    proxy_provider: Option<Arc<dyn ProxyProvider>>,
    proxy_strategy: ProxyStrategy,
    proxy_url: Option<&str>,
    engine_config: &EngineSettings,
    timeout_seconds: u64,
) -> Vec<Arc<dyn ScraperEngine>> {
    // T056/R-identity-003 + H1/H2 修复: ReqwestEngine 接入 ProxyProvider + 策略
    // - provider 为 Some 时 with_provider_strategy_and_timeout 注入
    // - provider 为 None 时 new_with_timeout（无代理）
    #[allow(unused_mut)]
    let mut engines: Vec<Arc<dyn ScraperEngine>> = match proxy_provider {
        Some(provider) => vec![Arc::new(ReqwestEngine::with_provider_strategy_and_timeout(
            http_client.clone(),
            provider,
            proxy_strategy,
            timeout_seconds,
        ))],
        None => vec![Arc::new(ReqwestEngine::new_with_timeout(
            http_client.clone(),
            timeout_seconds,
        ))],
    };

    #[cfg(feature = "engine-playwright")]
    engines.push(Arc::new(PlaywrightEngine::new()));

    #[cfg(feature = "engine-flaresolverr")]
    if engine_config.flaresolverr_tls.enabled {
        log::info!(
            "FlareSolverr TLS enabled with URL: {}",
            engine_config.flaresolverr_tls.url
        );
        engines.push(Arc::new(FlareSolverrEngine::with_tls_mode_and_url(
            http_client.clone(),
            &engine_config.flaresolverr_tls.url,
            proxy_url,
        )));
    }

    #[cfg(feature = "engine-flaresolverr")]
    if engine_config.flaresolverr_cdp.enabled {
        log::info!(
            "FlareSolverr CDP enabled with URL: {}",
            engine_config.flaresolverr_cdp.url
        );
        engines.push(Arc::new(FlareSolverrEngine::with_cdp_mode_and_url(
            http_client.clone(),
            &engine_config.flaresolverr_cdp.url,
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
/// * `proxy_provider` - 代理提供者（H2 修复：`Arc<dyn ProxyProvider>`）。
///   `None` 时不使用代理。
/// * `proxy_strategy` - 代理调度策略（H1 修复：RoundRobin / Sticky）。
/// * `proxy_url` - FlareSolverr 单代理 URL（`None` 表示未配置）
/// * `engine_config` - Engine-specific configuration
/// * `timeout_seconds` - 请求超时（秒），从 `settings.timeouts.engines.default_timeout_seconds`
///   注入 ReqwestEngine，避免硬编码 30 秒（架构 MEDIUM 2）
///
/// # Returns
///
/// Returns all engine components.
pub fn init_engine_components(
    http_client: Arc<reqwest::Client>,
    proxy_provider: Option<Arc<dyn ProxyProvider>>,
    proxy_strategy: ProxyStrategy,
    proxy_url: Option<String>,
    _engine_config: &EngineSettings,
    timeout_seconds: u64,
) -> EngineComponents {
    let engines = init_engines(
        http_client,
        proxy_provider,
        proxy_strategy,
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
    use std::time::Duration;

    fn make_http_client() -> Arc<reqwest::Client> {
        Arc::new(reqwest::Client::new())
    }

    fn make_proxy_provider() -> Arc<dyn ProxyProvider> {
        Arc::new(ProxyPool::from_urls(
            vec!["http://localhost:10808".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ))
    }

    // ========== init_engines tests ==========

    #[test]
    fn test_init_engines_returns_non_empty_vec() {
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let engines = init_engines(
            http_client,
            Some(make_proxy_provider()),
            ProxyStrategy::RoundRobin,
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
            Some(make_proxy_provider()),
            ProxyStrategy::RoundRobin,
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
            Some(make_proxy_provider()),
            ProxyStrategy::RoundRobin,
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
    fn test_init_engines_with_no_proxy_provider() {
        // Verify init_engines works with None proxy_provider (无代理路径)
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let engines = init_engines(
            http_client,
            None,
            ProxyStrategy::RoundRobin,
            None,
            &engine_config,
            30,
        );
        assert!(!engines.is_empty());
    }

    #[test]
    fn test_init_engines_with_sticky_strategy() {
        // H1 修复验证：Sticky 策略可正常构造 ReqwestEngine
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let engines = init_engines(
            http_client,
            Some(make_proxy_provider()),
            ProxyStrategy::Sticky,
            None,
            &engine_config,
            30,
        );
        assert!(!engines.is_empty());
    }

    // ========== init_engine_components tests ==========

    #[test]
    fn test_init_engine_components_populates_all_fields() {
        let http_client = make_http_client();
        let engine_config = EngineSettings::default();
        let components = init_engine_components(
            http_client,
            Some(make_proxy_provider()),
            ProxyStrategy::RoundRobin,
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
            Some(make_proxy_provider()),
            ProxyStrategy::RoundRobin,
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
            Some(make_proxy_provider()),
            ProxyStrategy::RoundRobin,
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
            Some(make_proxy_provider()),
            ProxyStrategy::RoundRobin,
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
            Some(make_proxy_provider()),
            ProxyStrategy::RoundRobin,
            Some("http://localhost:10808".to_string()),
            &engine_config,
            30,
        );
        // EngineComponents derives Clone; verify clone produces equivalent field counts
        let cloned = components.clone();
        assert_eq!(components.engines.len(), cloned.engines.len());
    }
}
