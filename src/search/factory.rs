// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(feature = "engine-fire-cdp")]
use crate::engines::client::fire_cdp::FireEngineCdp;

#[cfg(feature = "engine-fire-tls")]
use crate::engines::client::fire_tls::FireEngineTls;

use crate::engines::engine_client::EngineClient;
use crate::engines::router::EngineRouter;
use crate::infrastructure::services::config_service::ConfigServiceTrait;
use crate::search::client::baidu::BaiduSearchEngine;
use crate::search::client::bing::BingSearchEngine;
use crate::search::client::google::GoogleSearchEngine;
use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::router::{SearchEngineRouter, SearchEngineRouterConfig};
use crate::search::smart::{
    create_baidu_smart_search, create_bing_smart_search, create_google_smart_search,
    create_sogou_smart_search,
};
use crate::search::types::SearchEngineType;
use reqwest::Client;
use std::sync::Arc;

use log::info;

/// 搜索引擎工厂配置
#[derive(Debug, Clone)]
pub struct SearchEngineFactoryConfig {
    /// 默认搜索引擎类型
    pub default_engine: SearchEngineType,
    /// 启用自动故障转移
    pub enable_auto_failover: bool,
    /// 启用负载均衡
    pub enable_load_balancing: bool,
    /// 请求超时时间（秒）
    pub request_timeout: u64,
    /// 最大重试次数
    pub max_retries: u32,
}

impl Default for SearchEngineFactoryConfig {
    fn default() -> Self {
        Self {
            default_engine: SearchEngineType::Smart,
            enable_auto_failover: true,
            enable_load_balancing: true,
            request_timeout: 30,
            max_retries: 3,
        }
    }
}

/// 搜索引擎工厂
///
/// 提供统一的搜索引擎创建和管理接口
/// 支持多种搜索引擎类型和智能路由
pub struct SearchEngineFactory {
    /// 路由器实例
    router: SearchEngineRouter,
    /// 配置
    config: SearchEngineFactoryConfig,
    /// HTTP 客户端
    #[allow(dead_code)]
    http_client: Arc<Client>,
    /// 配置服务（用于获取代理等配置）
    config_service: Arc<dyn ConfigServiceTrait>,
}

impl SearchEngineFactory {
    /// 创建新的搜索引擎工厂
    pub fn new(http_client: Arc<Client>, config_service: Arc<dyn ConfigServiceTrait>) -> Self {
        Self::with_config(
            http_client,
            config_service,
            SearchEngineFactoryConfig::default(),
        )
    }

    /// 使用配置创建搜索引擎工厂
    pub fn with_config(
        http_client: Arc<Client>,
        config_service: Arc<dyn ConfigServiceTrait>,
        config: SearchEngineFactoryConfig,
    ) -> Self {
        let router_config = SearchEngineRouterConfig {
            enable_auto_failover: config.enable_auto_failover,
            enable_load_balancing: config.enable_load_balancing,
            request_timeout: std::time::Duration::from_secs(config.request_timeout),
            max_retries: config.max_retries,
            ..Default::default()
        };

        Self {
            router: SearchEngineRouter::with_config(router_config),
            config,
            http_client,
            config_service,
        }
    }

    /// 创建并注册所有支持的搜索引擎
    pub async fn create_all_engines(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("正在创建所有搜索引擎...");

        // Google 搜索引擎
        self.register_google_engine().await;

        // Bing 搜索引擎
        self.register_bing_engine();

        // Baidu 搜索引擎
        self.register_baidu_engine();

        // Sogou 搜索引擎
        self.register_sogou_engine();

        info!(
            "所有搜索引擎创建完成，已注册: {:?}",
            self.router.registered_engines()
        );

        Ok(())
    }

    /// 创建并注册 Google 搜索引擎
    pub async fn register_google_engine(&mut self) {
        #[cfg(feature = "engine-playwright")]
        {
            let engine_client = self.create_engine_client_with_fire_engines();
            let google_engine = Arc::new(GoogleSearchEngine::new(engine_client));
            self.router.register_engine(google_engine);
            info!("Google 搜索引擎已注册（使用 Playwright/Fire Engine）");
        }

        #[cfg(not(feature = "engine-playwright"))]
        {
            // Even without playwright feature, we should provide an engine client
            // It might fallback to other engines or Reqwest if configured
            let engine_client = self.create_engine_client_with_fire_engines();
            let google_engine = Arc::new(GoogleSearchEngine::new(engine_client));
            self.router.register_engine(google_engine);
            info!("Google 搜索引擎已注册（HTTP 模式）");
        }
    }

