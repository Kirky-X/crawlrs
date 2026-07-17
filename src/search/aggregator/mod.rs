// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! SearchAggregator - Search result aggregation and caching
//!
//! This module handles aggregating results from multiple search engines
//! with caching and deduplication support.

pub mod deduplicator;

use async_trait::async_trait;
use dashmap::DashMap;
use futures::future::join_all;
use log::{info, warn};
use lru::LruCache;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crate::common::constants::cache_config;
use crate::domain::models::search_result::SearchResult;
use crate::domain::services::relevance_scorer::{DateParserComponent, RelevanceScorer};
use crate::search::aggregator::deduplicator::ResultDeduplicator;
use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::types::{EngineHealth, SearchEngineType};

/// 缓存键类型
type CacheEntry = (Vec<SearchResult>, Instant);

pub struct SearchAggregator {
    engines: Vec<Arc<dyn SearchEngine>>,
    timeout: Duration,
    // 使用LRU缓存替代无界DashMap，防止内存泄漏
    cache: Arc<tokio::sync::Mutex<LruCache<String, CacheEntry>>>,
    cache_ttl: Duration,
    failures: std::sync::Arc<DashMap<String, u32>>,
}

impl fmt::Debug for SearchAggregator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchAggregator")
            .field("engine_count", &self.engines.len())
            .field("timeout_ms", &self.timeout.as_millis())
            .finish_non_exhaustive()
    }
}

impl SearchAggregator {
    pub fn new(engines: Vec<Arc<dyn SearchEngine>>, timeout_ms: u64) -> Self {
        Self {
            engines,
            timeout: Duration::from_millis(timeout_ms),
            // 使用LRU缓存，自动淘汰旧条目，防止内存无限增长
            cache: Arc::new(tokio::sync::Mutex::new(LruCache::new(
                NonZeroUsize::new(cache_config::MAX_CACHE_ENTRIES)
                    .expect("MAX_CACHE_ENTRIES must be greater than 0"),
            ))),
            cache_ttl: Duration::from_secs(cache_config::DEFAULT_TTL_SECS),
            failures: std::sync::Arc::new(DashMap::new()),
        }
    }

    pub fn get_engine(&self, name: &str) -> Option<Arc<dyn SearchEngine>> {
        self.engines
            .iter()
            .find(|e| e.name().eq_ignore_ascii_case(name))
            .cloned()
    }

    pub fn engine_names(&self) -> Vec<&'static str> {
        self.engines.iter().map(|e| e.name()).collect()
    }

    pub async fn search_with_engine(
        &self,
        request: &SearchRequest,
        engine_name: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        if let Some(name) = engine_name {
            info!("search_with_engine called with engine_name: {}", name);
            let registered_names: Vec<&str> = self.engine_names();
            info!("Registered engine names: {:?}", registered_names);
            let target_engine = self.get_engine(name);
            match target_engine {
                Some(engine) => {
                    info!(
                        "Directly calling search engine: {} (actual name: {})",
                        name,
                        engine.name()
                    );
                    let response = engine.search(request).await?;
                    Ok(self.convert_response_to_results(response))
                }
                None => {
                    warn!("Engine '{}' not found, falling back to aggregator", name);
                    // Convert Response<ResponseItem> to Vec<SearchResult>
                    let response = self.search(request).await?;
                    Ok(self.convert_response_to_results(response))
                }
            }
        } else {
            let response = self.search(request).await?;
            Ok(self.convert_response_to_results(response))
        }
    }

    fn convert_response_to_results(&self, response: Response<ResponseItem>) -> Vec<SearchResult> {
        response
            .items
            .into_iter()
            .map(|item| SearchResult {
                title: item.title,
                url: item.url,
                description: Some(item.description),
                engine: item.engine.name().to_string(),
                score: 0.0,
                published_time: None,
            })
            .collect()
    }

    fn convert_results_to_response(&self, results: Vec<SearchResult>) -> Response<ResponseItem> {
        let items = results
            .into_iter()
            .map(|r| ResponseItem {
                title: r.title,
                url: r.url,
                description: r.description.unwrap_or_default(),
                engine: SearchEngineType::from_name(&r.engine).unwrap_or(SearchEngineType::Auto),
            })
            .collect();

        Response {
            items,
            total_results: None,
            engine: SearchEngineType::Auto,
        }
    }

    // Helper method for deduplication and ranking with PRD-compliant relevance scoring
    fn deduplicate_and_rank(&self, results: Vec<SearchResult>, query: &str) -> Vec<SearchResult> {
        // 去重委托给 ResultDeduplicator（SimHash + 指纹索引，O(n) 复杂度）
        // 替代原 O(n²) Jaro-Winkler 内联实现
        let mut dedup = ResultDeduplicator::with_default_config();
        let mut unique_results = dedup.deduplicate(results);

        // 评分 + 日期提取（业务逻辑保留在 aggregator）
        let scorer = RelevanceScorer::for_query(query);
        for result in &mut unique_results {
            let relevance_score =
                scorer.calculate_score(&result.title, result.description.as_deref(), &result.url);

            if result.published_time.is_none() {
                let combined_text = format!(
                    "{} {}",
                    result.title,
                    result.description.as_deref().unwrap_or("")
                );
                let parser = DateParserComponent::with_defaults();
                if let Some(published_date) =
                    RelevanceScorer::extract_published_date_with_parser(&combined_text, &parser)
                {
                    result.published_time = Some(published_date);
                }
            }

            let freshness_score = if let Some(published_time) = result.published_time {
                RelevanceScorer::calculate_freshness_score(published_time)
            } else {
                0.5 // Default freshness score for unknown dates
            };

            // Combine relevance and freshness scores (70% relevance, 30% freshness)
            result.score = relevance_score * 0.7 + freshness_score * 0.3;
        }

        // Sort by final score (highest first)
        unique_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        unique_results
    }
}

