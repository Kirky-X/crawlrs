// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use crate::domain::models::search_result::SearchResult;
use crate::infrastructure::cache::stats_collector::CacheStatsCollector;
use crate::infrastructure::cache::types::CacheStats;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStrategyConfig {
    pub cache_type: CacheType,
    pub ttl_seconds: u64,
    pub max_entries: usize,
    pub enable_compression: bool,
    pub enable_preload: bool,
    pub preheat_config: Option<PreheatConfig>,
    pub layered_config: Option<LayeredCacheConfig>,
}

impl Default for CacheStrategyConfig {
    fn default() -> Self {
        Self {
            cache_type: CacheType::Memory,
            ttl_seconds: 300,
            max_entries: 10000,
            enable_compression: true,
            enable_preload: false,
            preheat_config: None,
            layered_config: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CacheType {
    Memory,
    Redis,
    Layered,
    Smart,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreheatConfig {
    pub hot_queries: Vec<String>,
    pub preheat_interval: u64,
    pub batch_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayeredCacheConfig {
    pub memory_ttl: u64,
    pub redis_ttl: u64,
    pub memory_max_entries: usize,
}

impl Default for LayeredCacheConfig {
    fn default() -> Self {
        Self {
            memory_ttl: 60,
            redis_ttl: 3600,
            memory_max_entries: 1000,
        }
    }
}

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

#[async_trait]
pub trait CacheStrategy: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Vec<SearchResult>>>;
    async fn set(&self, key: &str, value: Vec<SearchResult>, ttl: Option<Duration>) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn clear(&self) -> Result<()>;
    fn get_stats(&self) -> CacheStats;
    async fn preheat(&self, hot_data: Vec<(String, Vec<SearchResult>)>) -> Result<()>;
    async fn get_batch(&self, keys: &[String]) -> Result<Vec<Option<Vec<SearchResult>>>>;
    async fn set_batch(
        &self,
        entries: Vec<(String, Vec<SearchResult>)>,
        ttl: Option<Duration>,
    ) -> Result<()>;
}

pub struct MemoryCacheStrategy {
    cache: DashMap<String, CacheEntry<Vec<SearchResult>>>,
    config: CacheStrategyConfig,
    stats_collector: CacheStatsCollector,
}

impl MemoryCacheStrategy {
    pub fn new(config: CacheStrategyConfig) -> Self {
        Self {
            cache: DashMap::new(),
            config,
            stats_collector: CacheStatsCollector::new(),
        }
    }

    fn evict_if_needed(&self) {
        let current_size = self.cache.len();
        if current_size <= self.config.max_entries {
            return;
        }

        let to_evict = current_size - self.config.max_entries + (self.config.max_entries / 10);

        let mut entries: Vec<(String, f64)> = self
            .cache
            .iter()
            .map(|entry| {
                let score = entry.value().get_priority_score();
                (entry.key().clone(), score)
            })
            .collect();

        entries.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        for (key, _) in entries.iter().take(to_evict) {
            self.cache.remove(key);
        }

        self.stats_collector.record_evictions(to_evict);

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
                self.stats_collector.record_miss();
                return Ok(None);
            }

            entry.touch();
            self.stats_collector.record_hit();

            Ok(Some(entry.data.clone()))
        } else {
            self.stats_collector.record_miss();
            Ok(None)
        }
    }

    async fn set(&self, key: &str, value: Vec<SearchResult>, ttl: Option<Duration>) -> Result<()> {
        let ttl = ttl.unwrap_or(Duration::from_secs(self.config.ttl_seconds));
        let results_count = value.len();
        let entry = CacheEntry::new(value, ttl);

        self.cache.insert(key.to_string(), entry);
        self.evict_if_needed();

        self.stats_collector.record_store();

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
        self.stats_collector.reset();
        info!("Cleared all memory cache entries");
        Ok(())
    }

    fn get_stats(&self) -> CacheStats {
        self.stats_collector.snapshot()
    }

    async fn preheat(&self, hot_data: Vec<(String, Vec<SearchResult>)>) -> Result<()> {
        let entry_count = hot_data.len();
        info!("Preheating memory cache with {} hot entries", entry_count);

        for (key, results) in hot_data {
            let ttl = Duration::from_secs(self.config.ttl_seconds);
            self.set(&key, results, Some(ttl)).await?;
        }

        self.stats_collector.record_preheat_hits(entry_count);

        info!("Memory cache preheating completed");
        Ok(())
    }

    async fn get_batch(&self, keys: &[String]) -> Result<Vec<Option<Vec<SearchResult>>>> {
        let mut results = Vec::with_capacity(keys.len());

        for key in keys {
            if let Some(mut entry) = self.cache.get_mut(key) {
                if entry.is_expired() {
                    self.cache.remove(key);
                    self.stats_collector.record_miss();
                    results.push(None);
                } else {
                    entry.touch();
                    self.stats_collector.record_hit();
                    results.push(Some(entry.data.clone()));
                }
            } else {
                self.stats_collector.record_miss();
                results.push(None);
            }
        }
        Ok(results)
    }

    async fn set_batch(
        &self,
        entries: Vec<(String, Vec<SearchResult>)>,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let ttl = ttl.unwrap_or(Duration::from_secs(self.config.ttl_seconds));
        let count = entries.len();

        for (key, value) in entries {
            let entry = CacheEntry::new(value, ttl);
            self.cache.insert(key, entry);
        }

        self.evict_if_needed();
        for _ in 0..count {
            self.stats_collector.record_store();
        }
        Ok(())
    }
}

#[cfg(feature = "redis-cache")]
pub struct RedisCacheStrategy {
    redis_client: Arc<crate::infrastructure::cache::redis_client::RedisClient>,
    config: CacheStrategyConfig,
    stats_collector: CacheStatsCollector,
}

#[cfg(feature = "redis-cache")]
impl RedisCacheStrategy {
    pub fn new(
        redis_client: Arc<crate::infrastructure::cache::redis_client::RedisClient>,
        config: CacheStrategyConfig,
    ) -> Self {
        Self {
            redis_client,
            config,
            stats_collector: CacheStatsCollector::new(),
        }
    }

    fn generate_cache_key(&self, key: &str) -> String {
        format!("search_cache:{}", key)
    }
}

#[async_trait]
#[cfg(feature = "redis-cache")]
impl CacheStrategy for RedisCacheStrategy {
    async fn get(&self, key: &str) -> Result<Option<Vec<SearchResult>>> {
        let cache_key = self.generate_cache_key(key);

        match self.redis_client.get(&cache_key).await? {
            Some(json_str) => {
                let results: Vec<SearchResult> = serde_json::from_str(&json_str)?;

                self.stats_collector.record_hit();

                debug!("Cache hit for key: {}", key);
                Ok(Some(results))
            }
            None => {
                self.stats_collector.record_miss();

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

        self.stats_collector.record_store();

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
        warn!("Redis cache clear not implemented, would need to iterate over all keys");
        Ok(())
    }

    fn get_stats(&self) -> CacheStats {
        self.stats_collector.snapshot()
    }

    async fn preheat(&self, hot_data: Vec<(String, Vec<SearchResult>)>) -> Result<()> {
        let entry_count = hot_data.len();
        info!("Preheating Redis cache with {} hot entries", entry_count);

        for (key, results) in hot_data {
            let ttl = Duration::from_secs(self.config.ttl_seconds);
            self.set(&key, results, Some(ttl)).await?;
        }

        self.stats_collector.record_preheat_hits(entry_count);

        info!("Redis cache preheating completed");
        Ok(())
    }

    async fn get_batch(&self, keys: &[String]) -> Result<Vec<Option<Vec<SearchResult>>>> {
        let mut results = Vec::with_capacity(keys.len());

        for key in keys {
            match self.get(key).await {
                Ok(result) => results.push(result),
                Err(_) => results.push(None),
            }
        }

        Ok(results)
    }

    async fn set_batch(
        &self,
        entries: Vec<(String, Vec<SearchResult>)>,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let ttl = ttl.unwrap_or(Duration::from_secs(self.config.ttl_seconds));

        for (key, value) in entries {
            self.set(&key, value, Some(ttl)).await?;
        }

        Ok(())
    }
}

#[cfg(feature = "redis-cache")]
pub struct LayeredCacheStrategy {
    memory_cache: MemoryCacheStrategy,
    redis_cache: RedisCacheStrategy,
    config: LayeredCacheConfig,
    #[allow(dead_code)]
    _stats: Arc<Mutex<CacheStats>>,
}

#[cfg(feature = "redis-cache")]
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
            _stats: Arc::new(Mutex::new(CacheStats::default())),
        }
    }
}

#[async_trait]
#[cfg(feature = "redis-cache")]
impl CacheStrategy for LayeredCacheStrategy {
    async fn get(&self, key: &str) -> Result<Option<Vec<SearchResult>>> {
        if let Some(results) = self.memory_cache.get(key).await? {
            debug!("Layered cache hit in memory layer for key: {}", key);
            return Ok(Some(results));
        }

        if let Some(results) = self.redis_cache.get(key).await? {
            debug!("Layered cache hit in Redis layer for key: {}", key);

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
        let memory_ttl = Duration::from_secs(self.config.memory_ttl);
        let redis_ttl = ttl.unwrap_or(Duration::from_secs(self.config.redis_ttl));

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

        let memory_entries = hot_data.clone();
        let redis_entries = hot_data;

        let memory_future = self.memory_cache.preheat(memory_entries);
        let redis_future = self.redis_cache.preheat(redis_entries);

        tokio::try_join!(memory_future, redis_future)?;

        info!("Layered cache preheating completed");
        Ok(())
    }

    async fn get_batch(&self, keys: &[String]) -> Result<Vec<Option<Vec<SearchResult>>>> {
        let mut results = Vec::with_capacity(keys.len());

        for key in keys {
            if let Some(results_val) = self.memory_cache.get(key).await? {
                results.push(Some(results_val));
            } else if let Some(results_val) = self.redis_cache.get(key).await? {
                let memory_ttl = Duration::from_secs(self.config.memory_ttl);
                let results_for_cache = results_val.clone();
                let _ = self
                    .memory_cache
                    .set(key, results_for_cache, Some(memory_ttl))
                    .await;
                results.push(Some(results_val));
            } else {
                results.push(None);
            }
        }

        Ok(results)
    }

    async fn set_batch(
        &self,
        entries: Vec<(String, Vec<SearchResult>)>,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let memory_ttl = Duration::from_secs(self.config.memory_ttl);
        let redis_ttl = ttl.unwrap_or(Duration::from_secs(self.config.redis_ttl));

        let mut memory_entries = Vec::with_capacity(entries.len());
        let mut redis_entries = Vec::with_capacity(entries.len());

        for (key, value) in entries.into_iter() {
            let key_clone = key.clone();
            let value_clone = value.clone();
            memory_entries.push((key_clone, value_clone));
            redis_entries.push((key, value));
        }

        let memory_future = self
            .memory_cache
            .set_batch(memory_entries, Some(memory_ttl));
        let redis_future = self.redis_cache.set_batch(redis_entries, Some(redis_ttl));

        tokio::try_join!(memory_future, redis_future)?;
        Ok(())
    }
}

pub struct CacheStrategyFactory;

impl CacheStrategyFactory {
    #[cfg(feature = "redis-cache")]
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
                let mut strategies: Vec<Box<dyn CacheStrategy>> = Vec::new();

