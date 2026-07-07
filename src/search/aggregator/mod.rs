// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! SearchAggregator - Search result aggregation and caching
//!
//! This module handles aggregating results from multiple search engines
//! with caching and deduplication support.

pub mod deduplicator;
pub mod enhanced;

use async_trait::async_trait;
use dashmap::DashMap;
use futures::future::join_all;
use lru::LruCache;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use strsim::jaro_winkler;
use log::{info, warn};

use crate::common::constants::cache_config;
use crate::domain::models::search_result::SearchResult;
use crate::domain::services::relevance_scorer::{DateParserComponent, RelevanceScorer};
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
        let mut unique_results: Vec<SearchResult> = Vec::new();
        let scorer = RelevanceScorer::for_query(query);

        for mut result in results {
            let is_duplicate = unique_results.iter().any(|existing| {
                // Check URL equality first
                if existing.url == result.url {
                    return true;
                }

                // Check title similarity using Jaro-Winkler
                let similarity = jaro_winkler(&existing.title, &result.title);
                similarity > 0.9 // Threshold
            });

            if !is_duplicate {
                // Calculate PRD-compliant relevance score
                let relevance_score = scorer.calculate_score(
                    &result.title,
                    result.description.as_deref(),
                    &result.url,
                );

                // Extract publication date if not already set
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

                // Apply freshness score if we have publication date
                let freshness_score = if let Some(published_time) = result.published_time {
                    RelevanceScorer::calculate_freshness_score(published_time)
                } else {
                    0.5 // Default freshness score for unknown dates
                };

                // Combine relevance and freshness scores (70% relevance, 30% freshness)
                result.score = relevance_score * 0.7 + freshness_score * 0.3;

                unique_results.push(result);
            }
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
