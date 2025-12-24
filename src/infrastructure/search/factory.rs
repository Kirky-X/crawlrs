// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::infrastructure::search::baidu::BaiduSearchEngine;
use crate::infrastructure::search::bing::BingSearchEngine;
use crate::infrastructure::search::google::GoogleSearchEngine;
use crate::infrastructure::search::search_engine_router::{SearchEngineRouter, SearchEngineRouterConfig};
use crate::infrastructure::search::sogou::SogouSearchEngine;
use crate::domain::search::engine::SearchEngine;
use std::sync::Arc;
use tracing::info;

/// 搜索引擎类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchEngineType {
    /// Google 搜索引擎
    Google,
    /// Bing 搜索引擎
    Bing,
    /// 百度搜索引擎
    Baidu,
    /// 搜狗搜索引擎
    Sogou,
    /// 智能搜索（自动路由）
    Smart,
    /// A/B 测试搜索
    ABTest,
}

impl SearchEngineType {
    /// 获取引擎名称
    pub fn name(&self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Bing => "bing",
            Self::Baidu => "baidu",
            Self::Sogou => "sogou",
            Self::Smart => "smart",
            Self::ABTest => "ab_test",
        }
    }

    /// 从字符串解析引擎类型
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "google" => Some(Self::Google),
            "bing" => Some(Self::Bing),
            "baidu" => Some(Self::Baidu),
            "sogou" => Some(Self::Sogou),
            "smart" => Some(Self::Smart),
            "ab_test" | "abtest" => Some(Self::ABTest),
            _ => None,
        }
    }
}

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
    pub async fn create_all_engines(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("正在创建所有搜索引擎...");

        // Google 搜索引擎
        self.register_google_engine().await;

        // Bing 搜索引擎
        self.register_bing_engine();

        // Baidu 搜索引擎
        self.register_baidu_engine();

        // Sogou 搜索引擎
        self.register_sogou_engine();

        info!("所有搜索引擎创建完成，已注册: {:?}", self.router.registered_engines());

        Ok(())
    }

    /// 创建并注册 Google 搜索引擎
    pub async fn register_google_engine(&mut self) {
        #[cfg(feature = "playwright")]
        {
            let google_engine = Arc::new(GoogleSearchEngine::new());
            self.router.register_engine(google_engine);
            info!("Google 搜索引擎已注册（使用 Playwright）");
        }

        #[cfg(not(feature = "playwright"))]
        {
            let google_engine = Arc::new(GoogleSearchEngine::new());
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
            SearchEngineType::Smart | SearchEngineType::ABTest => None,
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
    ) -> Result<Vec<crate::domain::models::search_result::SearchResult>, crate::domain::search::engine::SearchError> {
        match engine_type.unwrap_or(self.config.default_engine) {
            SearchEngineType::Google => {
                if let Some(engine) = self.router.get_engine("google") {
                    engine.search(query, limit, lang, country).await
                } else {
                    self.router.search(query, limit, lang, country, Some("google")).await
                }
            }
            SearchEngineType::Bing => {
                if let Some(engine) = self.router.get_engine("bing") {
                    engine.search(query, limit, lang, country).await
                } else {
                    self.router.search(query, limit, lang, country, Some("bing")).await
                }
            }
            SearchEngineType::Baidu => {
                if let Some(engine) = self.router.get_engine("baidu") {
                    engine.search(query, limit, lang, country).await
                } else {
                    self.router.search(query, limit, lang, country, Some("baidu")).await
                }
            }
            SearchEngineType::Sogou => {
                if let Some(engine) = self.router.get_engine("sogou") {
                    engine.search(query, limit, lang, country).await
                } else {
                    self.router.search(query, limit, lang, country, Some("sogou")).await
                }
            }
            SearchEngineType::Smart => {
                self.router.search(query, limit, lang, country, None).await
            }
            SearchEngineType::ABTest => {
                self.router.search(query, limit, lang, country, None).await
            }
        }
    }

    /// 获取工厂统计信息
    pub fn stats(&self) -> crate::infrastructure::search::search_engine_router::RouterStats {
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
pub async fn create_default_router() -> Result<SearchEngineRouter, Box<dyn std::error::Error + Send + Sync>> {
    let mut factory = SearchEngineFactory::new();
    factory.create_all_engines().await?;
    Ok(factory.router.clone())
}

/// 便捷函数：创建单一搜索引擎
#[cfg(feature = "playwright")]
pub fn create_google_engine() -> Arc<dyn SearchEngine> {
    Arc::new(GoogleSearchEngine::new())
}

#[cfg(not(feature = "playwright"))]
pub fn create_google_engine() -> Arc<dyn SearchEngine> {
    Arc::new(GoogleSearchEngine::new())
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
