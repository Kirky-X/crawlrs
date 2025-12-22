// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use crate::domain::models::search_result::SearchResult;

/// 缓存策略配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStrategyConfig {
    /// 缓存类型
    pub cache_type: CacheType,
    /// TTL（秒）
    pub ttl_seconds: u64,
    /// 最大缓存条目数
    pub max_entries: usize,
    /// 是否启用压缩
    pub enable_compression: bool,
    /// 是否启用预加载
    pub enable_preload: bool,
    /// 缓存预热配置
    pub preheat_config: Option<PreheatConfig>,
    /// 分层缓存配置
    pub layered_config: Option<LayeredCacheConfig>,
}

impl Default for CacheStrategyConfig {
    fn default() -> Self {
        Self {
            cache_type: CacheType::Memory,
            ttl_seconds: 300, // 5分钟
            max_entries: 10000,
            enable_compression: true,
            enable_preload: false,
            preheat_config: None,
            layered_config: None,
        }
    }
}

/// 缓存类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CacheType {
    /// 内存缓存
    Memory,
    /// Redis缓存
    Redis,
    /// 分层缓存（内存+Redis）
    Layered,
    /// 智能缓存（根据数据特性自动选择）
    Smart,
}

/// 预热配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreheatConfig {
    /// 预热查询列表
    pub hot_queries: Vec<String>,
    /// 预热间隔（秒）
    pub preheat_interval: u64,
    /// 预热批次大小
    pub batch_size: usize,
}

/// 分层缓存配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayeredCacheConfig {
    /// 内存缓存TTL（秒）
    pub memory_ttl: u64,
    /// Redis缓存TTL（秒）
    pub redis_ttl: u64,
    /// 内存缓存最大条目数
    pub memory_max_entries: usize,
}

/// 缓存统计信息
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub stores: u64,
    pub compression_saves: u64,
    pub preheat_hits: u64,
}

/// 缓存条目
#[derive(Clone)]
struct CacheEntry<T> {
    data: T,
    created_at: Instant,
    ttl: Duration,
    access_count: u64,
    last_accessed: Instant,
}

impl<T> CacheEntry<T> {
    fn new(data: T, ttl: Duration) -> Self {
        let now = Instant::now();
        Self {
            data,
            created_at: now,
            ttl,
            access_count: 0,
            last_accessed: now,
        }
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }

    fn touch(&mut self) {
        self.access_count += 1;
        self.last_accessed = Instant::now();
    }

    fn get_priority_score(&self) -> f64 {
        let age_score = self.created_at.elapsed().as_secs_f64() * 0.1;
        let access_score = self.access_count as f64 * 0.9;
        age_score + access_score
    }
}

/// 缓存策略接口
#[async_trait]
pub trait CacheStrategy: Send + Sync {
    /// 获取缓存值
    async fn get(&self, key: &str) -> Result<Option<Vec<SearchResult>>>;

    /// 设置缓存值
    async fn set(&self, key: &str, value: Vec<SearchResult>, ttl: Option<Duration>) -> Result<()>;

    /// 删除缓存值
    async fn delete(&self, key: &str) -> Result<()>;

    /// 清空缓存
    async fn clear(&self) -> Result<()>;

    /// 获取缓存统计信息
    fn get_stats(&self) -> CacheStats;

    /// 执行缓存预热
    async fn preheat(&self, hot_data: Vec<(String, Vec<SearchResult>)>) -> Result<()>;
}

/// 内存缓存策略
pub struct MemoryCacheStrategy {
    cache: DashMap<String, CacheEntry<Vec<SearchResult>>>,
    config: CacheStrategyConfig,
    _stats: std::sync::Arc<std::sync::Mutex<CacheStats>>,
}

impl MemoryCacheStrategy {
    pub fn new(config: CacheStrategyConfig) -> Self {
        Self {
            cache: DashMap::new(),
            config,
            _stats: std::sync::Arc::new(std::sync::Mutex::new(CacheStats::default())),
        }
    }