                strategies.push(Box::new(MemoryCacheStrategy::new(config.clone())));

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

pub struct SmartCacheStrategy {
    strategies: Vec<Box<dyn CacheStrategy>>,
    current_strategy: Arc<RwLock<usize>>,
    performance_history: Arc<Mutex<Vec<CachePerformance>>>,
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

#[allow(dead_code)]
pub struct CacheMonitor {
    stats: Arc<Mutex<CacheStats>>,
    _last_report: Instant,
}

impl CacheMonitor {
    pub fn new(stats: Arc<Mutex<CacheStats>>) -> Self {
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
            current_strategy: Arc::new(RwLock::new(0)),
            performance_history: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn select_optimal_strategy(&self) -> usize {
        let history = self
            .performance_history
            .lock()
            .expect("Performance history lock poisoned");
        if history.is_empty() {
            return 0;
        }

        let mut strategy_scores: Vec<(usize, f64)> = (0..self.strategies.len())
            .map(|i| {
                let relevant_history: Vec<&CachePerformance> =
                    history.iter().filter(|p| p.strategy_index == i).collect();

                if relevant_history.is_empty() {
                    (i, 0.5)
                } else {
                    let avg_hit_rate = relevant_history.iter().map(|p| p.hit_rate).sum::<f64>()
                        / relevant_history.len() as f64;
                    let avg_latency = relevant_history.iter().map(|p| p.latency_ms).sum::<f64>()
                        / relevant_history.len() as f64;

                    let score = avg_hit_rate * 0.7 + (1000.0 / (avg_latency + 1.0)) * 0.3;
                    (i, score)
                }
            })
            .collect();

        strategy_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        strategy_scores[0].0
    }

    fn record_performance(&self, strategy_index: usize, hit_rate: f64, latency_ms: f64) {
        let mut history = self
            .performance_history
            .lock()
            .expect("Performance history lock poisoned");
        history.push(CachePerformance {
            strategy_index,
            hit_rate,
            latency_ms,
            _memory_usage: 0.0,
            _timestamp: Instant::now(),
        });

        if history.len() > 1000 {
            history.drain(0..100);
        }
    }
}

#[async_trait]
impl CacheStrategy for SmartCacheStrategy {
    async fn get(&self, key: &str) -> Result<Option<Vec<SearchResult>>> {
        let start = Instant::now();
        let current_index = *self
            .current_strategy
            .read()
            .expect("Current strategy lock poisoned");

        let result: Option<Vec<SearchResult>> = self.strategies[current_index].get(key).await?;
        let latency_ms = start.elapsed().as_millis() as f64;

        let hit_rate = if result.is_some() { 1.0 } else { 0.0 };
        self.record_performance(current_index, hit_rate, latency_ms);

        if rand::random::<f64>() < 0.01 {
            let optimal_index = self.select_optimal_strategy();
            if optimal_index != current_index {
                *self
                    .current_strategy
                    .write()
                    .expect("Current strategy lock poisoned") = optimal_index;
                info!("Switched to cache strategy index: {}", optimal_index);
            }
        }

        Ok(result)
    }

