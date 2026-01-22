// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified regex cache for efficient regex compilation and reuse.
//!
//! This module provides a thread-safe regex cache that eliminates duplicate
//! regex compilation patterns across the codebase.
//!
//! # DI Support
//!
//! This module provides a `RegexCacheTrait` interface for dependency injection.
//! Use `RegexCacheComponent` to register it in your DI container.

use dashmap::DashMap;
use regex::Regex;
use shaku::{Component, Interface};
use std::sync::Arc;

/// 正则缓存 trait（支持 DI）
///
/// 提供正则表达式缓存的抽象接口，便于测试时注入 mock 实现。
pub trait RegexCacheTrait: Interface + Send + Sync {
    /// 获取或插入正则表达式
    fn get_or_insert(&self, pattern: &str) -> Result<Regex, String>;
    /// 获取或插入带转义的正则表达式
    fn get_or_insert_escaped(&self, literal: &str) -> Result<Regex, String>;
    /// 清空缓存
    fn clear(&self);
    /// 获取缓存大小
    fn len(&self) -> usize;
    /// 检查缓存是否为空
    fn is_empty(&self) -> bool;
}

/// Thread-safe regex cache with lazy initialization using DashMap.
///
/// Uses `DashMap` for lock-free concurrent access, providing better
/// performance under high concurrency compared to mutex-based solutions.
///
/// # Example
///
/// ```rust
/// use crate::utils::regex_cache::RegexCache;
///
/// let cache = RegexCache::new();
/// let regex = cache.get_or_insert(r"\d+").unwrap();
/// ```
#[derive(Clone)]
pub struct RegexCache {
    cache: Arc<DashMap<String, Regex>>,
}

impl RegexCache {
    /// Creates a new regex cache with default capacity.
    #[inline]
    pub fn new() -> Self {
        Self {
            cache: Arc::new(DashMap::with_capacity_and_shard_amount(256, 32)),
        }
    }

    /// Gets a cached regex or compiles and caches a new one.
    ///
    /// # Errors
    ///
    /// Returns an error if regex compilation fails.
    #[inline]
    pub fn get_or_insert(&self, pattern: &str) -> Result<Regex, String> {
        if let Some(regex) = self.cache.get(pattern) {
            return Ok(regex.clone());
        }

        let regex = Regex::new(pattern).map_err(|e| e.to_string())?;
        self.cache.insert(pattern.to_string(), regex.clone());
        Ok(regex)
    }

    /// Gets a cached regex pattern with automatic escaping.
    ///
    /// This is useful for literal string matching where special regex
    /// characters should be treated literally.
    ///
    /// # Errors
    ///
    /// Returns an error if regex compilation fails.
    #[inline]
    pub fn get_or_insert_escaped(&self, literal: &str) -> Result<Regex, String> {
        let pattern = format!(r"\b{}\b", regex::escape(literal));
        self.get_or_insert(&pattern)
    }

    /// Clears all cached regex patterns.
    #[inline]
    pub fn clear(&self) {
        self.cache.clear();
    }

    /// Returns the current number of cached regex patterns.
    #[inline]
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns true if the cache is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

/// 正则缓存组件（DI 实现）
///
/// 实现了 `RegexCacheTrait` 接口，可通过 DI 容器注入。
#[derive(Component)]
#[shaku(interface = RegexCacheTrait)]
pub struct RegexCacheComponent {
    cache: Arc<DashMap<String, Regex>>,
}

impl RegexCacheComponent {
    /// 创建新的正则缓存组件
    pub fn new() -> Self {
        Self {
            cache: Arc::new(DashMap::with_capacity_and_shard_amount(256, 32)),
        }
    }
}

impl Default for RegexCacheComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl RegexCacheTrait for RegexCacheComponent {
    fn get_or_insert(&self, pattern: &str) -> Result<Regex, String> {
        if let Some(regex) = self.cache.get(pattern) {
            return Ok(regex.clone());
        }

        let regex = Regex::new(pattern).map_err(|e| e.to_string())?;
        self.cache.insert(pattern.to_string(), regex.clone());
        Ok(regex)
    }

    fn get_or_insert_escaped(&self, literal: &str) -> Result<Regex, String> {
        let pattern = format!(r"\b{}\b", regex::escape(literal));
        self.get_or_insert(&pattern)
    }

    fn clear(&self) {
        self.cache.clear();
    }

    fn len(&self) -> usize {
        self.cache.len()
    }

    fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

impl Default for RegexCache {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regex_cache_new() {
        let cache = RegexCache::new();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_regex_cache_get_or_insert() {
        let cache = RegexCache::new();

        // First insert
        let regex1 = cache.get_or_insert(r"\d+").unwrap();
        assert_eq!(regex1.as_str(), r"\d+");
        assert_eq!(cache.len(), 1);

        // Get cached
        let regex2 = cache.get_or_insert(r"\d+").unwrap();
        assert_eq!(regex1.as_str(), regex2.as_str());
        assert_eq!(cache.len(), 1);

        // Different pattern
        let regex3 = cache.get_or_insert(r"\w+").unwrap();
        assert_eq!(regex3.as_str(), r"\w+");
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_regex_cache_escaped() {
        let cache = RegexCache::new();

        // Test escaping special characters
        let regex = cache.get_or_insert_escaped("hello.world").unwrap();
        assert!(regex.is_match("hello.world"));
        // Word boundary means it won't match if there's no word boundary
        assert!(!regex.is_match("helloXworld")); // No word boundary between hello and world
        assert!(!regex.is_match("hello"));
    }

    #[test]
    fn test_regex_cache_clear() {
        let cache = RegexCache::new();

        cache.get_or_insert(r"\d+").unwrap();
        assert!(!cache.is_empty());

        cache.clear();
        assert!(cache.is_empty());
    }
}