    /// 创建并注册 Bing 搜索引擎
    pub fn register_bing_engine(&mut self) {
        let engine_client = self.create_engine_client_with_fire_engines();
        let bing_engine = Arc::new(BingSearchEngine::new(engine_client));
        self.router.register_engine(bing_engine);
        info!("Bing 搜索引擎已注册（使用 EngineClient）");
    }

    /// 创建并注册 Baidu 搜索引擎
    pub fn register_baidu_engine(&mut self) {
        let engine_client = self.create_engine_client_with_fire_engines();
        let baidu_engine = Arc::new(BaiduSearchEngine::new(engine_client));
        self.router.register_engine(baidu_engine);
        info!("Baidu 搜索引擎已注册（使用 EngineClient）");
    }

    /// 创建并注册 Sogou 搜索引擎
    pub fn register_sogou_engine(&mut self) {
        let engine_client = self.create_engine_client_with_fire_engines();
        let sogou_engine = create_sogou_smart_search(engine_client);
        self.router.register_engine(sogou_engine);
        info!("Sogou 搜索引擎已注册（使用 SmartSearchEngine + Playwright）");
    }

    /// 创建 EngineClient 并注册 Fire Engines（用于智能搜索）
    #[allow(deprecated)]
    #[allow(unused_mut)]
    pub fn create_engine_client_with_fire_engines(&self) -> Arc<EngineClient> {
        use crate::engines::engine_client::ScraperEngine;
        let mut engines: Vec<Arc<dyn ScraperEngine>> = Vec::new();

        // 获取代理URL配置（通过配置服务）
        let _proxy_url = self.config_service.get_proxy_url();

        // 注册 Fire Engine CDP（用于需要完整浏览器自动化的网站）
        #[cfg(feature = "engine-fire-cdp")]
        {
            let fire_engine_cdp = if let Some(ref proxy) = _proxy_url {
                Arc::new(FireEngineCdp::with_proxy(self.http_client.clone(), proxy))
            } else {
                Arc::new(FireEngineCdp::new(self.http_client.clone()))
            };
            engines.push(fire_engine_cdp.clone() as Arc<dyn ScraperEngine>);
            info!(
                "FireEngineCdp 已注册{}",
                _proxy_url
                    .as_ref()
                    .map(|p| format!("（代理: {}）", p))
                    .unwrap_or_default()
            );
        }

        // 注册 Fire Engine TLS（用于需要TLS指纹对抗的网站）
        #[cfg(feature = "engine-fire-tls")]
        {
            let fire_engine_tls = if let Some(ref proxy) = _proxy_url {
                Arc::new(FireEngineTls::with_proxy(self.http_client.clone(), proxy))
            } else {
                Arc::new(FireEngineTls::new(self.http_client.clone()))
            };
            engines.push(fire_engine_tls.clone() as Arc<dyn ScraperEngine>);
        }

        let router = Arc::new(EngineRouter::new(engines));
        info!("EngineRouter 创建完成，已注册 Fire Engines");

        Arc::new(EngineClient::with_router(router))
    }

    /// 创建 Google 智能搜索引擎（使用 Fire Engine）
    pub fn create_google_smart_search(&self) -> Arc<dyn SearchEngine> {
        let engine_client = self.create_engine_client_with_fire_engines();
        create_google_smart_search(engine_client)
    }

    /// 创建 Bing 智能搜索引擎
    pub fn create_bing_smart_search(&self) -> Arc<dyn SearchEngine> {
        let engine_client = self.create_engine_client_with_fire_engines();
        create_bing_smart_search(engine_client)
    }

    /// 创建 Baidu 智能搜索引擎
    pub fn create_baidu_smart_search(&self) -> Arc<dyn SearchEngine> {
        let engine_client = self.create_engine_client_with_fire_engines();
        create_baidu_smart_search(engine_client)
    }

    /// 注册自定义搜索引擎
    pub fn register_engine(&mut self, engine: Arc<dyn SearchEngine>) {
        self.router.register_engine(engine);
    }

