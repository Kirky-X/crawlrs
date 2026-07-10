// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified regex cache using oxcache.
//!
//! Since regex::Regex doesn't implement Serialize/Deserialize, we cache the pattern string
//! and compile it on demand. This provides fast lookup while avoiding serialization issues.

use crate::infrastructure::oxcache::RegexCacheType;
use log::warn;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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

    async fn make_cache() -> RegexCache {
        let cache: RegexCacheType = oxcache::Cache::builder()
            .capacity(100)
            .ttl(Duration::from_secs(3600))
            .build()
            .await
            .unwrap();
        RegexCache::new(Arc::new(cache))
    }

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

    #[tokio::test]
    async fn test_get_or_insert_caches_on_second_call() {
        let regex_cache = make_cache().await;
        let regex1 = regex_cache.get_or_insert(r"\d+").unwrap();
        let regex2 = regex_cache.get_or_insert(r"\d+").unwrap();
        assert!(regex1.is_match("123"));
        assert!(regex2.is_match("456"));
    }

    #[tokio::test]
    async fn test_get_or_insert_different_patterns() {
        let regex_cache = make_cache().await;
        let digits = regex_cache.get_or_insert(r"\d+").unwrap();
        let letters = regex_cache.get_or_insert(r"[a-z]+").unwrap();
        assert!(digits.is_match("123"));
        assert!(!digits.is_match("abc"));
        assert!(letters.is_match("abc"));
        assert!(!letters.is_match("123"));
    }

    #[tokio::test]
    async fn test_get_or_insert_invalid_regex_returns_error() {
        let regex_cache = make_cache().await;
        let result = regex_cache.get_or_insert(r"[invalid");
        assert!(result.is_err());
        let err_msg = result.unwrap_err();
        assert!(!err_msg.is_empty());
    }

    #[tokio::test]
    async fn test_get_or_insert_escaped_matches_literal() {
        let regex_cache = make_cache().await;
        let regex = regex_cache.get_or_insert_escaped("a.b").unwrap();
        // The escaped pattern should match the literal "a.b", not "a" + any char + "b"
        assert!(regex.is_match("a.b"));
        assert!(!regex.is_match("axb"));
    }

    #[tokio::test]
    async fn test_get_or_insert_escaped_special_chars() {
        let regex_cache = make_cache().await;
        let regex = regex_cache.get_or_insert_escaped("$pecial").unwrap();
        // \b 词边界要求 $ 前面是词字符，所以不能有空格
        assert!(regex.is_match("cost$pecial here"));
    }

    #[tokio::test]
    async fn test_get_or_compile_returns_arc() {
        let regex_cache = make_cache().await;
        let regex = regex_cache.get_or_compile(r"\w+").unwrap();
        assert!(regex.is_match("hello_world"));
    }

    #[tokio::test]
    async fn test_get_or_compile_caches_on_second_call() {
        let regex_cache = make_cache().await;
        let regex1 = regex_cache.get_or_compile(r"\d+").unwrap();
        let regex2 = regex_cache.get_or_compile(r"\d+").unwrap();
        assert!(Arc::ptr_eq(&regex1, &regex2));
    }

    #[tokio::test]
    async fn test_get_or_compile_invalid_regex_returns_error() {
        let regex_cache = make_cache().await;
        let result = regex_cache.get_or_compile(r"(unclosed");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_or_compile_different_patterns_return_different_arcs() {
        let regex_cache = make_cache().await;
        let regex1 = regex_cache.get_or_compile(r"\d+").unwrap();
        let regex2 = regex_cache.get_or_compile(r"[a-z]+").unwrap();
        assert!(!Arc::ptr_eq(&regex1, &regex2));
    }

    #[tokio::test]
    async fn test_regex_cache_clone_preserves_state() {
        let regex_cache = make_cache().await;
        let _ = regex_cache.get_or_insert(r"\d+").unwrap();
        let cloned = regex_cache.clone();
        // The cloned cache should also return a valid regex for the same pattern
        let regex = cloned.get_or_insert(r"\d+").unwrap();
        assert!(regex.is_match("123"));
    }

    #[tokio::test]
    async fn test_regex_cache_component_type_alias() {
        let regex_cache = make_cache().await;
        // RegexCacheComponent is a type alias for RegexCache
        let _: &RegexCacheComponent = &regex_cache;
    }
}