    async fn set(&self, key: &str, value: Vec<SearchResult>, ttl: Option<Duration>) -> Result<()> {
        let current_index = *self
            .current_strategy
            .read()
            .expect("Current strategy lock poisoned");
        self.strategies[current_index].set(key, value, ttl).await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let current_index = *self
            .current_strategy
            .read()
            .expect("Current strategy lock poisoned");
        self.strategies[current_index].delete(key).await
    }

    async fn clear(&self) -> Result<()> {
        for strategy in &self.strategies {
            strategy.clear().await?;
        }
        Ok(())
    }

    fn get_stats(&self) -> CacheStats {
        let current_index = *self
            .current_strategy
            .read()
            .expect("Current strategy lock poisoned");
        self.strategies[current_index].get_stats()
    }

    async fn preheat(&self, hot_data: Vec<(String, Vec<SearchResult>)>) -> Result<()> {
        let current_index = *self
            .current_strategy
            .read()
            .expect("Current strategy lock poisoned");
        self.strategies[current_index].preheat(hot_data).await
    }

    async fn get_batch(&self, keys: &[String]) -> Result<Vec<Option<Vec<SearchResult>>>> {
        let current_index = *self
            .current_strategy
            .read()
            .expect("Current strategy lock poisoned");
        self.strategies[current_index].get_batch(keys).await
    }