    fn evict_if_needed(&self) {
        let current_size = self.cache.len();
        if current_size <= self.config.max_entries {
            return;
        }

        // 需要淘汰条目
        let to_evict = current_size - self.config.max_entries + (self.config.max_entries / 10); // 多淘汰10%

        // 收集所有条目并按优先级排序
        let mut entries: Vec<(String, f64)> = self
            .cache
            .iter()
            .map(|entry| {
                let score = entry.value().get_priority_score();
                (entry.key().clone(), score)
            })
            .collect();

        entries.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // 淘汰优先级最低的条目
        for (key, _) in entries.iter().take(to_evict) {
            self.cache.remove(key);
        }

        let mut stats = self._stats.lock().unwrap();
        stats.evictions += to_evict as u64;

        debug!("Evicted {} entries from memory cache", to_evict);
    }
}

#[async_trait]
impl CacheStrategy for MemoryCacheStrategy {
    async fn get(&self, key: &str) -> Result<Option<Vec<SearchResult>>> {
        if let Some(mut entry) = self.cache.get_mut(key) {
            if entry.is_expired() {
                drop(entry);
                self.cache.remove(key);
                let mut stats = self._stats.lock().unwrap();
                stats.misses += 1;
                return Ok(None);
            }

            entry.touch();
            let mut stats = self._stats.lock().unwrap();
            stats.hits += 1;

            Ok(Some(entry.data.clone()))
        } else {
            let mut stats = self._stats.lock().unwrap();
            stats.misses += 1;
            Ok(None)
        }
    }

    async fn set(&self, key: &str, value: Vec<SearchResult>, ttl: Option<Duration>) -> Result<()> {
        let ttl = ttl.unwrap_or(Duration::from_secs(self.config.ttl_seconds));
        let results_count = value.len();
        let entry = CacheEntry::new(value, ttl);

        self.cache.insert(key.to_string(), entry);
        self.evict_if_needed();

        let mut stats = self._stats.lock().unwrap();
        stats.stores += 1;

        debug!(
            "Stored {} results in memory cache for key: {}",
            results_count, key
        );
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.cache.remove(key);
        debug!("Deleted cache entry for key: {}", key);
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        self.cache.clear();
        info!("Cleared all memory cache entries");
        Ok(())
    }

    fn get_stats(&self) -> CacheStats {
        self._stats.lock().unwrap().clone()
    }

    async fn preheat(&self, hot_data: Vec<(String, Vec<SearchResult>)>) -> Result<()> {
        let entry_count = hot_data.len();
        info!("Preheating memory cache with {} hot entries", entry_count);

        for (key, results) in hot_data {
            let ttl = Duration::from_secs(self.config.ttl_seconds);
            self.set(&key, results, Some(ttl)).await?;
        }

        let mut stats = self._stats.lock().unwrap();
        stats.preheat_hits += entry_count as u64;

        info!("Memory cache preheating completed");
        Ok(())
    }
}

/// Redis缓存策略
pub struct RedisCacheStrategy {
    redis_client: Arc<crate::infrastructure::cache::redis_client::RedisClient>,
    config: CacheStrategyConfig,
    _stats: std::sync::Arc<std::sync::Mutex<CacheStats>>,
}

impl RedisCacheStrategy {
    pub fn new(
        redis_client: Arc<crate::infrastructure::cache::redis_client::RedisClient>,
        config: CacheStrategyConfig,
    ) -> Self {
        Self {
            redis_client,
            config,
            _stats: std::sync::Arc::new(std::sync::Mutex::new(CacheStats::default())),
        }
    }

    fn generate_cache_key(&self, key: &str) -> String {
        format!("search_cache:{}", key)
    }
}

#[async_trait]
impl CacheStrategy for RedisCacheStrategy {
    async fn get(&self, key: &str) -> Result<Option<Vec<SearchResult>>> {
        let cache_key = self.generate_cache_key(key);

        match self.redis_client.get(&cache_key).await? {
            Some(json_str) => {
                let results: Vec<SearchResult> = serde_json::from_str(&json_str)?;

                let mut stats = self._stats.lock().unwrap();
                stats.hits += 1;

                debug!("Cache hit for key: {}", key);
                Ok(Some(results))
            }
            None => {
                let mut stats = self._stats.lock().unwrap();
                stats.misses += 1;

                debug!("Cache miss for key: {}", key);
                Ok(None)
            }
        }
    }

