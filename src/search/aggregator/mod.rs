// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

pub mod deduplicator;
pub mod enhanced;

use async_trait::async_trait;
use dashmap::DashMap;
use futures::future::join_all;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use strsim::jaro_winkler;
use tracing::{info, warn};

use crate::domain::models::search_result::SearchResult;
use crate::domain::services::relevance_scorer::RelevanceScorer;
use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::types::{EngineHealth, SearchEngineType};

pub struct SearchAggregator {
    engines: Vec<Arc<dyn SearchEngine>>,
    timeout: Duration,
    cache: DashMap<String, (Vec<SearchResult>, Instant)>,
    cache_ttl: Duration,
    failures: std::sync::Arc<DashMap<String, u32>>,
}

impl fmt::Debug for SearchAggregator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchAggregator")
            .field("engine_count", &self.engines.len())
            .field("timeout_ms", &self.timeout.as_millis())
            .field("cache_size", &self.cache.len())
            .finish()
    }
}

impl SearchAggregator {
    pub fn new(engines: Vec<Arc<dyn SearchEngine>>, timeout_ms: u64) -> Self {
        Self {
            engines,
            timeout: Duration::from_millis(timeout_ms),
            cache: DashMap::new(),
            cache_ttl: Duration::from_secs(300),
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
            let target_engine = self.get_engine(name);
            match target_engine {
                Some(engine) => {
                    info!("Directly calling search engine: {}", name);
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
        let scorer = RelevanceScorer::new(query);

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
                    if let Some(published_date) =
                        RelevanceScorer::extract_published_date(&combined_text)
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
        if let Some(entry) = self.cache.get(&cache_key) {
            if entry.1.elapsed() < self.cache_ttl {
                info!("Cache hit for query: {}", request.query);
                let mut cached_results = entry.0.clone();
                // Apply limit to cached results
                if cached_results.len() > request.limit as usize {
                    cached_results.truncate(request.limit as usize);
                }
                return Ok(self.convert_results_to_response(cached_results));
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
        self.cache
            .insert(cache_key, (final_results.clone(), Instant::now()));

        Ok(self.convert_results_to_response(final_results))
    }
}