    async fn set_batch(
        &self,
        entries: Vec<(String, Vec<SearchResult>)>,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let current_index = *self
            .current_strategy
            .read()
            .expect("Current strategy lock poisoned");
        self.strategies[current_index].set_batch(entries, ttl).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_entry_new() {
        let data = vec!["test_data".to_string()];
        let entry = CacheEntry::new(data.clone(), Duration::from_secs(300));

        assert!(!entry.is_expired());
        assert_eq!(entry.access_count, 0);
        assert_eq!(entry.data, data);
    }

    #[test]
    fn test_cache_entry_is_expired() {
        let data = vec!["test_data".to_string()];
        let entry = CacheEntry::new(data, Duration::from_secs(0));
        assert!(entry.is_expired());
    }

    #[test]
    fn test_cache_entry_touch() {
        let data = vec!["test_data".to_string()];
        let mut entry = CacheEntry::new(data, Duration::from_secs(300));

        assert_eq!(entry.access_count, 0);
        entry.touch();
        assert_eq!(entry.access_count, 1);
        entry.touch();
        assert_eq!(entry.access_count, 2);
    }

    #[test]
    fn test_cache_entry_priority_score() {
        let data = vec!["test_data".to_string()];
        let mut entry = CacheEntry::new(data, Duration::from_secs(300));

        let initial_score = entry.get_priority_score();

        entry.touch();
        entry.touch();

        let after_touch_score = entry.get_priority_score();
        assert!(after_touch_score > initial_score);
    }

    #[test]
    fn test_cache_strategy_config_default() {
        let config = CacheStrategyConfig::default();
        match config.cache_type {
            CacheType::Memory => assert!(true),
            _ => panic!("Expected Memory cache type"),
        }
        assert_eq!(config.ttl_seconds, 300);
        assert_eq!(config.max_entries, 10000);
        assert!(config.enable_compression);
        assert!(!config.enable_preload);
        assert!(config.preheat_config.is_none());
        assert!(config.layered_config.is_none());
    }

    #[test]
    fn test_cache_strategy_config_custom() {
        let preheat_config = PreheatConfig {
            hot_queries: vec!["query1".to_string(), "query2".to_string()],
            preheat_interval: 60,
            batch_size: 10,
        };

        let layered_config = LayeredCacheConfig {
            memory_ttl: 120,
            redis_ttl: 7200,
            memory_max_entries: 5000,
        };

        let config = CacheStrategyConfig {
            cache_type: CacheType::Layered,
            ttl_seconds: 600,
            max_entries: 5000,
            enable_compression: false,
            enable_preload: true,
            preheat_config: Some(preheat_config),
            layered_config: Some(layered_config),
        };

        match config.cache_type {
            CacheType::Layered => assert!(true),
            _ => panic!("Expected Layered cache type"),
        }
        assert_eq!(config.ttl_seconds, 600);
        assert!(!config.enable_compression);
        assert!(config.enable_preload);
        assert!(config.preheat_config.is_some());
        assert!(config.layered_config.is_some());
    }

    #[test]
    fn test_cache_type_display() {
        let memory = CacheType::Memory;
        let redis = CacheType::Redis;
        let layered = CacheType::Layered;
        let smart = CacheType::Smart;

        assert_eq!(format!("{:?}", memory), "Memory");
        assert_eq!(format!("{:?}", redis), "Redis");
        assert_eq!(format!("{:?}", layered), "Layered");
        assert_eq!(format!("{:?}", smart), "Smart");
    }

    #[test]
    fn test_layered_cache_config_default() {
        let config = LayeredCacheConfig::default();
        assert_eq!(config.memory_ttl, 60);
        assert_eq!(config.redis_ttl, 3600);
        assert_eq!(config.memory_max_entries, 1000);
    }

    #[test]
    fn test_memory_cache_strategy_stats() {
        let config = CacheStrategyConfig::default();
        let strategy = MemoryCacheStrategy::new(config);

        let stats = strategy.get_stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.evictions, 0);
        assert_eq!(stats.stores, 0);
    }
}