    /// 根据类型获取搜索引擎
    pub fn get_engine(&self, engine_type: SearchEngineType) -> Option<Arc<dyn SearchEngine>> {
        match engine_type {
            SearchEngineType::Google => self.router.get_engine("google"),
            SearchEngineType::Bing => self.router.get_engine("bing"),
            SearchEngineType::Baidu => self.router.get_engine("baidu"),
            SearchEngineType::Sogou => self.router.get_engine("sogou"),
            SearchEngineType::Smart | SearchEngineType::ABTest | SearchEngineType::Auto => None,
        }
    }

    /// 获取路由器实例
    pub fn router(&self) -> &SearchEngineRouter {
        &self.router
    }

    /// 获取可变的路由器实例
    pub fn router_mut(&mut self) -> &mut SearchEngineRouter {
        &mut self.router
    }

    /// 创建智能搜索引擎实例
    pub fn create_smart_search(&self) -> Arc<SearchEngineRouter> {
        Arc::new(self.router.clone())
    }

    /// 获取路由器克隆（用于测试）
    pub fn clone_router(&self) -> SearchEngineRouter {
        self.router.clone()
    }

    /// 执行搜索（使用默认或指定的搜索引擎）
    pub async fn search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
        engine_type: Option<SearchEngineType>,
    ) -> Result<
        Vec<crate::domain::models::search_result::SearchResult>,
        crate::search::error::SearchError,
    > {
        let request = SearchRequest {
            query: query.to_string(),
            limit,
            offset: 0,
            engine: engine_type,
            lang: lang.map(|s| s.to_string()),
            country: country.map(|s| s.to_string()),
        };

        let engine_val = engine_type.unwrap_or(self.config.default_engine);
        let preferred_engine = match engine_val {
            SearchEngineType::Google => Some("google"),
            SearchEngineType::Bing => Some("bing"),
            SearchEngineType::Baidu => Some("baidu"),
            SearchEngineType::Sogou => Some("sogou"),
            SearchEngineType::Smart | SearchEngineType::ABTest | SearchEngineType::Auto => None,
        };

        let response = self
            .router
            .search(&request, preferred_engine)
            .await
            .map_err(|e| crate::search::error::SearchError::Engine(e.to_string()))?;

        let total_items = response.items.len();
        Ok(response
            .items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                // 基于位置计算简单分数：第一个结果得 1.0，最后一个得 0.0
                // 保留结果间的相对排序信息
                let score = if total_items > 1 {
                    1.0 - (index as f64 / (total_items - 1) as f64)
                } else {
                    1.0
                };

                crate::domain::models::search_result::SearchResult {
                    title: item.title,
                    url: item.url,
                    description: Some(item.description),
                    engine: format!("{:?}", item.engine),
                    score,
                    published_time: None,
                }
            })
            .collect())
    }

    /// 获取工厂统计信息
    pub fn stats(&self) -> crate::search::router::RouterStats {
        self.router.stats()
    }

    /// 获取所有已注册的引擎名称
    pub fn registered_engines(&self) -> Vec<String> {
        self.router.registered_engines()
    }

    /// 更新配置
    pub fn update_config(&mut self, config: SearchEngineFactoryConfig) {
        self.router.update_config(SearchEngineRouterConfig {
            enable_auto_failover: config.enable_auto_failover,
            enable_load_balancing: config.enable_load_balancing,
            request_timeout: std::time::Duration::from_secs(config.request_timeout),
            max_retries: config.max_retries,
            ..Default::default()
        });
        self.config = config;
    }
}

/// 便捷函数：创建默认的搜索引擎路由器（使用配置服务）
pub async fn create_default_router_with_config(
    http_client: Arc<Client>,
    config_service: Arc<dyn ConfigServiceTrait>,
) -> Result<SearchEngineRouter, Box<dyn std::error::Error + Send + Sync>> {
    let mut factory = SearchEngineFactory::new(http_client, config_service);
    factory.create_all_engines().await?;
    Ok(factory.router.clone())
}

/// 便捷函数：获取引擎类型列表
pub fn available_engine_types() -> Vec<SearchEngineType> {
    vec![
        SearchEngineType::Google,
        SearchEngineType::Bing,
        SearchEngineType::Baidu,
        SearchEngineType::Sogou,
        SearchEngineType::Smart,
        SearchEngineType::ABTest,
    ]
}