#[async_trait]
impl SearchEngine for SearchAggregator {
    fn name(&self) -> &'static str {
        "aggregator"
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Auto
    }

    fn health(&self) -> EngineHealth {
        // 检查是否有可用的引擎
        if self.engines.is_empty() {
            return EngineHealth::Unhealthy;
        }

        // 检查是否有引擎处于不健康状态
        let mut unhealthy_count = 0;
        let total_count = self.engines.len();

        for engine in &self.engines {
            match engine.health() {
                EngineHealth::Unhealthy | EngineHealth::Isolated => {
                    unhealthy_count += 1;
                }
                EngineHealth::Degraded => {
                    // 部分失败也算作降级
                    unhealthy_count += 1;
                }
                EngineHealth::Healthy | EngineHealth::Unknown => {}
            }
        }

        // 如果超过 50% 的引擎不健康，标记为降级
        if unhealthy_count > 0 {
            if unhealthy_count >= total_count / 2 {
                EngineHealth::Degraded
            } else {
                EngineHealth::Healthy
            }
        } else {
            EngineHealth::Healthy
        }
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        // Check cache - include all dimensions to avoid cache pollution
        let cache_key = format!(
            "{}:{}:{}:{}:{}",
            request.query,
            request.limit,
            request.offset,
            request.lang.as_deref().unwrap_or("default"),
            request.country.as_deref().unwrap_or("default")
        );

        // 使用LRU缓存的get方法
        {
            let mut cache = self.cache.lock().await;
            if let Some((cached_results, timestamp)) = cache.get(&cache_key) {
                if timestamp.elapsed() < self.cache_ttl {
                    info!("Cache hit for query: {}", request.query);
                    let mut results = cached_results.clone();
                    // Apply limit to cached results
                    if results.len() > request.limit as usize {
                        results.truncate(request.limit as usize);
                    }
                    return Ok(self.convert_results_to_response(results));
                }
            }
        }

        let futures = self.engines.iter().map(|engine| {
            let engine = engine.clone();
            let request = request.clone();
            let failures = self.failures.clone();

            async move {
                let engine_name = engine.name();
                // Check circuit breaker
                if let Some(count) = failures.get(engine_name) {
                    if *count >= 3 {
                        warn!(
                            "Engine {} circuit broken ({} failures)",
                            engine_name, *count
                        );
                        return None;
                    }
                }

                let result = tokio::time::timeout(self.timeout, engine.search(&request)).await;

                match result {
                    Ok(Ok(response)) => {
                        info!(
                            "Engine {} returned {} results",
                            engine_name,
                            response.items.len()
                        );
                        // Reset failure count on success
                        if failures.contains_key(engine_name) {
                            failures.remove(engine_name);
                        }
                        Some(response)
                    }
                    Ok(Err(e)) => {
                        warn!("Engine {} failed: {}", engine_name, e);
                        let mut count = failures.entry(engine_name.to_string()).or_insert(0);
                        *count += 1;
                        None
                    }
                    Err(_) => {
                        warn!("Engine {} timed out", engine_name);
                        let mut count = failures.entry(engine_name.to_string()).or_insert(0);
                        *count += 1;
                        None
                    }
                }
            }
        });

        let responses: Vec<Response<ResponseItem>> =
            join_all(futures).await.into_iter().flatten().collect();

        // Convert to SearchResult for deduplication
        let mut all_results: Vec<SearchResult> = Vec::new();
        for response in responses {
            all_results.extend(self.convert_response_to_results(response));
        }

        let final_results = self.deduplicate_and_rank(all_results, &request.query);

        // 使用LRU缓存的push方法，自动淘汰旧条目
        let mut cache = self.cache.lock().await;
        cache.push(cache_key.clone(), (final_results.clone(), Instant::now()));

        Ok(self.convert_results_to_response(final_results))
    }

    /// Override search_with_engine to support specific engine selection
    async fn search_with_engine(
        &self,
        query: &str,
        limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
        engine: Option<&str>,
    ) -> Result<Vec<ResponseItem>, SearchError> {
        if let Some(name) = engine {
            warn!(
                "[DEBUG] search_with_engine called with engine_name: {}",
                name
            );
            let registered_names: Vec<&str> = self.engine_names();
            warn!("[DEBUG] Registered engine names: {:?}", registered_names);
            let target_engine = self.get_engine(name);
            match target_engine {
                Some(e) => {
                    warn!(
                        "[DEBUG] Directly calling search engine: {} (actual name: {})",
                        name,
                        e.name()
                    );
                    let request = SearchRequest::new(query).with_limit(limit);
                    let response = e.search(&request).await?;
                    Ok(response.items)
                }
                None => {
                    warn!("Engine '{}' not found, falling back to aggregator", name);
                    // Fall back to searching all engines
                    let request = SearchRequest::new(query).with_limit(limit);
                    let response = self.search(&request).await?;
                    Ok(response.items)
                }
            }
        } else {
            // Search all engines
            let request = SearchRequest::new(query).with_limit(limit);
            let response = self.search(&request).await?;
            Ok(response.items)
        }
    }
}

#[cfg(test)]
mod tests {
    // Copyright (c) 2025 Kirky.X
    use super::*;
    use crate::search::engine_trait::SearchRequest;
    use crate::search::response::{Response, ResponseItem};
    use crate::search::types::{EngineHealth, SearchEngineType};
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    // ========== Mock SearchEngine for aggregator tests ==========

