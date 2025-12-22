// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use crate::infrastructure::cache::cache_manager::CacheManager;
use crate::infrastructure::cache::cache_strategy::CacheStrategyConfig;

/// 增强的搜索聚合器，集成智能缓存策略
pub struct EnhancedSearchAggregator {
    engines: Vec<Arc<dyn SearchEngine>>,
    timeout: Duration,
    cache_manager: Arc<CacheManager>,
    cache_config: CacheStrategyConfig,
}

impl EnhancedSearchAggregator {
    pub fn new(
        engines: Vec<Arc<dyn SearchEngine>>,
        timeout_ms: u64,
        cache_manager: Arc<CacheManager>,
        cache_config: CacheStrategyConfig,
    ) -> Self {
        Self {
            engines,
            timeout: Duration::from_millis(timeout_ms),
            cache_manager,
            cache_config,
        }
    }

    /// 生成缓存键
    fn generate_cache_key(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> String {
        CacheManager::generate_cache_key(query, limit, lang, country, None)
    }

    /// 执行缓存感知的搜索
    async fn search_with_cache(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let cache_key = self.generate_cache_key(query, limit, lang, country);

        // 首先检查缓存
        match self.cache_manager.get(&cache_key).await {
            Ok(Some(cached_results)) => {
                info!(
                    "Cache hit for query: {} ({} results)",
                    query,
                    cached_results.len()
                );
                return Ok(cached_results);
            }
            Ok(None) => {
                info!("Cache miss for query: {}", query);
            }
            Err(e) => {
                warn!("Cache error for query {}: {}", query, e);
                // 缓存错误不影响搜索，继续执行搜索
            }
        }

        // 缓存未命中，执行搜索
        let results = self.search_engines(query, limit, lang, country).await?;

        // 将结果缓存起来
        if !results.is_empty() {
            let cache_ttl = Duration::from_secs(self.cache_config.ttl_seconds);
            if let Err(e) = self
                .cache_manager
                .set(&cache_key, results.clone(), Some(cache_ttl))
                .await
            {
                warn!("Failed to cache results for query {}: {}", query, e);
            } else {
                info!("Cached {} results for query: {}", results.len(), query);
            }
        }

        Ok(results)
    }

    /// 执行引擎搜索
    async fn search_engines(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        use futures::future::join_all;
        use std::time::Instant;

        let start_time = Instant::now();
        let mut all_results: Vec<SearchResult> = Vec::new();

        // 并行搜索所有引擎
        let search_futures: Vec<_> = self
            .engines
            .iter()
            .map(|engine| {
                let engine = engine.clone();
                let query = query.to_string();
                let lang = lang.map(|s| s.to_string());
                let country = country.map(|s| s.to_string());

                async move {
                    let engine_name = engine.name();

                    // 使用超时机制
                    match tokio::time::timeout(
                        self.timeout,
                        engine.search(&query, limit, lang.as_deref(), country.as_deref()),
                    )
                    .await
                    {
                        Ok(Ok(results)) => {
                            info!("Engine {} returned {} results", engine_name, results.len());
                            Some(results)
                        }
                        Ok(Err(e)) => {
                            warn!("Engine {} failed: {}", engine_name, e);
                            None
                        }
                        Err(_) => {
                            warn!("Engine {} timed out", engine_name);
                            None
                        }
                    }
                }
            })
            .collect();

        // 等待所有搜索完成
        let results: Vec<_> = join_all(search_futures).await;

        // 合并所有结果
        all_results.extend(results.into_iter().flatten().flatten());

        let search_time = start_time.elapsed();
        info!(
            "Search completed in {:?}, total results: {}",
            search_time,
            all_results.len()
        );

        Ok(all_results)
    }

    /// 获取缓存统计信息
    pub async fn get_cache_stats(
        &self,
    ) -> crate::infrastructure::cache::cache_strategy::CacheStats {
        self.cache_manager.get_stats().await
    }

    /// 获取缓存性能指标
    pub async fn get_cache_performance(
        &self,
    ) -> crate::infrastructure::cache::cache_manager::CachePerformanceMetrics {
        self.cache_manager.monitor_performance().await
    }

    /// 执行缓存预热
    pub async fn preheat_cache(&self, hot_queries: Vec<String>) -> anyhow::Result<()> {
        info!(
            "Starting cache preheat for {} hot queries",
            hot_queries.len()
        );
        self.cache_manager.preheat(hot_queries).await
    }

    /// 清空缓存
    pub async fn clear_cache(&self) -> anyhow::Result<()> {
        info!("Clearing search cache");
        self.cache_manager.clear().await
    }

    /// 获取缓存命中率
    pub async fn get_cache_hit_rate(&self) -> f64 {
        self.cache_manager.get_hit_rate().await
    }
}

#[async_trait]
impl SearchEngine for EnhancedSearchAggregator {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        self.search_with_cache(query, limit, lang, country).await
    }

    fn name(&self) -> &'static str {
        "enhanced_aggregator"
    }
}

/// 增强搜索聚合器构建器
pub struct EnhancedSearchAggregatorBuilder {
    engines: Vec<Arc<dyn SearchEngine>>,
    timeout_ms: u64,
    cache_config: CacheStrategyConfig,
    redis_url: Option<String>,
}

impl EnhancedSearchAggregatorBuilder {
    pub fn new() -> Self {
        Self {
            engines: Vec::new(),
            timeout_ms: 5000, // 默认5秒超时
            cache_config: CacheStrategyConfig::default(),
            redis_url: None,
        }
    }

    pub fn add_engine(mut self, engine: Arc<dyn SearchEngine>) -> Self {
        self.engines.push(engine);
        self
    }

    pub fn timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub fn cache_config(mut self, cache_config: CacheStrategyConfig) -> Self {
        self.cache_config = cache_config;
        self
    }

    pub fn redis_url(mut self, redis_url: &str) -> Self {
        self.redis_url = Some(redis_url.to_string());
        self
    }

    pub async fn build(self) -> Result<EnhancedSearchAggregator, anyhow::Error> {
        let cache_manager = Arc::new(
            CacheManager::new(self.cache_config.clone(), self.redis_url.as_deref()).await?,
        );

        Ok(EnhancedSearchAggregator::new(
            self.engines,
            self.timeout_ms,
            cache_manager,
            self.cache_config,
        ))
    }
}

impl Default for EnhancedSearchAggregatorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::cache::cache_strategy::{CacheType, LayeredCacheConfig};

    #[tokio::test]
    async fn test_cache_key_generation() {
        let aggregator = EnhancedSearchAggregatorBuilder::new()
            .timeout_ms(1000)
            .build()
            .await
            .unwrap();

        let key1 = aggregator.generate_cache_key("rust programming", 10, Some("en"), Some("US"));
        assert_eq!(key1, "rust programming:10:lang:en:country:US");

        let key2 = aggregator.generate_cache_key("python", 5, None, None);
        assert_eq!(key2, "python:5");
    }

    #[tokio::test]
    async fn test_builder_with_layered_cache() {
        let layered_config = LayeredCacheConfig {
            memory_ttl: 120,
            redis_ttl: 3600,
            memory_max_entries: 2000,
        };

        let cache_config = CacheStrategyConfig {
            cache_type: CacheType::Layered,
            ttl_seconds: 1800,
            max_entries: 10000,
            enable_compression: true,
            enable_preload: false,
            preheat_config: None,
            layered_config: Some(layered_config),
        };

        let _aggregator = EnhancedSearchAggregatorBuilder::new()
            .cache_config(cache_config)
            .redis_url("redis://localhost:6379")
            .timeout_ms(5000)
            .build()
            .await;
    }
}