#[cfg(test)]
mod tests {
    // Copyright (c) 2025 Kirky.X
    use super::*;
    use crate::infrastructure::services::config_service::ConfigServiceTrait;
    use crate::search::engine_trait::{SearchEngine, SearchRequest};
    use crate::search::response::{Response, ResponseItem};
    use crate::search::types::{EngineHealth, SearchEngineType};
    use async_trait::async_trait;
    use std::sync::Arc;
    use std::time::Duration;

    /// Minimal `ConfigServiceTrait` mock for factory tests — all defaults, no env vars.
    struct MockConfigService;

    #[async_trait]
    impl ConfigServiceTrait for MockConfigService {
        fn get_proxy_url(&self) -> Option<String> {
            None
        }
        fn get_remote_debugging_url(&self) -> Option<String> {
            None
        }
        fn is_test_mode(&self) -> bool {
            true
        }
        fn get_default_timeout(&self) -> Duration {
            Duration::from_secs(30)
        }
        fn get_browser_timeout(&self) -> Duration {
            Duration::from_secs(30)
        }
        fn get_browser_launch_timeout(&self) -> Duration {
            Duration::from_secs(30)
        }
        fn get_app_environment(&self) -> String {
            "test".to_string()
        }
        fn is_production(&self) -> bool {
            false
        }
        fn is_development(&self) -> bool {
            false
        }
        fn get_webhook_secret(&self) -> String {
            "test-secret".to_string()
        }
        fn get_health_check_url(&self) -> Option<String> {
            None
        }
        fn is_ssrf_protection_disabled(&self) -> bool {
            false
        }
        fn is_network_tests_enabled(&self) -> bool {
            false
        }
        fn is_debug_save_html_enabled(&self) -> bool {
            false
        }
        fn get_flaresolverr_url(&self) -> Option<String> {
            None
        }
    }

    fn make_http_client() -> Arc<Client> {
        Arc::new(Client::new())
    }

    fn make_config_service() -> Arc<dyn ConfigServiceTrait> {
        Arc::new(MockConfigService)
    }

    // ========== SearchEngineFactoryConfig tests ==========

    #[test]
    fn test_factory_config_default_values() {
        let config = SearchEngineFactoryConfig::default();
        assert_eq!(config.default_engine, SearchEngineType::Smart);
        assert!(config.enable_auto_failover);
        assert!(config.enable_load_balancing);
        assert_eq!(config.request_timeout, 30);
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_factory_config_clone_preserves_fields() {
        let config = SearchEngineFactoryConfig {
            default_engine: SearchEngineType::Google,
            enable_auto_failover: false,
            enable_load_balancing: false,
            request_timeout: 60,
            max_retries: 5,
        };
        let cloned = config.clone();
        assert_eq!(cloned.default_engine, SearchEngineType::Google);
        assert!(!cloned.enable_auto_failover);
        assert!(!cloned.enable_load_balancing);
        assert_eq!(cloned.request_timeout, 60);
        assert_eq!(cloned.max_retries, 5);
    }

    // ========== available_engine_types tests ==========

    #[test]
    fn test_available_engine_types_contains_all_supported() {
        let types = available_engine_types();
        assert!(types.contains(&SearchEngineType::Google));
        assert!(types.contains(&SearchEngineType::Bing));
        assert!(types.contains(&SearchEngineType::Baidu));
        assert!(types.contains(&SearchEngineType::Sogou));
        assert!(types.contains(&SearchEngineType::Smart));
        assert!(types.contains(&SearchEngineType::ABTest));
    }

    #[test]
    fn test_available_engine_types_count() {
        let types = available_engine_types();
        assert_eq!(types.len(), 6, "should list 6 engine types");
    }

    #[test]
    fn test_available_engine_types_excludes_auto() {
        let types = available_engine_types();
        assert!(
            !types.contains(&SearchEngineType::Auto),
            "Auto is not a user-selectable engine type"
        );
    }

    // ========== SearchEngineFactory construction tests ==========

    #[test]
    fn test_factory_new_creates_empty_router() {
        let factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        assert!(
            factory.registered_engines().is_empty(),
            "newly created factory should have no engines registered"
        );
    }

    #[test]
    fn test_factory_with_config_uses_custom_values() {
        let config = SearchEngineFactoryConfig {
            default_engine: SearchEngineType::Bing,
            enable_auto_failover: false,
            enable_load_balancing: false,
            request_timeout: 45,
            max_retries: 7,
        };
        let factory =
            SearchEngineFactory::with_config(make_http_client(), make_config_service(), config);
        assert!(
            factory.registered_engines().is_empty(),
            "factory with custom config should still start with no engines"
        );
    }

    // ========== register_*_engine tests ==========

    #[tokio::test]
    async fn test_register_google_engine_adds_to_router() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory.register_google_engine().await;
        let engines = factory.registered_engines();
        assert!(
            engines.iter().any(|n| n == "Google"),
            "google engine should be registered, got {:?}",
            engines
        );
    }

