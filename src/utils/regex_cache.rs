// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified regex cache using oxcache.
//!
//! Since regex::Regex doesn't implement Serialize/Deserialize, we cache the pattern string
//! and compile it on demand. This provides fast lookup while avoiding serialization issues.

use crate::infrastructure::oxcache::RegexCacheType;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

/// Regex cache trait
pub trait RegexCacheTrait: Send + Sync {
    fn get_or_insert(&self, pattern: &str) -> Result<Regex, String>;
    fn get_or_insert_escaped(&self, literal: &str) -> Result<Regex, String>;
    fn get_or_compile(&self, pattern: &str) -> Result<Arc<Regex>, String>;
}

/// Regex cache using oxcache for persistence and in-memory map for compiled regexes
#[derive(Clone)]
pub struct RegexCache {
    cache: Arc<RegexCacheType>,
    compiled: Arc<RwLock<HashMap<String, Arc<Regex>>>>,
}

/// Type alias for RegexCache component (for DI module compatibility)
pub type RegexCacheComponent = RegexCache;

impl RegexCache {
    pub fn new(cache: Arc<RegexCacheType>) -> Self {
        Self {
            cache,
            compiled: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[inline]
    pub fn get_or_insert(&self, pattern: &str) -> Result<Regex, String> {
        let key = format!("regex:{}", pattern);
        
        let compiled_read = futures::executor::block_on(self.compiled.read());
        if let Some(regex) = compiled_read.get(&key) {
            return Ok((**regex).clone());
        }
        drop(compiled_read);
        
        let regex = Regex::new(pattern).map_err(|e| e.to_string())?;
        
        let mut compiled_write = futures::executor::block_on(self.compiled.write());
        compiled_write.insert(key.clone(), Arc::new(regex.clone()));
        
        if let Err(e) = futures::executor::block_on(self.cache.set(&key, &pattern.to_string())) {
            warn!("Failed to cache regex pattern: {}", e);
        }
        
        Ok(regex)
    }

    #[inline]
    pub fn get_or_insert_escaped(&self, literal: &str) -> Result<Regex, String> {
        let pattern = format!(r"\b{}\b", regex::escape(literal));
        self.get_or_insert(&pattern)
    }

    #[inline]
    pub fn get_or_compile(&self, pattern: &str) -> Result<Arc<Regex>, String> {
        let key = format!("regex:{}", pattern);
        
        let compiled_read = futures::executor::block_on(self.compiled.read());
        if let Some(regex) = compiled_read.get(&key) {
            return Ok(regex.clone());
        }
        drop(compiled_read);
        
        let regex = Regex::new(pattern).map_err(|e| e.to_string())?;
        let regex_arc = Arc::new(regex);
        
        let mut compiled_write = futures::executor::block_on(self.compiled.write());
        compiled_write.insert(key.clone(), regex_arc.clone());
        
        if let Err(e) = futures::executor::block_on(self.cache.set(&key, &pattern.to_string())) {
            warn!("Failed to cache regex pattern: {}", e);
        }
        
        Ok(regex_arc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_regex_cache_creation() {
        let cache: RegexCacheType = oxcache::Cache::builder()
            .capacity(100)
            .ttl(Duration::from_secs(3600))
            .build()
            .await
            .unwrap();

        let regex_cache = RegexCache::new(Arc::new(cache));
        let regex = regex_cache.get_or_insert(r"\d+").unwrap();
        assert!(regex.is_match("123"));
    }
}
