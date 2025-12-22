// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::domain::models::search_result::SearchResult;
use crate::infrastructure::cache::cache_strategy::{
    CacheStrategy, CacheStrategyConfig, CacheStrategyFactory, CacheType, PreheatConfig,
};
use crate::infrastructure::cache::redis_client::RedisClient;

/// 缓存管理器
///
/// 提供统一的缓存接口，支持多种缓存策略
pub struct CacheManager {
    strategy: Arc<RwLock<Box<dyn CacheStrategy>>>,
    config: CacheStrategyConfig,
    redis_client: Option<Arc<RedisClient>>,
}

impl CacheManager {
    /// 创建新的缓存管理器
    pub async fn new(config: CacheStrategyConfig, redis_url: Option<&str>) -> Result<Self> {
        let redis_client = if let Some(url) = redis_url {
            Some(Arc::new(RedisClient::new(url).await?))
        } else {
            None
        };

        let strategy = CacheStrategyFactory::create_strategy(config.clone(), redis_client.clone());

        Ok(Self {
            strategy: Arc::new(RwLock::new(strategy)),
            config,
            redis_client,
        })
    }

    /// 获取缓存值
    pub async fn get(&self, key: &str) -> Result<Option<Vec<SearchResult>>> {
        let strategy = self.strategy.read().await;
        strategy.get(key).await
    }

    /// 设置缓存值
    pub async fn set(
        &self,
        key: &str,
        value: Vec<SearchResult>,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let strategy = self.strategy.write().await;
        strategy.set(key, value, ttl).await
    }

    /// 删除缓存值
    pub async fn delete(&self, key: &str) -> Result<()> {
        let strategy = self.strategy.write().await;
        strategy.delete(key).await
    }

    /// 清空缓存
    pub async fn clear(&self) -> Result<()> {
        let strategy = self.strategy.write().await;
        strategy.clear().await
    }

    /// 获取缓存统计信息
    pub async fn get_stats(&self) -> crate::infrastructure::cache::cache_strategy::CacheStats {
        let strategy = self.strategy.read().await;
        strategy.get_stats()
    }

    /// 执行缓存预热
    pub async fn preheat(&self, hot_queries: Vec<String>) -> Result<()> {
        if !self.config.enable_preload {
            debug!("Cache preloading is disabled");
            return Ok(());
        }

        info!(
            "Starting cache preheat for {} hot queries",
            hot_queries.len()
        );

        // 这里可以使用真实的搜索逻辑，但为了避免循环依赖，我们可以在这里通过引擎列表进行预热
        // 目前先清理预热标记
        let hot_data: Vec<(String, Vec<SearchResult>)> = hot_queries
            .into_iter()
            .map(|query| {
                let initial_results = vec![SearchResult {
                    title: format!("Preheated result for: {}", query),
                    url: format!("https://example.com/{}", query.replace(' ', "-")),
                    description: Some(format!("Preheated description for {}", query)),
                    engine: "preheat".to_string(),
                    score: 0.1,
                    published_time: None,
                }];
                (query, initial_results)
            })
            .collect();

        let strategy = self.strategy.write().await;
        strategy.preheat(hot_data).await?;

        info!("Cache preheat completed");
        Ok(())
    }

    /// 切换缓存策略
    pub async fn switch_strategy(&self, new_config: CacheStrategyConfig) -> Result<()> {
        info!("Switching cache strategy to: {:?}", new_config.cache_type);

        let new_strategy =
            CacheStrategyFactory::create_strategy(new_config.clone(), self.redis_client.clone());

        let mut strategy = self.strategy.write().await;
        *strategy = new_strategy;

        info!("Cache strategy switched successfully");
        Ok(())
    }

    /// 获取缓存命中率
    pub async fn get_hit_rate(&self) -> f64 {
        let stats = self.get_stats().await;
        let total_requests = stats.hits + stats.misses;

        if total_requests == 0 {
            0.0
        } else {
            stats.hits as f64 / total_requests as f64
        }
    }

    /// 监控缓存性能
    pub async fn monitor_performance(&self) -> CachePerformanceMetrics {
        let stats = self.get_stats().await;
        let hit_rate = self.get_hit_rate().await;

        CachePerformanceMetrics {
            hit_rate,
            total_hits: stats.hits,
            total_misses: stats.misses,
            total_stores: stats.stores,
            total_evictions: stats.evictions,
            preheat_hits: stats.preheat_hits,
        }
    }

    /// 智能缓存键生成
    pub fn generate_cache_key(
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
        engine_filter: Option<&str>,
    ) -> String {
        let mut key_parts = vec![query.to_string(), limit.to_string()];

        if let Some(lang) = lang {
            key_parts.push(format!("lang:{}", lang));
        }

        if let Some(country) = country {
            key_parts.push(format!("country:{}", country));
        }

        if let Some(engine) = engine_filter {
            key_parts.push(format!("engine:{}", engine));
        }

        key_parts.join(":")
    }

