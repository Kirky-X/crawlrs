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
use std::sync::RwLock;

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

        // Use std::sync::RwLock for sync access (no async block_on needed).
        // Poisoning is ignored because the cache state remains usable even after a panic.
        let compiled_read = self.compiled.read().unwrap_or_else(|e| e.into_inner());
        if let Some(regex) = compiled_read.get(&key) {
            return Ok((**regex).clone());
        }
        drop(compiled_read);

        let regex = Regex::new(pattern).map_err(|e| e.to_string())?;

        let mut compiled_write = self.compiled.write().unwrap_or_else(|e| e.into_inner());
        compiled_write.insert(key.clone(), Arc::new(regex.clone()));

        // Best-effort persistence to oxcache. Fire-and-forget via tokio::spawn
        // because we're in a sync method and cannot await the async cache.set.
        // The in-memory `compiled` HashMap is the primary cache; oxcache persistence
        // is for cross-process sharing / restart survival and is non-essential.
        self.try_persist_to_cache(&key, pattern);

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

        // Use std::sync::RwLock for sync access (no async block_on needed).
        // Poisoning is ignored because the cache state remains usable even after a panic.
        let compiled_read = self.compiled.read().unwrap_or_else(|e| e.into_inner());
        if let Some(regex) = compiled_read.get(&key) {
            return Ok(regex.clone());
        }
        drop(compiled_read);

        let regex = Regex::new(pattern).map_err(|e| e.to_string())?;
        let regex_arc = Arc::new(regex);

        let mut compiled_write = self.compiled.write().unwrap_or_else(|e| e.into_inner());
        compiled_write.insert(key.clone(), regex_arc.clone());

        // Best-effort persistence (see get_or_insert for rationale).
        self.try_persist_to_cache(&key, pattern);

        Ok(regex_arc)
    }

    /// Best-effort persistence of a regex pattern to oxcache.
    ///
    /// Spawns a fire-and-forget task on the current tokio runtime (if any).
    /// This avoids the deadlock-prone `futures::executor::block_on` pattern
    /// that previously could panic when called from `spawn_blocking` or
    /// non-tokio threads (no tokio runtime available).
    ///
    /// The in-memory `compiled` HashMap is the authoritative cache; oxcache
    /// persistence is for cross-process sharing / restart survival only.
    fn try_persist_to_cache(&self, key: &str, value: &str) {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let cache = Arc::clone(&self.cache);
            let key = key.to_string();
            let value = value.to_string();
            // Spawn a detached task — fire-and-forget.
            // Works in both async and spawn_blocking contexts because Handle::spawn
            // only requires a Handle to the runtime, not active async execution.
            drop(handle.spawn(async move {
                if let Err(e) = cache.set(&key, &value).await {
                    warn!("Failed to cache regex pattern: {}", e);
                }
            }));
        }
        // If not in tokio runtime (rare for this codebase), skip persistence silently.
        // The in-memory `compiled` cache still provides correct results.
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

    // ========== cache.set 失败时 warn! 分支覆盖测试 ==========

    use async_trait::async_trait;
    use oxcache::backend::{BackendKind, CacheConnector, CacheReader, CacheWriter};
    use oxcache::OxCacheResult;
    use std::collections::HashMap;

    /// FailingBackend: set 方法总是返回错误，用于覆盖 regex_cache.rs 行 59/88 的 warn!
    struct FailingBackend;

    #[async_trait]
    impl CacheReader for FailingBackend {
        async fn get(&self, _key: &str) -> OxCacheResult<Option<Vec<u8>>> {
            Ok(None)
        }
        async fn exists(&self, _key: &str) -> OxCacheResult<bool> {
            Ok(false)
        }
        async fn ttl(&self, _key: &str) -> OxCacheResult<Option<Duration>> {
            Ok(None)
        }
        async fn len(&self) -> OxCacheResult<u64> {
            Ok(0)
        }
        async fn capacity(&self) -> OxCacheResult<u64> {
            Ok(0)
        }
        async fn stats(&self) -> OxCacheResult<HashMap<String, String>> {
            Ok(HashMap::new())
        }
    }

    #[async_trait]
    impl CacheWriter for FailingBackend {
        async fn set(
            &self,
            _key: &str,
            _value: Vec<u8>,
            _ttl: Option<Duration>,
        ) -> OxCacheResult<()> {
            Err(oxcache::OxCacheError::Operation(
                "FailingBackend always fails".to_string(),
            ))
        }
        async fn delete(&self, _key: &str) -> OxCacheResult<()> {
            Ok(())
        }
        async fn clear(&self) -> OxCacheResult<()> {
            Ok(())
        }
        async fn expire(&self, _key: &str, _ttl: Duration) -> OxCacheResult<bool> {
            Ok(false)
        }
    }

    #[async_trait]
    impl CacheConnector for FailingBackend {
        async fn health_check(&self) -> OxCacheResult<()> {
            Ok(())
        }
        async fn shutdown(&self) {}
        fn backend_kind(&self) -> BackendKind {
            BackendKind::Unknown
        }
    }

    async fn make_failing_cache() -> RegexCache {
        let cache: RegexCacheType = oxcache::Cache::builder()
            .backend_arc(std::sync::Arc::new(FailingBackend))
            .build()
            .await
            .expect("build with FailingBackend should succeed");
        RegexCache::new(std::sync::Arc::new(cache))
    }

    #[tokio::test]
    async fn test_get_or_insert_warns_when_cache_set_fails() {
        // 当 cache.set 失败时，get_or_insert 应执行 warn!（行 59），
        // 但仍返回有效的 regex。
        let regex_cache = make_failing_cache().await;
        let regex = regex_cache
            .get_or_insert(r"\d+")
            .expect("get_or_insert should succeed even when cache.set fails");
        assert!(regex.is_match("123"));
    }

    #[tokio::test]
    async fn test_get_or_compile_warns_when_cache_set_fails() {
        // 当 cache.set 失败时，get_or_compile 应执行 warn!（行 88），
        // 但仍返回有效的 Arc<Regex>。
        let regex_cache = make_failing_cache().await;
        let regex = regex_cache
            .get_or_compile(r"[a-z]+")
            .expect("get_or_compile should succeed even when cache.set fails");
        assert!(regex.is_match("hello"));
    }

    // ========== 并发安全回归测试 ==========
    // 之前 bug: 同步方法用 futures::executor::block_on 访问 tokio::sync::RwLock，
    // 在多线程并发下会死锁。修复后改用 std::sync::RwLock，无 async/block_on 调用。

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_get_or_insert_no_deadlock() {
        // 回归测试：多个线程并发调用同步方法 get_or_insert，必须不死锁。
        // 之前用 futures::executor::block_on(tokio::sync::RwLock.read()) 会在
        // 异步多线程环境下死锁（block_on 阻塞当前线程，无法让持锁任务释放）。
        //
        // 此测试只验证「不死锁 + 编译成功」，不验证 regex 匹配语义
        // （不同 pattern 匹配不同字符类，统一字符串会引入 false negative）。
        use std::time::Duration;
        let cache = Arc::new(make_cache().await);

        let mut handles = Vec::new();
        for i in 0..8u32 {
            let c = Arc::clone(&cache);
            handles.push(tokio::task::spawn_blocking(move || {
                let pattern = match i % 4 {
                    0 => r"\d+",
                    1 => r"[a-z]+",
                    2 => r"\w+",
                    _ => r"[A-Z]+",
                };
                // 只关心 compile 是否成功（Result<Regex, String>）
                c.get_or_insert(pattern).map(|_| ())
            }));
        }

        // 如果死锁，5 秒后会超时
        for handle in handles {
            let result = tokio::time::timeout(Duration::from_secs(5), handle)
                .await
                .expect("concurrent get_or_insert must not deadlock (timed out)");
            // 验证 task 未 panic 且 regex 编译成功
            result
                .expect("task should not panic")
                .expect("regex should compile");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_get_or_compile_no_deadlock() {
        // 回归测试：多线程并发调用 get_or_compile，必须不死锁。
        use std::time::Duration;
        let cache = Arc::new(make_cache().await);

        let mut handles = Vec::new();
        for i in 0..8u32 {
            let c = Arc::clone(&cache);
            handles.push(tokio::task::spawn_blocking(move || {
                let pattern = match i % 4 {
                    0 => r"\d+",
                    1 => r"[a-z]+",
                    2 => r"\w+",
                    _ => r"[A-Z]+",
                };
                c.get_or_compile(pattern).map(|_| ())
            }));
        }

        for handle in handles {
            let result = tokio::time::timeout(Duration::from_secs(5), handle)
                .await
                .expect("concurrent get_or_compile must not deadlock (timed out)");
            result
                .expect("task should not panic")
                .expect("regex should compile");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_mixed_operations_no_deadlock() {
        // 回归测试：混合并发调用 get_or_insert + get_or_compile，必须不死锁。
        use std::time::Duration;
        let cache = Arc::new(make_cache().await);

        let patterns = [r"\d+", r"[a-z]+", r"\w+", r"[A-Z]+", r"\s+"];

        let mut handles = Vec::new();
        for i in 0..16u32 {
            let c = Arc::clone(&cache);
            let pattern = patterns[(i % 5) as usize].to_string();
            handles.push(tokio::task::spawn_blocking(move || {
                if i % 2 == 0 {
                    c.get_or_insert(&pattern).map(|_| ())
                } else {
                    c.get_or_compile(&pattern).map(|_| ())
                }
            }));
        }

        for handle in handles {
            let result = tokio::time::timeout(Duration::from_secs(5), handle)
                .await
                .expect("concurrent mixed operations must not deadlock (timed out)");
            result
                .expect("task should not panic")
                .expect("regex should compile");
        }
    }
}