    /// Configurable mock engine that can return success, failure, or simulate a timeout.
    struct MockAggEngine {
        name: &'static str,
        engine_type: SearchEngineType,
        items: Vec<ResponseItem>,
        health: EngineHealth,
        /// If true, `search()` returns `SearchError::NoEngineAvailable`.
        fail: bool,
        /// If true, `search()` sleeps longer than the aggregator timeout.
        slow: bool,
        /// Records the number of times `search()` was called.
        call_count: Mutex<u32>,
    }

    impl MockAggEngine {
        fn healthy(
            name: &'static str,
            engine_type: SearchEngineType,
            items: Vec<ResponseItem>,
        ) -> Self {
            Self {
                name,
                engine_type,
                items,
                health: EngineHealth::Healthy,
                fail: false,
                slow: false,
                call_count: Mutex::new(0),
            }
        }

        fn unhealthy(name: &'static str, engine_type: SearchEngineType) -> Self {
            Self {
                name,
                engine_type,
                items: Vec::new(),
                health: EngineHealth::Unhealthy,
                fail: false,
                slow: false,
                call_count: Mutex::new(0),
            }
        }

        fn failing(name: &'static str, engine_type: SearchEngineType) -> Self {
            Self {
                name,
                engine_type,
                items: Vec::new(),
                health: EngineHealth::Healthy,
                fail: true,
                slow: false,
                call_count: Mutex::new(0),
            }
        }

        #[allow(dead_code)]
        fn slow(name: &'static str, engine_type: SearchEngineType) -> Self {
            Self {
                name,
                engine_type,
                items: Vec::new(),
                health: EngineHealth::Healthy,
                fail: false,
                slow: true,
                call_count: Mutex::new(0),
            }
        }

        fn call_count(&self) -> u32 {
            *self.call_count.lock().unwrap()
        }
    }

    #[async_trait]
    impl SearchEngine for MockAggEngine {
        fn name(&self) -> &'static str {
            self.name
        }

        fn engine_type(&self) -> SearchEngineType {
            self.engine_type
        }

        fn health(&self) -> EngineHealth {
            self.health
        }

        async fn search(
            &self,
            _request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            {
                let mut count = self.call_count.lock().unwrap();
                *count += 1;
            }
            if self.slow {
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            if self.fail {
                return Err(SearchError::NoEngineAvailable);
            }
            Ok(Response {
                items: self.items.clone(),
                total_results: Some(self.items.len() as u64),
                engine: self.engine_type,
            })
        }
    }

    fn make_item(title: &str, url: &str, engine: SearchEngineType) -> ResponseItem {
        ResponseItem {
            title: title.to_string(),
            url: url.to_string(),
            description: format!("desc for {}", title),
            engine,
        }
    }

    fn make_engines(engines: Vec<Arc<dyn SearchEngine>>) -> Vec<Arc<dyn SearchEngine>> {
        engines
    }

    // ========== Construction & basic accessor tests ==========

    #[test]
    fn test_new_aggregator_with_no_engines() {
        let agg = SearchAggregator::new(vec![], 5000);
        assert!(agg.engine_names().is_empty());
    }