    /// 批量获取缓存
    pub async fn get_batch(&self, keys: &[String]) -> Result<Vec<Option<Vec<SearchResult>>>> {
        let mut results = Vec::new();

        for key in keys {
            let result = self.get(key).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// 批量设置缓存
    pub async fn set_batch(
        &self,
        entries: Vec<(String, Vec<SearchResult>)>,
        ttl: Option<Duration>,
    ) -> Result<()> {
        for (key, value) in entries {
            self.set(&key, value, ttl).await?;
        }

        Ok(())
    }

    /// 获取热门查询模式
    pub fn extract_hot_patterns(queries: &[String]) -> Vec<String> {
        use std::collections::HashMap;

        let mut pattern_counts: HashMap<String, u32> = HashMap::new();

        for query in queries {
            // 提取关键词模式（简化实现）
            let words: Vec<&str> = query.split_whitespace().collect();

            if words.len() >= 2 {
                // 提取前两个词作为模式
                let pattern = format!("{} {}", words[0], words[1]);
                *pattern_counts.entry(pattern).or_insert(0) += 1;
            }
        }

        // 按频率排序并返回前10个热门模式
        let mut patterns: Vec<(String, u32)> = pattern_counts.into_iter().collect();
        patterns.sort_by(|a, b| b.1.cmp(&a.1));

        patterns
            .into_iter()
            .take(10)
            .map(|(pattern, _)| pattern)
            .collect()
    }
}

/// 缓存性能指标
#[derive(Debug, Clone)]
pub struct CachePerformanceMetrics {
    pub hit_rate: f64,
    pub total_hits: u64,
    pub total_misses: u64,
    pub total_stores: u64,
    pub total_evictions: u64,
    pub preheat_hits: u64,
}

impl std::fmt::Display for CachePerformanceMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CachePerformance {{ hit_rate: {:.2}%, hits: {}, misses: {}, stores: {}, evictions: {}, preheat_hits: {} }}",
            self.hit_rate * 100.0,
            self.total_hits,
            self.total_misses,
            self.total_stores,
            self.total_evictions,
            self.preheat_hits
        )
    }
}

/// 缓存配置构建器
pub struct CacheConfigBuilder {
    config: CacheStrategyConfig,
}

impl CacheConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: CacheStrategyConfig::default(),
        }
    }

    pub fn cache_type(mut self, cache_type: CacheType) -> Self {
        self.config.cache_type = cache_type;
        self
    }

    pub fn ttl(mut self, seconds: u64) -> Self {
        self.config.ttl_seconds = seconds;
        self
    }

    pub fn max_entries(mut self, max_entries: usize) -> Self {
        self.config.max_entries = max_entries;
        self
    }

    pub fn enable_compression(mut self, enable: bool) -> Self {
        self.config.enable_compression = enable;
        self
    }

    pub fn enable_preload(mut self, enable: bool) -> Self {
        self.config.enable_preload = enable;
        self
    }

    pub fn preheat_config(
        mut self,
        hot_queries: Vec<String>,
        interval: u64,
        batch_size: usize,
    ) -> Self {
        self.config.preheat_config = Some(PreheatConfig {
            hot_queries,
            preheat_interval: interval,
            batch_size,
        });
        self
    }

    pub fn build(self) -> CacheStrategyConfig {
        self.config
    }
}

impl Default for CacheConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_generation() {
        let key1 =
            CacheManager::generate_cache_key("rust programming", 10, Some("en"), Some("US"), None);
        assert_eq!(key1, "rust programming:10:lang:en:country:US");

        let key2 = CacheManager::generate_cache_key("python", 5, None, None, Some("google"));
        assert_eq!(key2, "python:5:engine:google");
    }

    #[test]
    fn test_hot_pattern_extraction() {
        let queries = vec![
            "rust programming tutorial".to_string(),
            "rust programming guide".to_string(),
            "python programming tutorial".to_string(),
            "rust development tools".to_string(),
        ];

        let patterns = CacheManager::extract_hot_patterns(&queries);
        assert!(patterns.contains(&"rust programming".to_string()));
        assert!(patterns.contains(&"python programming".to_string()));
    }

    #[test]
    fn test_cache_config_builder() {
        let config = CacheConfigBuilder::new()
            .cache_type(CacheType::Layered)
            .ttl(600)
            .max_entries(5000)
            .enable_compression(true)
            .enable_preload(true)
            .build();

        assert!(matches!(config.cache_type, CacheType::Layered));
        assert_eq!(config.ttl_seconds, 600);
        assert_eq!(config.max_entries, 5000);
        assert!(config.enable_compression);
        assert!(config.enable_preload);
    }
}
