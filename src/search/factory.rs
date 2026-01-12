// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(deprecated)]

#[cfg(feature = "engine-fire-cdp")]
use crate::engines::client::fire_cdp::FireEngineCdp;

#[cfg(feature = "engine-fire-tls")]
use crate::engines::client::fire_tls::FireEngineTls;

use crate::engines::engine_client::EngineClient;
use crate::engines::router::EngineRouter;
use crate::search::client::baidu::BaiduSearchEngine;
use crate::search::client::bing::BingSearchEngine;
use crate::search::client::google::GoogleSearchEngine;
use crate::search::client::sogou::SogouSearchEngine;
use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::router::{SearchEngineRouter, SearchEngineRouterConfig};
use crate::search::smart::{
    create_baidu_smart_search, create_bing_smart_search, create_google_smart_search,
};
use crate::search::types::SearchEngineType;
use std::sync::Arc;

use tracing::info;

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
}

impl SearchEngineFactory {
    /// 创建新的搜索引擎工厂
    pub fn new() -> Self {
        Self::with_config(SearchEngineFactoryConfig::default())
    }

    /// 使用配置创建搜索引擎工厂
    pub fn with_config(config: SearchEngineFactoryConfig) -> Self {
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
        let bing_engine = Arc::new(BingSearchEngine::new());
        self.router.register_engine(bing_engine);
        info!("Bing 搜索引擎已注册");
    }

    /// 创建并注册 Baidu 搜索引擎
    pub fn register_baidu_engine(&mut self) {
        let baidu_engine = Arc::new(BaiduSearchEngine::new());
        self.router.register_engine(baidu_engine);
        info!("Baidu 搜索引擎已注册");
    }

    /// 创建并注册 Sogou 搜索引擎
    pub fn register_sogou_engine(&mut self) {
        let sogou_engine = Arc::new(SogouSearchEngine::new());
        self.router.register_engine(sogou_engine);
        info!("Sogou 搜索引擎已注册");
    }

    /// 创建 EngineClient 并注册 Fire Engines（用于智能搜索）
    #[allow(deprecated)]
    pub fn create_engine_client_with_fire_engines(&self) -> Arc<EngineClient> {
        let engines: Vec<Arc<dyn crate::engines::traits::ScraperEngine>> = Vec::new();

        // 注册 Fire Engine CDP（用于需要完整浏览器自动化的网站）
        #[cfg(feature = "engine-fire-cdp")]
        {
            let fire_engine_cdp = Arc::new(FireEngineCdp::new());
            engines.push(fire_engine_cdp.clone() as Arc<dyn crate::engines::traits::ScraperEngine>);
        }

        // 注册 Fire Engine TLS（用于需要TLS指纹对抗的网站）
        #[cfg(feature = "engine-fire-tls")]
        {
            let fire_engine_tls = Arc::new(FireEngineTls::new());
            engines.push(fire_engine_tls.clone() as Arc<dyn crate::engines::traits::ScraperEngine>);
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
        crate::domain::search::engine::SearchError,
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
            .map_err(|e| crate::domain::search::engine::SearchError::EngineError(e.to_string()))?;

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

impl Default for SearchEngineFactory {
    fn default() -> Self {
        Self::new()
    }
}

/// 便捷函数：创建默认的搜索引擎路由器
pub async fn create_default_router(
) -> Result<SearchEngineRouter, Box<dyn std::error::Error + Send + Sync>> {
    let mut factory = SearchEngineFactory::new();
    factory.create_all_engines().await?;
    Ok(factory.router.clone())
}

/// 便捷函数：创建单一搜索引擎
#[cfg(feature = "engine-playwright")]
pub fn create_google_engine() -> Arc<dyn SearchEngine> {
    let engines: Vec<Arc<dyn crate::engines::traits::ScraperEngine>> = Vec::new();

    #[cfg(feature = "engine-fire-cdp")]
    {
        let fire_engine_cdp = Arc::new(FireEngineCdp::new());
        engines.push(fire_engine_cdp as Arc<dyn crate::engines::traits::ScraperEngine>);
    }

    #[cfg(feature = "engine-fire-tls")]
    {
        let fire_engine_tls = Arc::new(FireEngineTls::new());
        engines.push(fire_engine_tls as Arc<dyn crate::engines::traits::ScraperEngine>);
    }

    let engine_client = Arc::new(EngineClient::with_engines(engines));
    Arc::new(GoogleSearchEngine::new(engine_client))
}

#[cfg(not(feature = "engine-playwright"))]
pub fn create_google_engine() -> Arc<dyn SearchEngine> {
    let engines: Vec<Arc<dyn crate::engines::traits::ScraperEngine>> = Vec::new();

    #[cfg(feature = "engine-fire-cdp")]
    {
        let fire_engine_cdp = Arc::new(FireEngineCdp::new());
        engines.push(fire_engine_cdp as Arc<dyn crate::engines::traits::ScraperEngine>);
    }

    #[cfg(feature = "engine-fire-tls")]
    {
        let fire_engine_tls = Arc::new(FireEngineTls::new());
        engines.push(fire_engine_tls as Arc<dyn crate::engines::traits::ScraperEngine>);
    }

    let engine_client = Arc::new(EngineClient::with_engines(engines));
    Arc::new(GoogleSearchEngine::new(engine_client))
}

/// 便捷函数：创建 Bing 搜索引擎
pub fn create_bing_engine() -> Arc<dyn SearchEngine> {
    Arc::new(BingSearchEngine::new())
}

/// 便捷函数：创建 Baidu 搜索引擎
pub fn create_baidu_engine() -> Arc<dyn SearchEngine> {
    Arc::new(BaiduSearchEngine::new())
}

/// 便捷函数：创建 Sogou 搜索引擎
pub fn create_sogou_engine() -> Arc<dyn SearchEngine> {
    Arc::new(SogouSearchEngine::new())
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