    #[test]
    fn test_new_aggregator_preserves_engine_names() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![
            Arc::new(MockAggEngine::healthy(
                "google",
                SearchEngineType::Google,
                vec![],
            )),
            Arc::new(MockAggEngine::healthy(
                "bing",
                SearchEngineType::Bing,
                vec![],
            )),
        ];
        let agg = SearchAggregator::new(engines, 5000);
        let names = agg.engine_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"google"));
        assert!(names.contains(&"bing"));
    }

    #[test]
    fn test_get_engine_found_case_insensitive() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![Arc::new(MockAggEngine::healthy(
            "google",
            SearchEngineType::Google,
            vec![],
        ))];
        let agg = SearchAggregator::new(engines, 5000);

        assert!(
            agg.get_engine("google").is_some(),
            "exact match should work"
        );
        assert!(
            agg.get_engine("Google").is_some(),
            "case-insensitive match should work"
        );
        assert!(
            agg.get_engine("GOOGLE").is_some(),
            "uppercase match should work"
        );
    }

    #[test]
    fn test_get_engine_not_found() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![Arc::new(MockAggEngine::healthy(
            "google",
            SearchEngineType::Google,
            vec![],
        ))];
        let agg = SearchAggregator::new(engines, 5000);
        assert!(
            agg.get_engine("yahoo").is_none(),
            "unknown engine should return None"
        );
    }

    #[test]
    fn test_get_engine_empty_aggregator() {
        let agg = SearchAggregator::new(vec![], 5000);
        assert!(agg.get_engine("google").is_none());
    }

    // ========== Debug impl tests ==========

    #[test]
    fn test_debug_shows_engine_count() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![
            Arc::new(MockAggEngine::healthy(
                "google",
                SearchEngineType::Google,
                vec![],
            )),
            Arc::new(MockAggEngine::healthy(
                "bing",
                SearchEngineType::Bing,
                vec![],
            )),
        ];
        let agg = SearchAggregator::new(engines, 3000);
        let debug_str = format!("{:?}", agg);
        assert!(debug_str.contains("engine_count"));
        assert!(debug_str.contains("2"), "debug should show engine_count=2");
        assert!(debug_str.contains("timeout_ms"));
    }

    #[test]
    fn test_debug_empty_aggregator() {
        let agg = SearchAggregator::new(vec![], 1000);
        let debug_str = format!("{:?}", agg);
        assert!(debug_str.contains("SearchAggregator"));
    }

    // ========== SearchEngine trait impl tests ==========

    #[test]
    fn test_aggregator_name() {
        let agg = SearchAggregator::new(vec![], 5000);
        assert_eq!(agg.name(), "aggregator");
    }

    #[test]
    fn test_aggregator_engine_type_is_auto() {
        let agg = SearchAggregator::new(vec![], 5000);
        assert_eq!(agg.engine_type(), SearchEngineType::Auto);
    }

    // ========== health() tests ==========

    #[test]
    fn test_health_empty_engines_is_unhealthy() {
        let agg = SearchAggregator::new(vec![], 5000);
        assert_eq!(agg.health(), EngineHealth::Unhealthy);
    }

    #[test]
    fn test_health_all_healthy_engines() {
        let engines: Vec<Arc<dyn SearchEngine>> = vec![
            Arc::new(MockAggEngine::healthy(
                "google",
                SearchEngineType::Google,
                vec![],
            )),
            Arc::new(MockAggEngine::healthy(
                "bing",
                SearchEngineType::Bing,
                vec![],
            )),
        ];
        let agg = SearchAggregator::new(engines, 5000);
        assert_eq!(agg.health(), EngineHealth::Healthy);
    }

    #[test]
    fn test_health_majority_unhealthy_is_degraded() {
        // 2 out of 2 engines unhealthy → 100% >= 50% → Degraded
        let engines: Vec<Arc<dyn SearchEngine>> = vec![
            Arc::new(MockAggEngine::unhealthy("google", SearchEngineType::Google)),
            Arc::new(MockAggEngine::unhealthy("bing", SearchEngineType::Bing)),
        ];
        let agg = SearchAggregator::new(engines, 5000);
        assert_eq!(agg.health(), EngineHealth::Degraded);
    }

    #[test]
    fn test_health_half_unhealthy_is_degraded() {
        // 1 out of 2 engines unhealthy → 1 >= 2/2=1 → Degraded
        let engines: Vec<Arc<dyn SearchEngine>> = vec![
            Arc::new(MockAggEngine::healthy(
                "google",
                SearchEngineType::Google,
                vec![],
            )),
            Arc::new(MockAggEngine::unhealthy("bing", SearchEngineType::Bing)),
        ];
        let agg = SearchAggregator::new(engines, 5000);
        assert_eq!(agg.health(), EngineHealth::Degraded);
    }

    #[test]
    fn test_health_minority_unhealthy_is_healthy() {
        // 1 out of 3 engines unhealthy → 1 < 3/2=1 → Healthy (minority)
        // Note: 3/2 = 1 in integer division, and 1 >= 1, so this is Degraded
        // Let's use 1 out of 4 instead: 1 < 4/2=2 → Healthy
        let engines: Vec<Arc<dyn SearchEngine>> = vec![
            Arc::new(MockAggEngine::healthy(
                "google",
                SearchEngineType::Google,
                vec![],
            )),
            Arc::new(MockAggEngine::healthy(
                "bing",
                SearchEngineType::Bing,
                vec![],
            )),
            Arc::new(MockAggEngine::healthy(
                "baidu",
                SearchEngineType::Baidu,
                vec![],
            )),
            Arc::new(MockAggEngine::unhealthy("sogou", SearchEngineType::Sogou)),
        ];
        let agg = SearchAggregator::new(engines, 5000);
        assert_eq!(
            agg.health(),
            EngineHealth::Healthy,
            "1 out of 4 unhealthy should be Healthy"
        );
    }

    // ========== search() tests ==========

    #[tokio::test]
    async fn test_search_empty_aggregator_returns_empty() {
        let agg = SearchAggregator::new(vec![], 5000);
        let request = SearchRequest::new("test").with_limit(10);
        let response = agg.search(&request).await.expect("search should succeed");
        assert!(
            response.items.is_empty(),
            "empty aggregator should return no results"
        );
    }

    #[tokio::test]
    async fn test_search_single_engine_returns_results() {
        let items = vec![make_item(
            "Rust Guide",
            "https://rust-lang.org",
            SearchEngineType::Google,
        )];
        let engines = make_engines(vec![Arc::new(MockAggEngine::healthy(
            "google",
            SearchEngineType::Google,
            items,
        ))]);
        let agg = SearchAggregator::new(engines, 5000);

        let request = SearchRequest::new("rust").with_limit(10);
        let response = agg.search(&request).await.expect("search should succeed");
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].title, "Rust Guide");
    }

    #[tokio::test]
    async fn test_search_deduplicates_by_url() {
        // Two engines return the same URL → should be deduplicated to 1 result.
        let items_a = vec![make_item(
            "Rust Guide",
            "https://rust-lang.org",
            SearchEngineType::Google,
        )];
        let items_b = vec![make_item(
            "Rust Guide",
            "https://rust-lang.org",
            SearchEngineType::Bing,
        )];
        let engines = make_engines(vec![
            Arc::new(MockAggEngine::healthy(
                "google",
                SearchEngineType::Google,
                items_a,
            )),
            Arc::new(MockAggEngine::healthy(
                "bing",
                SearchEngineType::Bing,
                items_b,
            )),
        ]);
        let agg = SearchAggregator::new(engines, 5000);

        let request = SearchRequest::new("rust").with_limit(10);
        let response = agg.search(&request).await.expect("search should succeed");
        assert_eq!(
            response.items.len(),
            1,
            "duplicate URLs should be deduplicated"
        );
    }

    #[tokio::test]
    async fn test_search_deduplicates_by_title_similarity() {
        // Same title, different URLs → Jaro-Winkler > 0.9 → deduplicated.
        let items_a = vec![make_item(
            "Rust Programming Language Guide",
            "https://a.com",
            SearchEngineType::Google,
        )];
        let items_b = vec![make_item(
            "Rust Programming Language Guide",
            "https://b.com",
            SearchEngineType::Bing,
        )];
        let engines = make_engines(vec![
            Arc::new(MockAggEngine::healthy(
                "google",
                SearchEngineType::Google,
                items_a,
            )),
            Arc::new(MockAggEngine::healthy(
                "bing",
                SearchEngineType::Bing,
                items_b,
            )),
        ]);
        let agg = SearchAggregator::new(engines, 5000);

        let request = SearchRequest::new("rust").with_limit(10);
        let response = agg.search(&request).await.expect("search should succeed");
        assert_eq!(
            response.items.len(),
            1,
            "highly similar titles should be deduplicated"
        );
    }

    #[tokio::test]
    async fn test_search_keeps_different_results() {
        // Different titles and URLs → both kept.
        let items_a = vec![make_item(
            "Rust Guide",
            "https://rust-lang.org",
            SearchEngineType::Google,
        )];
        let items_b = vec![make_item(
            "Python Guide",
            "https://python.org",
            SearchEngineType::Bing,
        )];
        let engines = make_engines(vec![
            Arc::new(MockAggEngine::healthy(
                "google",
                SearchEngineType::Google,
                items_a,
            )),
            Arc::new(MockAggEngine::healthy(
                "bing",
                SearchEngineType::Bing,
                items_b,
            )),
        ]);
        let agg = SearchAggregator::new(engines, 5000);

        let request = SearchRequest::new("programming").with_limit(10);
        let response = agg.search(&request).await.expect("search should succeed");
        assert_eq!(
            response.items.len(),
            2,
            "distinct results should both be kept"
        );
    }

    #[tokio::test]
    async fn test_search_caches_results() {
        // The cache key includes query, limit, offset, lang, country.
        // A second search with the same parameters should hit the cache and
        // NOT call the engine again.
        let mock = Arc::new(MockAggEngine::healthy(
            "google",
            SearchEngineType::Google,
            vec![make_item(
                "Cached",
                "https://cached.com",
                SearchEngineType::Google,
            )],
        ));
        let engines = make_engines(vec![mock.clone()]);
        let agg = SearchAggregator::new(engines, 5000);

        let request = SearchRequest::new("cached-query").with_limit(10);
        let response1 = agg
            .search(&request)
            .await
            .expect("first search should succeed");
        assert_eq!(response1.items.len(), 1);
        assert_eq!(
            mock.call_count(),
            1,
            "engine should be called once on first search"
        );

        let response2 = agg
            .search(&request)
            .await
            .expect("second search should succeed");
        assert_eq!(
            response2.items.len(),
            1,
            "cached search should return the same result"
        );
        assert_eq!(
            mock.call_count(),
            1,
            "engine should NOT be called again on cached search"
        );
    }

    #[tokio::test]
    async fn test_search_different_queries_bypass_cache() {
        let mock = Arc::new(MockAggEngine::healthy(
            "google",
            SearchEngineType::Google,
            vec![make_item(
                "Result",
                "https://r.com",
                SearchEngineType::Google,
            )],
        ));
        let engines = make_engines(vec![mock.clone()]);
        let agg = SearchAggregator::new(engines, 5000);

        let req1 = SearchRequest::new("query-a").with_limit(10);
        let req2 = SearchRequest::new("query-b").with_limit(10);

        agg.search(&req1).await.unwrap();
        assert_eq!(mock.call_count(), 1, "first query should call the engine");

        agg.search(&req2).await.unwrap();
        assert_eq!(
            mock.call_count(),
            2,
            "different query should bypass cache and call the engine again"
        );
    }

    #[tokio::test]
    async fn test_search_continues_when_engine_fails() {
        // One engine fails, the other succeeds → results from the working engine.
        let engines = make_engines(vec![
            Arc::new(MockAggEngine::failing("google", SearchEngineType::Google)),
            Arc::new(MockAggEngine::healthy(
                "bing",
                SearchEngineType::Bing,
                vec![make_item(
                    "Bing Result",
                    "https://bing.com",
                    SearchEngineType::Bing,
                )],
            )),
        ]);
        let agg = SearchAggregator::new(engines, 5000);

        let request = SearchRequest::new("test").with_limit(10);
        let response = agg.search(&request).await.expect("search should succeed");
        assert_eq!(
            response.items.len(),
            1,
            "should return results from the working engine"
        );
        assert_eq!(response.items[0].title, "Bing Result");
    }

    #[tokio::test]
    async fn test_search_all_engines_fail_returns_empty() {
        let engines = make_engines(vec![
            Arc::new(MockAggEngine::failing("google", SearchEngineType::Google)),
            Arc::new(MockAggEngine::failing("bing", SearchEngineType::Bing)),
        ]);
        let agg = SearchAggregator::new(engines, 5000);

        let request = SearchRequest::new("test").with_limit(10);
        let response = agg.search(&request).await.expect("search should succeed");
        assert!(
            response.items.is_empty(),
            "all engines failing should yield empty results"
        );
    }

    // ========== search_with_engine (trait method) tests ==========
    // The inherent `search_with_engine` method shadows the trait method,
    // so we use fully qualified syntax to call the trait method.

    #[tokio::test]
    async fn test_search_with_engine_specific_found() {
        let google_items = vec![make_item(
            "Google Result",
            "https://google.com",
            SearchEngineType::Google,
        )];
        let bing_items = vec![make_item(
            "Bing Result",
            "https://bing.com",
            SearchEngineType::Bing,
        )];
        let engines = make_engines(vec![
            Arc::new(MockAggEngine::healthy(
                "google",
                SearchEngineType::Google,
                google_items,
            )),
            Arc::new(MockAggEngine::healthy(
                "bing",
                SearchEngineType::Bing,
                bing_items,
            )),
        ]);
        let agg = SearchAggregator::new(engines, 5000);

        let items = <SearchAggregator as SearchEngine>::search_with_engine(
            &agg,
            "test",
            10,
            None,
            None,
            Some("google"),
        )
        .await
        .expect("should succeed with specific engine");

        assert_eq!(
            items.len(),
            1,
            "should return only the google engine's results"
        );
        assert_eq!(items[0].title, "Google Result");
    }

    #[tokio::test]
    async fn test_search_with_engine_not_found_falls_back() {
        let engines = make_engines(vec![Arc::new(MockAggEngine::healthy(
            "google",
            SearchEngineType::Google,
            vec![make_item(
                "G Result",
                "https://g.com",
                SearchEngineType::Google,
            )],
        ))]);
        let agg = SearchAggregator::new(engines, 5000);

        let items = <SearchAggregator as SearchEngine>::search_with_engine(
            &agg,
            "test",
            10,
            None,
            None,
            Some("nonexistent"),
        )
        .await
        .expect("should fall back to aggregator search");

        assert_eq!(
            items.len(),
            1,
            "fallback should search all engines and return their results"
        );
    }

    #[tokio::test]
    async fn test_search_with_engine_none_searches_all() {
        let engines = make_engines(vec![
            Arc::new(MockAggEngine::healthy(
                "google",
                SearchEngineType::Google,
                vec![make_item("A", "https://a.com", SearchEngineType::Google)],
            )),
            Arc::new(MockAggEngine::healthy(
                "bing",
                SearchEngineType::Bing,
                vec![make_item("B", "https://b.com", SearchEngineType::Bing)],
            )),
        ]);
        let agg = SearchAggregator::new(engines, 5000);

        let items = <SearchAggregator as SearchEngine>::search_with_engine(
            &agg, "test", 10, None, None, None,
        )
        .await
        .expect("should search all engines");

        assert_eq!(
            items.len(),
            2,
            "with engine=None, should search all engines"
        );
    }

    // ========== search_with_engine (inherent method) tests ==========

    #[tokio::test]
    async fn test_inherent_search_with_engine_specific() {
        let engines = make_engines(vec![Arc::new(MockAggEngine::healthy(
            "google",
            SearchEngineType::Google,
            vec![make_item("G", "https://g.com", SearchEngineType::Google)],
        ))]);
        let agg = SearchAggregator::new(engines, 5000);

        let request = SearchRequest::new("test").with_limit(10);
        let results = agg
            .search_with_engine(&request, Some("google"))
            .await
            .expect("inherent method should succeed with specific engine");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "G");
    }

    #[tokio::test]
    async fn test_inherent_search_with_engine_none() {
        let engines = make_engines(vec![Arc::new(MockAggEngine::healthy(
            "google",
            SearchEngineType::Google,
            vec![make_item("G", "https://g.com", SearchEngineType::Google)],
        ))]);
        let agg = SearchAggregator::new(engines, 5000);

        let request = SearchRequest::new("test").with_limit(10);
        let results = agg
            .search_with_engine(&request, None)
            .await
            .expect("inherent method should succeed with None engine");

        assert_eq!(results.len(), 1);
    }

    // ========== Circuit breaker tests ==========

    #[tokio::test]
    async fn test_circuit_breaker_skips_after_three_failures() {
        let mock = Arc::new(MockAggEngine::failing("google", SearchEngineType::Google));
        let engines = make_engines(vec![mock.clone()]);
        let agg = SearchAggregator::new(engines, 5000);

        // Use different queries for each call to bypass the cache, since
        // the first failed search caches an empty result for that query.
        let queries = ["q1", "q2", "q3", "q4"];

        // First 3 calls: engine is called and fails, incrementing failure count.
        agg.search(&SearchRequest::new(queries[0]).with_limit(10))
            .await
            .unwrap();
        assert_eq!(mock.call_count(), 1, "engine called on 1st search");

        agg.search(&SearchRequest::new(queries[1]).with_limit(10))
            .await
            .unwrap();
        assert_eq!(mock.call_count(), 2, "engine called on 2nd search");

        agg.search(&SearchRequest::new(queries[2]).with_limit(10))
            .await
            .unwrap();
        assert_eq!(mock.call_count(), 3, "engine called on 3rd search");

        // 4th call: circuit breaker trips (failure count >= 3), engine is skipped.
        agg.search(&SearchRequest::new(queries[3]).with_limit(10))
            .await
            .unwrap();
        assert_eq!(
            mock.call_count(),
            3,
            "engine should NOT be called after 3 failures (circuit breaker open)"
        );
    }

    // ========== Supplementary tests: health() branches, cache truncation, recovery ==========

    /// Helper: create a mock engine with a custom health value.
    fn make_engine_with_health(
        name: &'static str,
        engine_type: SearchEngineType,
        health: EngineHealth,
    ) -> Arc<dyn SearchEngine> {
        Arc::new(MockAggEngine {
            name,
            engine_type,
            items: Vec::new(),
            health,
            fail: false,
            slow: false,
            call_count: Mutex::new(0),
        })
    }

    #[test]
    fn test_health_with_degraded_engine_is_degraded() {
        // A single engine with Degraded health → unhealthy_count=1, total=1,
        // 1 >= 1/2=0 → Degraded.
        let engines = make_engines(vec![make_engine_with_health(
            "google",
            SearchEngineType::Google,
            EngineHealth::Degraded,
        )]);
        let agg = SearchAggregator::new(engines, 5000);
        assert_eq!(agg.health(), EngineHealth::Degraded);
    }

    #[test]
    fn test_health_with_isolated_engine_counts_as_unhealthy() {
        // Isolated health is treated as unhealthy in the health() aggregation.
        let engines = make_engines(vec![make_engine_with_health(
            "google",
            SearchEngineType::Google,
            EngineHealth::Isolated,
        )]);
        let agg = SearchAggregator::new(engines, 5000);
        assert_eq!(agg.health(), EngineHealth::Degraded);
    }

    #[test]
    fn test_health_with_unknown_engine_is_healthy() {
        // Unknown health is treated as healthy in the health() aggregation.
        let engines = make_engines(vec![make_engine_with_health(
            "google",
            SearchEngineType::Google,
            EngineHealth::Unknown,
        )]);
        let agg = SearchAggregator::new(engines, 5000);
        assert_eq!(agg.health(), EngineHealth::Healthy);
    }

    #[test]
    fn test_health_mixed_degraded_and_healthy_majority_healthy() {
        // 1 Degraded out of 4 → 1 < 4/2=2 → Healthy.
        let engines = make_engines(vec![
            make_engine_with_health("google", SearchEngineType::Google, EngineHealth::Healthy),
            make_engine_with_health("bing", SearchEngineType::Bing, EngineHealth::Healthy),
            make_engine_with_health("baidu", SearchEngineType::Baidu, EngineHealth::Healthy),
            make_engine_with_health("sogou", SearchEngineType::Sogou, EngineHealth::Degraded),
        ]);
        let agg = SearchAggregator::new(engines, 5000);
        assert_eq!(agg.health(), EngineHealth::Healthy);
    }

    #[test]
    fn test_health_all_unknown_is_healthy() {
        let engines = make_engines(vec![
            make_engine_with_health("google", SearchEngineType::Google, EngineHealth::Unknown),
            make_engine_with_health("bing", SearchEngineType::Bing, EngineHealth::Unknown),
        ]);
        let agg = SearchAggregator::new(engines, 5000);
        assert_eq!(agg.health(), EngineHealth::Healthy);
    }

    #[tokio::test]
    async fn test_cache_hit_truncates_to_limit() {
        // The cache stores ALL results from deduplicate_and_rank (without
        // applying the limit). On a cache hit, the cached results are truncated
        // to the request limit. To trigger this, the engine must return more
        // items than the limit, and the same cache key (same query+limit+...)
        // must be used for both searches.
        // Use distinct titles to avoid Jaro-Winkler deduplication.
        let items: Vec<ResponseItem> = vec![
            make_item("Alpha", "https://alpha.com", SearchEngineType::Google),
            make_item("Beta", "https://beta.com", SearchEngineType::Google),
            make_item("Gamma", "https://gamma.com", SearchEngineType::Google),
            make_item("Delta", "https://delta.com", SearchEngineType::Google),
            make_item("Epsilon", "https://epsilon.com", SearchEngineType::Google),
        ];
        let mock = Arc::new(MockAggEngine::healthy(
            "google",
            SearchEngineType::Google,
            items,
        ));
        let engines = make_engines(vec![mock.clone()]);
        let agg = SearchAggregator::new(engines, 5000);

        // First search with limit=3: engine returns 5 items, cache stores 5.
        let req1 = SearchRequest::new("trunc").with_limit(3);
        let resp1 = agg
            .search(&req1)
            .await
            .expect("first search should succeed");
        // First search returns ALL 5 results (limit not applied on first search).
        assert_eq!(resp1.items.len(), 5);
        assert_eq!(mock.call_count(), 1);

        // Second search with SAME limit=3: cache hit, truncated to 3.
        let req2 = SearchRequest::new("trunc").with_limit(3);
        let resp2 = agg
            .search(&req2)
            .await
            .expect("second search should hit cache");
        assert_eq!(
            resp2.items.len(),
            3,
            "cached results should be truncated to the request limit"
        );
        assert_eq!(
            mock.call_count(),
            1,
            "engine should NOT be called again on cache hit"
        );
    }

    #[tokio::test]
    async fn test_inherent_search_with_engine_not_found_falls_back() {
        // Inherent search_with_engine with a non-existent engine name should
        // fall back to aggregator search (search all engines).
        let engines = make_engines(vec![Arc::new(MockAggEngine::healthy(
            "google",
            SearchEngineType::Google,
            vec![make_item(
                "G Result",
                "https://g.com",
                SearchEngineType::Google,
            )],
        ))]);
        let agg = SearchAggregator::new(engines, 5000);

        let request = SearchRequest::new("test").with_limit(10);
        let results = agg
            .search_with_engine(&request, Some("nonexistent"))
            .await
            .expect("fallback to aggregator should succeed");
        assert_eq!(
            results.len(),
            1,
            "fallback should search all engines and return their results"
        );
        assert_eq!(results[0].title, "G Result");
    }

    #[tokio::test]
    async fn test_circuit_breaker_resets_on_success() {
        // Engine fails once (failure count=1), then succeeds on the next call.
        // The success should reset the failure count to 0.
        // We use a mock that fails on the first call and succeeds afterward.
        use std::sync::atomic::{AtomicBool, Ordering};

        struct FlakyEngine {
            name: &'static str,
            engine_type: SearchEngineType,
            items: Vec<ResponseItem>,
            should_fail: AtomicBool,
            call_count: Mutex<u32>,
        }

        #[async_trait]
        impl SearchEngine for FlakyEngine {
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
            ) -> Result<Response<ResponseItem>, SearchError> {
                {
                    let mut c = self.call_count.lock().unwrap();
                    *c += 1;
                }
                if self.should_fail.load(Ordering::SeqCst) {
                    return Err(SearchError::NoEngineAvailable);
                }
                Ok(Response {
                    items: self.items.clone(),
                    total_results: Some(self.items.len() as u64),
                    engine: self.engine_type,
                })
            }
        }

        let flaky = Arc::new(FlakyEngine {
            name: "flaky",
            engine_type: SearchEngineType::Google,
            items: vec![make_item(
                "Flaky Result",
                "https://flaky.com",
                SearchEngineType::Google,
            )],
            should_fail: AtomicBool::new(true),
            call_count: Mutex::new(0),
        });
        let engines: Vec<Arc<dyn SearchEngine>> = vec![flaky.clone()];
        let agg = SearchAggregator::new(engines, 5000);

        // First call: fails, failure count becomes 1.
        let req1 = SearchRequest::new("q1").with_limit(10);
        let r1 = agg.search(&req1).await.unwrap();
        assert!(r1.items.is_empty(), "first call should fail → empty");
        assert_eq!(*flaky.call_count.lock().unwrap(), 1);

        // Make the engine succeed for subsequent calls.
        flaky.should_fail.store(false, Ordering::SeqCst);

        // Second call with a different query: should succeed and reset failure count.
        let req2 = SearchRequest::new("q2").with_limit(10);
        let r2 = agg.search(&req2).await.unwrap();
        assert_eq!(r2.items.len(), 1, "second call should succeed");
        assert_eq!(*flaky.call_count.lock().unwrap(), 2);

        // Third call with yet another query: should still call the engine
        // (failure count was reset, circuit breaker not tripped).
        let req3 = SearchRequest::new("q3").with_limit(10);
        let r3 = agg.search(&req3).await.unwrap();
        assert_eq!(r3.items.len(), 1, "third call should succeed");
        assert_eq!(
            *flaky.call_count.lock().unwrap(),
            3,
            "engine should be called again (failure count was reset on success)"
        );
    }

    #[tokio::test]
    async fn test_search_with_slow_engine_times_out() {
        // A slow engine that sleeps longer than the aggregator timeout should
        // trigger the timeout branch, incrementing the failure count.
        let engines = make_engines(vec![Arc::new(MockAggEngine::slow(
            "slow-google",
            SearchEngineType::Google,
        ))]);
        // Use a very short timeout (1ms) so the slow engine (5s sleep) times out.
        let agg = SearchAggregator::new(engines, 1);

        let request = SearchRequest::new("slow-test").with_limit(10);
        let response = agg
            .search(&request)
            .await
            .expect("search should succeed (timeout returns None, not error)");
        assert!(
            response.items.is_empty(),
            "timed-out engine should yield no results"
        );
    }

    #[tokio::test]
    async fn test_search_with_lang_and_country_in_cache_key() {
        // The cache key includes lang and country. Same query with different
        // lang/country should bypass the cache and call the engine again.
        let mock = Arc::new(MockAggEngine::healthy(
            "google",
            SearchEngineType::Google,
            vec![make_item("R", "https://r.com", SearchEngineType::Google)],
        ));
        let engines = make_engines(vec![mock.clone()]);
        let agg = SearchAggregator::new(engines, 5000);

        let req1 = SearchRequest::new("query").with_limit(10);
        req1.clone(); // just to use req1
        let mut req1 = SearchRequest::new("query").with_limit(10);
        req1.lang = Some("en".to_string());
        agg.search(&req1).await.unwrap();
        assert_eq!(mock.call_count(), 1);

        let mut req2 = SearchRequest::new("query").with_limit(10);
        req2.lang = Some("zh".to_string());
        agg.search(&req2).await.unwrap();
        assert_eq!(mock.call_count(), 2, "different lang should bypass cache");
    }

    // ========== deduplicate_and_rank with published date extraction ==========
    // These tests cover lines 180, 183, 189: extract_published_date_with_parser
    // returns Some when the combined title+description text contains a date,
    // and calculate_freshness_score is applied using the extracted date.

    #[test]
    fn test_deduplicate_and_rank_extracts_published_date_from_description() {
        let agg = SearchAggregator::new(vec![], 5000);
        let results = vec![SearchResult {
            title: "Rust 2024 release notes".to_string(),
            url: "https://example.com/rust-2024".to_string(),
            description: Some("Published on 2024-01-15 by the Rust team".to_string()),
            engine: "google".to_string(),
            score: 0.0,
            published_time: None,
        }];
        let ranked = agg.deduplicate_and_rank(results, "rust");
        assert_eq!(ranked.len(), 1);
        assert!(
            ranked[0].published_time.is_some(),
            "published_time should be extracted from description containing date"
        );
    }

    #[test]
    fn test_deduplicate_and_rank_extracts_published_date_from_title() {
        let agg = SearchAggregator::new(vec![], 5000);
        let results = vec![SearchResult {
            title: "Rust 1.75 released on 2024-01-15".to_string(),
            url: "https://example.com/rust-1-75".to_string(),
            description: None,
            engine: "bing".to_string(),
            score: 0.0,
            published_time: None,
        }];
        let ranked = agg.deduplicate_and_rank(results, "rust");
        assert_eq!(ranked.len(), 1);
        assert!(
            ranked[0].published_time.is_some(),
            "published_time should be extracted from title containing date"
        );
    }
}
