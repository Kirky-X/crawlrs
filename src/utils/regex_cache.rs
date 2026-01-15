// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified regex cache for efficient regex compilation and reuse.
//!
//! This module provides a thread-safe regex cache that eliminates duplicate
//! regex compilation patterns across the codebase.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Mutex;

/// Thread-safe regex cache with lazy initialization.
///
/// # Example
///
/// ```rust
/// use crate::utils::regex_cache::RegexCache;
///
/// let cache = RegexCache::global();
/// let regex = cache.get_or_insert(r"\d+").unwrap();
/// ```
pub struct RegexCache {
    cache: Mutex<HashMap<String, Regex>>,
}

impl RegexCache {
    /// Creates a new regex cache with default capacity.
    #[inline]
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::with_capacity(256)),
        }
    }

    /// Gets a cached regex or compiles and caches a new one.
    ///
    /// # Errors
    ///
    /// Returns an error if regex compilation fails or if the lock is poisoned.
    #[inline]
    pub fn get_or_insert(&self, pattern: &str) -> Result<Regex, String> {
        let mut cache = self.cache.lock().map_err(|e| e.to_string())?;

        if let Some(regex) = cache.get(pattern) {
            return Ok(regex.clone());
        }

        let regex = Regex::new(pattern).map_err(|e| e.to_string())?;
        cache.insert(pattern.to_string(), regex.clone());
        Ok(regex)
    }

    /// Gets a cached regex pattern with automatic escaping.
    ///
    /// This is useful for literal string matching where special regex
    /// characters should be treated literally.
    ///
    /// # Errors
    ///
    /// Returns an error if regex compilation fails or if the lock is poisoned.
    #[inline]
    pub fn get_or_insert_escaped(&self, literal: &str) -> Result<Regex, String> {
        let pattern = format!(r"\b{}\b", regex::escape(literal));
        self.get_or_insert(&pattern)
    }

    /// Clears all cached regex patterns.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock is poisoned.
    #[inline]
    pub fn clear(&self) -> Result<(), String> {
        let mut cache = self.cache.lock().map_err(|e| e.to_string())?;
        cache.clear();
        Ok(())
    }

    /// Returns the current number of cached regex patterns.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock is poisoned.
    #[inline]
    pub fn len(&self) -> Result<usize, String> {
        let cache = self.cache.lock().map_err(|e| e.to_string())?;
        Ok(cache.len())
    }

    /// Returns true if the cache is empty.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock is poisoned.
    #[inline]
    pub fn is_empty(&self) -> Result<bool, String> {
        let cache = self.cache.lock().map_err(|e| e.to_string())?;
        Ok(cache.is_empty())
    }
}

impl Default for RegexCache {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Global regex cache instance for convenience.
///
/// This static lazy instance can be used across the codebase
/// without requiring explicit initialization.
pub static GLOBAL_REGEX_CACHE: Lazy<RegexCache> = Lazy::new(RegexCache::new);

impl RegexCache {
    /// Returns a reference to the global regex cache.
    #[inline]
    pub fn global() -> &'static Self {
        &GLOBAL_REGEX_CACHE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regex_cache_new() {
        let cache = RegexCache::new();
        assert!(cache.is_empty().unwrap());
    }

    #[test]
    fn test_regex_cache_get_or_insert() {
        let cache = RegexCache::new();

        // First insert
        let regex1 = cache.get_or_insert(r"\d+").unwrap();
        assert_eq!(regex1.as_str(), r"\d+");
        assert_eq!(cache.len().unwrap(), 1);

        // Get cached
        let regex2 = cache.get_or_insert(r"\d+").unwrap();
        assert_eq!(regex1.as_str(), regex2.as_str());
        assert_eq!(cache.len().unwrap(), 1);

        // Different pattern
        let regex3 = cache.get_or_insert(r"\w+").unwrap();
        assert_eq!(regex3.as_str(), r"\w+");
        assert_eq!(cache.len().unwrap(), 2);
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
        assert!(!cache.is_empty().unwrap());

        cache.clear().unwrap();
        assert!(cache.is_empty().unwrap());
    }
}
