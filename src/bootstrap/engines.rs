// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scraper engines initialization and configuration.

use crate::config::engines::EngineSettings;
use crate::config::settings::{EngineTimeoutSettings, ProxyStrategy};
#[cfg(feature = "engine-flaresolverr")]
use crate::engines::client::flare_solverr::FlareSolverrEngine;
#[cfg(feature = "engine-playwright")]
use crate::engines::client::playwright::PlaywrightEngine;
use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::EngineClient;
use crate::engines::engine_client::ScraperEngine;
use crate::engines::provider::ProxyProvider;
#[cfg(test)]
use crate::engines::proxy_pool::ProxyPool;
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
/// * `proxy_provider` - 代理提供者（design.md §12，T054/T056/R-identity-003 + H2 修复）。
///   `None` 时 ReqwestEngine 不使用代理；`Some` 时按 `proxy_strategy` 调度策略取代理。
///   `Arc<dyn ProxyProvider>` 可被多 worker 共享，并依赖抽象 trait 而非具体 `ProxyPool`。
/// * `proxy_strategy` - 代理调度策略（H1 修复：RoundRobin / Sticky）。
///   `proxy_provider` 为 `None` 时此参数被忽略。
/// * `proxy_url` - FlareSolverr 引擎使用的单代理 URL（FlareSolverr 服务自身访问代理）。
///   `None` 表示未配置代理。注意：FlareSolverr 暂未接入 ProxyProvider（T056 范围外）。
/// * `engine_config` - Engine-specific configuration settings
/// * `engine_timeouts` - 引擎超时配置（T061：包含 `default_timeout_seconds` + 三个 MRT 字段
///   `fetch_seconds` / `tls_seconds` / `cdp_seconds`）。从此注入超时与 MRT，避免硬编码。
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
    engine_timeouts: &EngineTimeoutSettings,
) -> Vec<Arc<dyn ScraperEngine>> {
    // T061：从 EngineTimeoutSettings 派生超时与各引擎 MRT，避免硬编码
    let timeout_seconds = engine_timeouts.default_timeout_seconds;
    let fetch_mrt = std::time::Duration::from_secs(engine_timeouts.fetch_seconds);
    let tls_mrt = std::time::Duration::from_secs(engine_timeouts.tls_seconds);
    let cdp_mrt = std::time::Duration::from_secs(engine_timeouts.cdp_seconds);

    // T056/R-identity-003 + H1/H2 修复 + T060/T061：ReqwestEngine 接入 ProxyProvider + 策略 + MRT
    // - provider 为 Some 时 with_provider_strategy_timeout_and_mrt 注入
    // - provider 为 None 时 new_with_timeout_and_mrt（无代理）
    #[allow(unused_mut)]
    let mut engines: Vec<Arc<dyn ScraperEngine>> = match proxy_provider {
        Some(provider) => vec![Arc::new(
            ReqwestEngine::with_provider_strategy_timeout_and_mrt(
                http_client.clone(),
                provider,
                proxy_strategy,
                timeout_seconds,
                fetch_mrt,
            ),
        )],
        None => vec![Arc::new(ReqwestEngine::new_with_timeout_and_mrt(
            http_client.clone(),
            timeout_seconds,
            fetch_mrt,
        ))],
    };

    // T060/T061：PlaywrightEngine 注入 cdp_seconds 作为 MRT
    #[cfg(feature = "engine-playwright")]
    engines.push(Arc::new(PlaywrightEngine::with_mrt(cdp_mrt)));

    // T060/T061：FlareSolverrEngine 按模式注入对应 MRT
    // - Tls 模式 → tls_seconds
    // - Cdp / Full 模式 → cdp_seconds
    #[cfg(feature = "engine-flaresolverr")]
    if engine_config.flaresolverr_tls.enabled {
        log::info!(
            "FlareSolverr TLS enabled with URL: {}",
            engine_config.flaresolverr_tls.url
        );
        engines.push(Arc::new(FlareSolverrEngine::with_tls_mode_url_and_mrt(
            http_client.clone(),
            &engine_config.flaresolverr_tls.url,
            proxy_url,
            tls_mrt,
        )));
    }

    #[cfg(feature = "engine-flaresolverr")]
    if engine_config.flaresolverr_cdp.enabled {
        log::info!(
            "FlareSolverr CDP enabled with URL: {}",
            engine_config.flaresolverr_cdp.url
        );
        engines.push(Arc::new(FlareSolverrEngine::with_cdp_mode_url_and_mrt(
            http_client.clone(),
            &engine_config.flaresolverr_cdp.url,
            proxy_url,
            cdp_mrt,
        )));
    }

    #[cfg(feature = "engine-flaresolverr")]
    if engine_config.flaresolverr.enabled {
        log::info!(
            "FlareSolverr enabled with URL: {}",
            engine_config.flaresolverr.url
        );
        engines.push(Arc::new(FlareSolverrEngine::with_url_and_mrt(
            http_client.clone(),
            &engine_config.flaresolverr.url,
            cdp_mrt,
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
/// * `engine_timeouts` - 引擎超时配置（T061：包含 `default_timeout_seconds` + 三个 MRT 字段）。
///   从此注入超时与 MRT，避免硬编码。
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
    engine_timeouts: &EngineTimeoutSettings,
) -> EngineComponents {
    let engines = init_engines(
        http_client,
        proxy_provider,
        proxy_strategy,
        proxy_url.as_deref(),
        _engine_config,
        engine_timeouts,
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

    /// T061：构造默认 EngineTimeoutSettings（30s/30s/30s + MRT 5s/15s/30s）
    fn make_engine_timeouts() -> EngineTimeoutSettings {
        EngineTimeoutSettings {
            default_timeout_seconds: 30,
            playwright_timeout_seconds: 30,
            flaresolverr_timeout_seconds: 30,
            fetch_seconds: 5,
            tls_seconds: 15,
            cdp_seconds: 30,
        }
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
            &make_engine_timeouts(),
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
            &make_engine_timeouts(),
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
            &make_engine_timeouts(),
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
            &make_engine_timeouts(),
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
            &make_engine_timeouts(),
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
            &make_engine_timeouts(),
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
            &make_engine_timeouts(),
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
            &make_engine_timeouts(),
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
            &make_engine_timeouts(),
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
            &make_engine_timeouts(),
        );
        // EngineComponents derives Clone; verify clone produces equivalent field counts
        let cloned = components.clone();
        assert_eq!(components.engines.len(), cloned.engines.len());
    }
}