    async fn set(&self, key: &str, value: Vec<SearchResult>, ttl: Option<Duration>) -> Result<()> {
        let cache_key = self.generate_cache_key(key);
        let ttl = ttl.unwrap_or(Duration::from_secs(self.config.ttl_seconds));

        let json_str = serde_json::to_string(&value)?;

        self.redis_client
            .set(&cache_key, &json_str, ttl.as_secs() as usize)
            .await?;

        let mut stats = self._stats.lock().unwrap();
        stats.stores += 1;

        debug!(
            "Stored {} results in Redis cache for key: {}",
            value.len(),
            key
        );
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let cache_key = self.generate_cache_key(key);
        self.redis_client.set_forever(&cache_key, "").await?;

        debug!("Deleted cache entry for key: {}", key);
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        // Redis不支持直接清空所有匹配模式的键，这里简单记录日志
        warn!("Redis cache clear not implemented, would need to iterate over all keys");
        Ok(())
    }

    fn get_stats(&self) -> CacheStats {
        self._stats.lock().unwrap().clone()
    }

    async fn preheat(&self, hot_data: Vec<(String, Vec<SearchResult>)>) -> Result<()> {
        let entry_count = hot_data.len();
        info!("Preheating Redis cache with {} hot entries", entry_count);

        for (key, results) in hot_data {
            let ttl = Duration::from_secs(self.config.ttl_seconds);
            self.set(&key, results, Some(ttl)).await?;
        }

        let mut stats = self._stats.lock().unwrap();
        stats.preheat_hits += entry_count as u64;

        info!("Redis cache preheating completed");
        Ok(())
    }
}

/// 分层缓存策略（内存+Redis）
pub struct LayeredCacheStrategy {
    memory_cache: MemoryCacheStrategy,
    redis_cache: RedisCacheStrategy,
    config: LayeredCacheConfig,
    #[allow(dead_code)]
    _stats: std::sync::Arc<std::sync::Mutex<CacheStats>>,
}

impl LayeredCacheStrategy {
    pub fn new(
        memory_config: CacheStrategyConfig,
        redis_client: Arc<crate::infrastructure::cache::redis_client::RedisClient>,
        redis_config: CacheStrategyConfig,
        layered_config: LayeredCacheConfig,
    ) -> Self {
        Self {
            memory_cache: MemoryCacheStrategy::new(memory_config),
            redis_cache: RedisCacheStrategy::new(redis_client, redis_config),
            config: layered_config,
            _stats: std::sync::Arc::new(std::sync::Mutex::new(CacheStats::default())),
        }
    }
}

#[async_trait]
impl CacheStrategy for LayeredCacheStrategy {
    async fn get(&self, key: &str) -> Result<Option<Vec<SearchResult>>> {
        // 先检查内存缓存
        if let Some(results) = self.memory_cache.get(key).await? {
            debug!("Layered cache hit in memory layer for key: {}", key);
            return Ok(Some(results));
        }

        // 内存未命中，检查Redis
        if let Some(results) = self.redis_cache.get(key).await? {
            debug!("Layered cache hit in Redis layer for key: {}", key);

            // 回填到内存缓存
            let memory_ttl = Duration::from_secs(self.config.memory_ttl);
            self.memory_cache
                .set(key, results.clone(), Some(memory_ttl))
                .await?;

            return Ok(Some(results));
        }

        debug!("Layered cache miss for key: {}", key);
        Ok(None)
    }

    async fn set(&self, key: &str, value: Vec<SearchResult>, ttl: Option<Duration>) -> Result<()> {
        // 设置到两层缓存
        let memory_ttl = Duration::from_secs(self.config.memory_ttl);
        let redis_ttl = ttl.unwrap_or(Duration::from_secs(self.config.redis_ttl));

        // 并行设置
        let memory_future = self.memory_cache.set(key, value.clone(), Some(memory_ttl));
        let redis_future = self.redis_cache.set(key, value, Some(redis_ttl));

        tokio::try_join!(memory_future, redis_future)?;

        debug!("Set layered cache for key: {}", key);
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let memory_future = self.memory_cache.delete(key);
        let redis_future = self.redis_cache.delete(key);

        tokio::try_join!(memory_future, redis_future)?;

        debug!("Deleted layered cache for key: {}", key);
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        let memory_future = self.memory_cache.clear();
        let redis_future = self.redis_cache.clear();

        tokio::try_join!(memory_future, redis_future)?;

        info!("Cleared layered cache");
        Ok(())
    }