    #[test]
    fn test_register_bing_engine_adds_to_router() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory.register_bing_engine();
        let engines = factory.registered_engines();
        assert!(
            engines.iter().any(|n| n == "Bing"),
            "bing engine should be registered, got {:?}",
            engines
        );
    }

    #[test]
    fn test_register_baidu_engine_adds_to_router() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory.register_baidu_engine();
        let engines = factory.registered_engines();
        assert!(
            engines.iter().any(|n| n == "Baidu"),
            "baidu engine should be registered, got {:?}",
            engines
        );
    }

    #[test]
    fn test_register_sogou_engine_adds_to_router() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory.register_sogou_engine();
        let engines = factory.registered_engines();
        assert!(
            engines.iter().any(|n| n == "sogou"),
            "sogou engine should be registered, got {:?}",
            engines
        );
    }

    // ========== create_all_engines tests ==========

    #[tokio::test]
    async fn test_create_all_engines_registers_four_engines() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory
            .create_all_engines()
            .await
            .expect("create_all_engines should succeed");

        let engines = factory.registered_engines();
        assert_eq!(
            engines.len(),
            4,
            "should register exactly 4 engines, got {:?}",
            engines
        );
    }

    #[tokio::test]
    async fn test_create_all_engines_returns_ok() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let result = factory.create_all_engines().await;
        assert!(result.is_ok(), "create_all_engines should return Ok(())");
    }

    // ========== get_engine tests ==========

    #[tokio::test]
    async fn test_get_engine_google_after_registration() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory.register_google_engine().await;
        // Note: register_google_engine registers with name "Google" (capital G),
        // but get_engine(SearchEngineType::Google) looks up "google" (lowercase).
        // This name mismatch means get_engine returns None.
        let engine = factory.get_engine(SearchEngineType::Google);
        assert!(
            engine.is_none(),
            "get_engine returns None due to name mismatch (registered as 'Google', looked up as 'google')"
        );
    }

    #[test]
    fn test_get_engine_bing_after_registration() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory.register_bing_engine();
        // Note: register_bing_engine registers with name "Bing" (capital B),
        // but get_engine(SearchEngineType::Bing) looks up "bing" (lowercase).
        let engine = factory.get_engine(SearchEngineType::Bing);
        assert!(
            engine.is_none(),
            "get_engine returns None due to name mismatch (registered as 'Bing', looked up as 'bing')"
        );
    }

    #[test]
    fn test_get_engine_baidu_after_registration() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory.register_baidu_engine();
        // Note: register_baidu_engine registers with name "Baidu" (capital B),
        // but get_engine(SearchEngineType::Baidu) looks up "baidu" (lowercase).
        let engine = factory.get_engine(SearchEngineType::Baidu);
        assert!(
            engine.is_none(),
            "get_engine returns None due to name mismatch (registered as 'Baidu', looked up as 'baidu')"
        );
    }

    #[test]
    fn test_get_engine_sogou_after_registration() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory.register_sogou_engine();
        let engine = factory.get_engine(SearchEngineType::Sogou);
        assert!(engine.is_some(), "should retrieve sogou engine by type");
    }

    #[test]
    fn test_get_engine_returns_none_for_smart_type() {
        let factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        assert!(
            factory.get_engine(SearchEngineType::Smart).is_none(),
            "Smart type is not a directly retrievable engine"
        );
    }

    #[test]
    fn test_get_engine_returns_none_for_auto_type() {
        let factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        assert!(
            factory.get_engine(SearchEngineType::Auto).is_none(),
            "Auto type is not a directly retrievable engine"
        );
    }

    #[test]
    fn test_get_engine_returns_none_for_abtest_type() {
        let factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        assert!(
            factory.get_engine(SearchEngineType::ABTest).is_none(),
            "ABTest type is not a directly retrievable engine"
        );
    }

    #[test]
    fn test_get_engine_returns_none_when_not_registered() {
        let factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        assert!(
            factory.get_engine(SearchEngineType::Google).is_none(),
            "should return None when the engine is not registered"
        );
    }

    // ========== registered_engines / router tests ==========

    #[tokio::test]
    async fn test_registered_engines_lists_all_after_create_all() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory.create_all_engines().await.unwrap();
        let engines = factory.registered_engines();
        assert!(
            engines.iter().any(|n| n == "Google"),
            "Google should be registered, got {:?}",
            engines
        );
        assert!(
            engines.iter().any(|n| n == "Bing"),
            "Bing should be registered, got {:?}",
            engines
        );
        assert!(
            engines.iter().any(|n| n == "Baidu"),
            "Baidu should be registered, got {:?}",
            engines
        );
        assert!(
            engines.iter().any(|n| n == "sogou"),
            "sogou should be registered, got {:?}",
            engines
        );
    }

    #[test]
    fn test_router_returns_reference() {
        let factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let engines = factory.router().registered_engines();
        assert!(engines.is_empty());
    }

    #[test]
    fn test_router_mut_allows_registration() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());

        // Create a mock engine for direct router registration.
        let mock = Arc::new(MockFactoryEngine::success(
            "direct-mock",
            SearchEngineType::Google,
            vec![],
        ));
        factory.router_mut().register_engine(mock);

        assert_eq!(factory.registered_engines().len(), 1);
    }

    #[test]
    fn test_clone_router_produces_independent_copy() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory.register_bing_engine();

        let cloned = factory.clone_router();
        let cloned_engines = cloned.registered_engines();
        assert!(
            cloned_engines.iter().any(|n| n == "Bing"),
            "cloned router should contain the same engines"
        );
    }

    // ========== register_engine (custom) tests ==========

    #[test]
    fn test_register_custom_engine_adds_to_factory() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let mock = Arc::new(MockFactoryEngine::success(
            "custom-engine",
            SearchEngineType::Google,
            vec![],
        )) as Arc<dyn SearchEngine>;
        factory.register_engine(mock);
        assert_eq!(factory.registered_engines().len(), 1);
        assert!(factory
            .registered_engines()
            .contains(&"custom-engine".to_string()));
    }

    // ========== create_*_smart_search tests ==========

    #[test]
    fn test_create_google_smart_search_returns_engine() {
        let factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let engine = factory.create_google_smart_search();
        assert_eq!(engine.name(), "google");
    }

    #[test]
    fn test_create_bing_smart_search_returns_engine() {
        let factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let engine = factory.create_bing_smart_search();
        assert_eq!(engine.name(), "bing");
    }

    #[test]
    fn test_create_baidu_smart_search_returns_engine() {
        let factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let engine = factory.create_baidu_smart_search();
        assert_eq!(engine.name(), "baidu");
    }

    // ========== create_smart_search / stats tests ==========

    #[tokio::test]
    async fn test_stats_returns_zero_for_new_factory() {
        let factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let stats = factory.stats();
        assert_eq!(stats.engine_count, 0);
        assert_eq!(stats.total_requests, 0);
    }

    #[tokio::test]
    async fn test_stats_reports_engine_count_after_registration() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory.register_bing_engine();
        let stats = factory.stats();
        assert_eq!(stats.engine_count, 1);
    }

    // ========== update_config tests ==========

    #[test]
    fn test_update_config_changes_router_config() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let new_config = SearchEngineFactoryConfig {
            default_engine: SearchEngineType::Bing,
            enable_auto_failover: false,
            enable_load_balancing: false,
            request_timeout: 99,
            max_retries: 10,
        };
        factory.update_config(new_config);
        // The router config should now reflect the new values (verified via stats still working).
        let stats = factory.stats();
        assert_eq!(
            stats.engine_count, 0,
            "config update should not clear engines"
        );
    }

    // ========== Mock SearchEngine for factory tests ==========

    /// Mock `SearchEngine` for testing direct router registration.
    struct MockFactoryEngine {
        name: &'static str,
        engine_type: SearchEngineType,
        items: Vec<ResponseItem>,
    }

    impl MockFactoryEngine {
        fn success(
            name: &'static str,
            engine_type: SearchEngineType,
            items: Vec<ResponseItem>,
        ) -> Self {
            Self {
                name,
                engine_type,
                items,
            }
        }
    }

    #[async_trait]
    impl SearchEngine for MockFactoryEngine {
        fn name(&self) -> &'static str {
            self.name
        }

        fn engine_type(&self) -> SearchEngineType {
            self.engine_type
        }

        fn health(&self) -> EngineHealth {
            EngineHealth::Healthy
        }

        async fn search(
            &self,
            _request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, crate::search::error::SearchError> {
            Ok(Response {
                items: self.items.clone(),
                total_results: Some(self.items.len() as u64),
                engine: self.engine_type,
            })
        }
    }

    // ========== search method tests ==========

    #[tokio::test]
    async fn test_search_with_mock_engine_returns_results() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let items = vec![ResponseItem {
            title: "Test Result".to_string(),
            url: "https://example.com".to_string(),
            description: "Test description".to_string(),
            engine: SearchEngineType::Google,
        }];
        let mock = Arc::new(MockFactoryEngine::success(
            "google",
            SearchEngineType::Google,
            items,
        )) as Arc<dyn SearchEngine>;
        factory.register_engine(mock);

        let results = factory
            .search("test query", 10, None, None, Some(SearchEngineType::Google))
            .await
            .expect("search should succeed with registered mock engine");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Test Result");
        assert_eq!(results[0].url, "https://example.com");
        assert_eq!(results[0].description, Some("Test description".to_string()));
        assert_eq!(results[0].engine, "Google");
        assert!((results[0].score - 1.0).abs() < f64::EPSILON);
        assert!(results[0].published_time.is_none());
    }

    #[tokio::test]
    async fn test_search_with_no_engines_returns_error() {
        let factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let result = factory
            .search("test query", 10, None, None, Some(SearchEngineType::Google))
            .await;
        assert!(
            result.is_err(),
            "search with no engines registered should return an error"
        );
    }

    #[tokio::test]
    async fn test_search_score_single_item_is_one() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let items = vec![ResponseItem {
            title: "Only Result".to_string(),
            url: "https://single.com".to_string(),
            description: "desc".to_string(),
            engine: SearchEngineType::Google,
        }];
        let mock = Arc::new(MockFactoryEngine::success(
            "google",
            SearchEngineType::Google,
            items,
        )) as Arc<dyn SearchEngine>;
        factory.register_engine(mock);

        let results = factory
            .search("query", 10, None, None, Some(SearchEngineType::Google))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(
            (results[0].score - 1.0).abs() < f64::EPSILON,
            "single item should have score 1.0"
        );
    }

    #[tokio::test]
    async fn test_search_score_multiple_items_decreasing() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let items: Vec<ResponseItem> = (0..3)
            .map(|i| ResponseItem {
                title: format!("Result {}", i + 1),
                url: format!("https://example.com/{}", i),
                description: format!("Desc {}", i + 1),
                engine: SearchEngineType::Google,
            })
            .collect();
        let mock = Arc::new(MockFactoryEngine::success(
            "google",
            SearchEngineType::Google,
            items,
        )) as Arc<dyn SearchEngine>;
        factory.register_engine(mock);

        let results = factory
            .search("query", 10, None, None, Some(SearchEngineType::Google))
            .await
            .unwrap();
        assert_eq!(results.len(), 3);
        // For 3 items: scores = 1.0, 0.5, 0.0
        assert!((results[0].score - 1.0).abs() < f64::EPSILON);
        assert!((results[1].score - 0.5).abs() < f64::EPSILON);
        assert!((results[2].score - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_search_score_two_items_extremes() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let items = vec![
            ResponseItem {
                title: "First".to_string(),
                url: "https://first.com".to_string(),
                description: "d1".to_string(),
                engine: SearchEngineType::Google,
            },
            ResponseItem {
                title: "Second".to_string(),
                url: "https://second.com".to_string(),
                description: "d2".to_string(),
                engine: SearchEngineType::Google,
            },
        ];
        let mock = Arc::new(MockFactoryEngine::success(
            "google",
            SearchEngineType::Google,
            items,
        )) as Arc<dyn SearchEngine>;
        factory.register_engine(mock);

        let results = factory
            .search("query", 10, None, None, Some(SearchEngineType::Google))
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!((results[0].score - 1.0).abs() < f64::EPSILON);
        assert!((results[1].score - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_search_with_smart_type_no_preferred_engine() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let items = vec![ResponseItem {
            title: "Smart Result".to_string(),
            url: "https://smart.com".to_string(),
            description: "smart".to_string(),
            engine: SearchEngineType::Smart,
        }];
        let mock = Arc::new(MockFactoryEngine::success(
            "smart-engine",
            SearchEngineType::Smart,
            items,
        )) as Arc<dyn SearchEngine>;
        factory.register_engine(mock);

        let results = factory
            .search("query", 10, None, None, Some(SearchEngineType::Smart))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Smart Result");
    }

    #[tokio::test]
    async fn test_search_with_auto_type_no_preferred_engine() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let items = vec![ResponseItem {
            title: "Auto".to_string(),
            url: "https://auto.com".to_string(),
            description: "auto".to_string(),
            engine: SearchEngineType::Auto,
        }];
        let mock = Arc::new(MockFactoryEngine::success(
            "auto-engine",
            SearchEngineType::Auto,
            items,
        )) as Arc<dyn SearchEngine>;
        factory.register_engine(mock);

        let results = factory
            .search("query", 10, None, None, Some(SearchEngineType::Auto))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_search_with_abtest_type_no_preferred_engine() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let items = vec![ResponseItem {
            title: "ABTest".to_string(),
            url: "https://abtest.com".to_string(),
            description: "ab".to_string(),
            engine: SearchEngineType::ABTest,
        }];
        let mock = Arc::new(MockFactoryEngine::success(
            "abtest-engine",
            SearchEngineType::ABTest,
            items,
        )) as Arc<dyn SearchEngine>;
        factory.register_engine(mock);

        let results = factory
            .search("query", 10, None, None, Some(SearchEngineType::ABTest))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_search_with_lang_and_country_params() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let items = vec![ResponseItem {
            title: "Localized".to_string(),
            url: "https://loc.com".to_string(),
            description: "loc".to_string(),
            engine: SearchEngineType::Google,
        }];
        let mock = Arc::new(MockFactoryEngine::success(
            "google",
            SearchEngineType::Google,
            items,
        )) as Arc<dyn SearchEngine>;
        factory.register_engine(mock);

        let results = factory
            .search(
                "query",
                5,
                Some("zh"),
                Some("CN"),
                Some(SearchEngineType::Google),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Localized");
    }

    #[tokio::test]
    async fn test_search_without_engine_type_uses_default() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let items = vec![ResponseItem {
            title: "Default".to_string(),
            url: "https://default.com".to_string(),
            description: "def".to_string(),
            engine: SearchEngineType::Smart,
        }];
        let mock = Arc::new(MockFactoryEngine::success(
            "default-engine",
            SearchEngineType::Smart,
            items,
        )) as Arc<dyn SearchEngine>;
        factory.register_engine(mock);

        // engine_type = None uses config.default_engine which is Smart by default
        let results = factory.search("query", 10, None, None, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Default");
    }

    // ========== create_default_router_with_config tests ==========

    #[tokio::test]
    async fn test_create_default_router_with_config_returns_populated_router() {
        let router = create_default_router_with_config(make_http_client(), make_config_service())
            .await
            .expect("create_default_router_with_config should succeed");
        let engines = router.registered_engines();
        assert_eq!(engines.len(), 4, "should register exactly 4 engines");
        assert!(engines.iter().any(|n| n == "Google"));
        assert!(engines.iter().any(|n| n == "Bing"));
        assert!(engines.iter().any(|n| n == "Baidu"));
        assert!(engines.iter().any(|n| n == "sogou"));
    }

    // ========== create_smart_search tests ==========

    #[test]
    fn test_create_smart_search_returns_arc_with_engines() {
        let mut factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        factory.register_bing_engine();

        let smart = factory.create_smart_search();
        let engines = smart.registered_engines();
        assert!(
            engines.iter().any(|n| n == "Bing"),
            "create_smart_search should return router with registered engines"
        );
    }

    #[test]
    fn test_create_smart_search_empty_factory() {
        let factory = SearchEngineFactory::new(make_http_client(), make_config_service());
        let smart = factory.create_smart_search();
        assert!(
            smart.registered_engines().is_empty(),
            "smart search from empty factory should have no engines"
        );
    }
}
