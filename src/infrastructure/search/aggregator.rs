// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use dashmap::DashMap;
use futures::future::join_all;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use strsim::jaro_winkler;
use tracing::{info, warn};

use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use crate::domain::services::relevance_scorer::RelevanceScorer;

pub struct SearchAggregator {
    engines: Vec<Arc<dyn SearchEngine>>,
    timeout: Duration,
    cache: DashMap<String, (Vec<SearchResult>, Instant)>,
    cache_ttl: Duration,
    failures: std::sync::Arc<DashMap<String, u32>>,
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
    async fn search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // Check cache
        let cache_key = format!(
            "{}:{}:{}:{}",
            query,
            limit,
            lang.unwrap_or(""),
            country.unwrap_or("")
        );
        if let Some(entry) = self.cache.get(&cache_key) {
            if entry.1.elapsed() < self.cache_ttl {
                info!("Cache hit for query: {}", query);
                return Ok(entry.0.clone());
            }
        }

        let futures = self.engines.iter().map(|engine| {
            let engine = engine.clone();
            let query = query.to_string();
            let lang = lang.map(|s| s.to_string());
            let country = country.map(|s| s.to_string());
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

                let result = tokio::time::timeout(
                    self.timeout,
                    engine.search(&query, limit, lang.as_deref(), country.as_deref()),
                )
                .await;

                match result {
                    Ok(Ok(results)) => {
                        info!("Engine {} returned {} results", engine_name, results.len());
                        // Reset failure count on success
                        if failures.contains_key(engine_name) {
                            failures.remove(engine_name);
                        }
                        Some(results)
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

        let results: Vec<Vec<SearchResult>> =
            join_all(futures).await.into_iter().flatten().collect();

        let final_results = self.deduplicate_and_rank(results.concat(), query);
        self.cache
            .insert(cache_key, (final_results.clone(), Instant::now()));

        Ok(final_results)
    }

    fn name(&self) -> &'static str {
        "aggregator"
    }
}