    fn get_stats(&self) -> CacheStats {
        let memory_stats = self.memory_cache.get_stats();
        let redis_stats = self.redis_cache.get_stats();

        CacheStats {
            hits: memory_stats.hits + redis_stats.hits,
            misses: memory_stats.misses + redis_stats.misses,
            evictions: memory_stats.evictions + redis_stats.evictions,
            stores: memory_stats.stores + redis_stats.stores,
            compression_saves: memory_stats.compression_saves + redis_stats.compression_saves,
            preheat_hits: memory_stats.preheat_hits + redis_stats.preheat_hits,
        }
    }

    async fn preheat(&self, hot_data: Vec<(String, Vec<SearchResult>)>) -> Result<()> {
        info!(
            "Preheating layered cache with {} hot entries",
            hot_data.len()
        );

        let memory_future = self.memory_cache.preheat(hot_data.clone());
        let redis_future = self.redis_cache.preheat(hot_data);

        tokio::try_join!(memory_future, redis_future)?;

        info!("Layered cache preheating completed");
        Ok(())
    }
}

/// 智能缓存策略
pub struct SmartCacheStrategy {
    strategies: Vec<Box<dyn CacheStrategy>>,
    current_strategy: std::sync::Arc<std::sync::RwLock<usize>>,
    performance_history: std::sync::Arc<std::sync::Mutex<Vec<CachePerformance>>>,
}

#[derive(Clone)]
struct CachePerformance {
    strategy_index: usize,
    hit_rate: f64,
    latency_ms: f64,
    #[allow(dead_code)]
    _memory_usage: f64,
    _timestamp: Instant,
}

/// 缓存监控器
#[allow(dead_code)]
pub struct CacheMonitor {
    stats: Arc<std::sync::Mutex<CacheStats>>,
    _last_report: Instant,
}

impl CacheMonitor {
    pub fn new(stats: Arc<std::sync::Mutex<CacheStats>>) -> Self {
        Self {
            stats,
            _last_report: Instant::now(),
        }
    }
}

