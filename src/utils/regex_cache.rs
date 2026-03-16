// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified regex cache using oxcache.

use crate::infrastructure::oxcache::SearchCache;
use futures::executor::block_on;
use oxcache::Cache;
use regex::Regex;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

/// Regex cache trait
pub trait RegexCacheTrait: Send + Sync {
    fn get_or_insert(&self, pattern: &str) -> Result<Regex, String>;
    fn get_or_insert_escaped(&self, literal: &str) -> Result<Regex, String>;
    fn get_or_compile(&self, pattern: &str) -> Result<Arc<Regex>, String>;
}

/// Regex cache using oxcache
#[derive(Clone)]
pub struct RegexCache {
    cache: Arc<SearchCache>,
}

/// Type alias for RegexCache component (for DI module compatibility)
pub type RegexCacheComponent = RegexCache;

impl RegexCache {
    pub fn new(cache: Arc<SearchCache>) -> Self {
        Self { cache }
    }

    #[inline]
    pub fn get_or_insert(&self, pattern: &str) -> Result<Regex, String> {
        let key = format!("regex:{}", pattern);
        
        match block_on(self.cache.get(&key)) {
            Ok(Some(cached)) => Ok(cached.clone()),
            _ => {
                let regex = Regex::new(pattern).map_err(|e| e.to_string())?;
                if let Err(e) = block_on(self.cache.set(&key, &regex)) {
                    warn!("Failed to cache regex: {}", e);
                }
                Ok(regex)
            }
        }
    }

    #[inline]
    pub fn get_or_insert_escaped(&self, literal: &str) -> Result<Regex, String> {
        let pattern = format!(r"\b{}\b", regex::escape(literal));
        self.get_or_insert(&pattern)
    }

    #[inline]
    pub fn get_or_compile(&self, pattern: &str) -> Result<Arc<Regex>, String> {
        let regex = self.get_or_insert(pattern)?;
        Ok(Arc::new(regex))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_regex_cache_creation() {
        let cache = Arc::new(oxcache::Cache::builder()
            .capacity(100)
            .ttl(Duration::from_secs(3600))
            .build()
            .await
            .unwrap());

        let regex_cache = RegexCache::new(cache);
        let regex = regex_cache.get_or_insert(r"\d+").unwrap();
        assert!(regex.is_match("123"));
    }
}
