use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use log::warn;

use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use crate::infrastructure::oxcache::generate_search_key;

#[allow(dead_code)]
pub struct EnhancedSearchAggregator {
    engines: Vec<Arc<dyn SearchEngine>>,
    timeout: Duration,
}

impl EnhancedSearchAggregator {
    pub fn new(engines: Vec<Arc<dyn SearchEngine>>, timeout_ms: u64) -> Self {
        Self {
            engines,
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    #[allow(dead_code)]
    fn generate_cache_key(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> String {
        generate_search_key(query, limit, lang, country)
    }

    async fn search_with_cache(
        &self,
        _query: &str,
        _limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // Note: Cache functionality will be implemented when OxCacheComponent is ready
        // For now, fall through to engine search
        self.search_engines(_query, _limit, _lang, _country).await
    }

    async fn search_engines(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let mut all_results = Vec::new();

        for engine in &self.engines {
            match engine.search(query, limit, lang, country).await {
                Ok(results) => {
                    all_results.extend(results);
                }
                Err(e) => {
                    warn!("Engine search failed: {}", e);
                }
            }
        }

        all_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all_results.truncate(limit as usize);

        Ok(all_results)
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
        "aggregator"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_generation() {
        let aggregator = EnhancedSearchAggregator::new(vec![], 5000);
        let key1 = aggregator.generate_cache_key("rust", 10, Some("en"), Some("US"));
        assert!(key1.starts_with("search:rust"));

        let key2 = aggregator.generate_cache_key("python", 5, None, None);
        assert!(key2.starts_with("search:python"));
    }
}