impl SmartCacheStrategy {
    pub fn new(strategies: Vec<Box<dyn CacheStrategy>>) -> Self {
        Self {
            strategies,
            current_strategy: std::sync::Arc::new(std::sync::RwLock::new(0)),
            performance_history: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn select_optimal_strategy(&self) -> usize {
        let history = self.performance_history.lock().unwrap();
        if history.is_empty() {
            return 0;
        }

        // 简单的性能评分算法
        let mut strategy_scores: Vec<(usize, f64)> = (0..self.strategies.len())
            .map(|i| {
                let relevant_history: Vec<&CachePerformance> =
                    history.iter().filter(|p| p.strategy_index == i).collect();

                if relevant_history.is_empty() {
                    (i, 0.5) // 默认分数
                } else {
                    let avg_hit_rate = relevant_history.iter().map(|p| p.hit_rate).sum::<f64>()
                        / relevant_history.len() as f64;
                    let avg_latency = relevant_history.iter().map(|p| p.latency_ms).sum::<f64>()
                        / relevant_history.len() as f64;

                    // 评分：命中率权重 70%，延迟权重 30%
                    let score = avg_hit_rate * 0.7 + (1000.0 / (avg_latency + 1.0)) * 0.3;
                    (i, score)
                }
            })
            .collect();

        strategy_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        strategy_scores[0].0
    }

    fn record_performance(&self, strategy_index: usize, hit_rate: f64, latency_ms: f64) {
        let mut history = self.performance_history.lock().unwrap();
        history.push(CachePerformance {
            strategy_index,
            hit_rate,
            latency_ms,
            _memory_usage: 0.0, // 简化实现
            _timestamp: Instant::now(),
        });

        // 保持历史记录在合理大小内
        if history.len() > 1000 {
            history.drain(0..100);
        }
    }
}

#[async_trait]
impl CacheStrategy for SmartCacheStrategy {
    async fn get(&self, key: &str) -> Result<Option<Vec<SearchResult>>> {
        let start = Instant::now();
        let current_index = *self.current_strategy.read().unwrap();

        let result = self.strategies[current_index].get(key).await?;
        let latency_ms = start.elapsed().as_millis() as f64;

        // 记录性能（简化版）
        let hit_rate = if result.is_some() { 1.0 } else { 0.0 };
        self.record_performance(current_index, hit_rate, latency_ms);

        // 定期重新选择策略
        if rand::random::<f64>() < 0.01 {
            let optimal_index = self.select_optimal_strategy();
            if optimal_index != current_index {
                *self.current_strategy.write().unwrap() = optimal_index;
                info!("Switched to cache strategy index: {}", optimal_index);
            }
        }

        Ok(result)
    }

    async fn set(&self, key: &str, value: Vec<SearchResult>, ttl: Option<Duration>) -> Result<()> {
        let current_index = *self.current_strategy.read().unwrap();
        self.strategies[current_index].set(key, value, ttl).await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let current_index = *self.current_strategy.read().unwrap();
        self.strategies[current_index].delete(key).await
    }

    async fn clear(&self) -> Result<()> {
        // 清空所有策略的缓存
        for strategy in &self.strategies {
            strategy.clear().await?;
        }
        Ok(())
    }

    fn get_stats(&self) -> CacheStats {
        let current_index = *self.current_strategy.read().unwrap();
        self.strategies[current_index].get_stats()
    }

    async fn preheat(&self, hot_data: Vec<(String, Vec<SearchResult>)>) -> Result<()> {
        let current_index = *self.current_strategy.read().unwrap();
        self.strategies[current_index].preheat(hot_data).await
    }
}

/// 缓存策略工厂
pub struct CacheStrategyFactory;

impl CacheStrategyFactory {
    pub fn create_strategy(
        config: CacheStrategyConfig,
        redis_client: Option<Arc<crate::infrastructure::cache::redis_client::RedisClient>>,
    ) -> Box<dyn CacheStrategy> {
        match config.cache_type {
            CacheType::Memory => Box::new(MemoryCacheStrategy::new(config)),
            CacheType::Redis => {
                let redis_client =
                    redis_client.expect("Redis client required for Redis cache type");
                Box::new(RedisCacheStrategy::new(redis_client, config))
            }
            CacheType::Layered => {
                let redis_client =
                    redis_client.expect("Redis client required for Layered cache type");
                let layered_config = config.layered_config.clone().unwrap_or_default();

                let memory_config = CacheStrategyConfig {
                    cache_type: CacheType::Memory,
                    ttl_seconds: layered_config.memory_ttl,
                    max_entries: layered_config.memory_max_entries,
                    ..Default::default()
                };

                let redis_config = CacheStrategyConfig {
                    cache_type: CacheType::Redis,
                    ttl_seconds: layered_config.redis_ttl,
                    ..Default::default()
                };

                Box::new(LayeredCacheStrategy::new(
                    memory_config,
                    redis_client,
                    redis_config,
                    layered_config,
                ))
            }
            CacheType::Smart => {
                // 创建多个策略供智能选择
                let mut strategies: Vec<Box<dyn CacheStrategy>> = Vec::new();

                // 内存策略
                strategies.push(Box::new(MemoryCacheStrategy::new(config.clone())));

                // Redis策略（如果可用）
                if let Some(ref redis_client) = redis_client {
                    let redis_config = CacheStrategyConfig {
                        cache_type: CacheType::Redis,
                        ttl_seconds: config.ttl_seconds,
                        ..Default::default()
                    };
                    strategies.push(Box::new(RedisCacheStrategy::new(
                        redis_client.clone(),
                        redis_config,
                    )));
                }

                // 分层策略（如果Redis可用）
                if let Some(ref redis_client) = redis_client {
                    if let Some(ref layered_config) = config.layered_config {
                        let layered_strategy = Self::create_strategy(
                            CacheStrategyConfig {
                                cache_type: CacheType::Layered,
                                layered_config: Some(layered_config.clone()),
                                ..config.clone()
                            },
                            Some(redis_client.clone()),
                        );
                        strategies.push(layered_strategy);
                    }
                }

                Box::new(SmartCacheStrategy::new(strategies))
            }
        }
    }
}

impl Default for LayeredCacheConfig {
    fn default() -> Self {
        Self {
            memory_ttl: 60,  // 1分钟
            redis_ttl: 3600, // 1小时
            memory_max_entries: 1000,
        }
    }
}
