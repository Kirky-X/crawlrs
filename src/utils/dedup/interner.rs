// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! URL Interner — Bloom 预筛 + HashSet 精确校验双层缓存
//! （design.md §9，T052/R-frontier-001）
//!
//! 简化移植自 spider `spider/src/utils/interner.rs`（ListBucket）。
//!
//! ## 与 spider 原版的差异
//!
//! | 维度 | spider | crawlrs |
//! |------|--------|---------|
//! | HashSet | `hashbrown::HashSet<SymbolUsize>` + `string_interner` | `hashbrown::HashSet<String>` |
//! | Key type | `CaseInsensitiveString` | `String`（区分大小写） |
//! | Bloom 集成 | `#[cfg(feature = "bloom")]` | 强制启用 |
//!
//! **不引入 `string_interner` 依赖**的原因：crawlrs 的 Bloom+Interner 是短生命
//! 周期缓存（每个爬取任务一个实例），string_interner 的内存优化收益有限；
//! 用 `HashSet<String>` 直接匹配 DB 返回类型（`HashSet<String>`），
//! 避免额外的 Symbol↔String 转换开销。
//!
//! ## 线程安全
//!
//! `UrlInterner` 是 `Send + Sync`（MmapBloom + hashbrown HashSet 均为 Send + Sync），
//! 但 `insert` 需 `&mut self`（spider 原设计为单线程插入/查询，匹配 ListBucket
//! 使用模式）。多线程共享需外层 `Mutex/RwLock`。

use super::bloom::MmapBloom;
use hashbrown::HashSet;

/// URL interner —— Bloom 预筛 + HashSet 精确校验双层缓存
///
/// 提供两种语义的 membership 查询：
/// - `contains(&self, url) -> bool`：返回**权威结论**（Bloom 阴性 → 不存在；
///   Bloom 阳性 → 查 HashSet 精确判定）
/// - `definitely_absent(&self, url) -> bool`：返回 Bloom 是否判定为绝对不存在
///   （用于"绝对新 URL"快速路径，无需 HashSet 查询）
#[derive(Debug)]
pub struct UrlInterner {
    /// 已访问 URL 的精确集合（HashSet）
    links_visited: HashSet<String>,
    /// mmap-backed bloom filter pre-check for O(1) membership queries
    bloom: MmapBloom,
}

impl Default for UrlInterner {
    fn default() -> Self {
        Self {
            links_visited: HashSet::new(),
            bloom: MmapBloom::with_default_capacity(),
        }
    }
}

impl UrlInterner {
    /// 创建空 interner（默认 Bloom 容量 1M URLs）
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建指定 Bloom 容量的 interner
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            links_visited: HashSet::new(),
            bloom: MmapBloom::new(capacity),
        }
    }

    /// 添加一个 URL 到 interner（同时插入 Bloom 和 HashSet）
    ///
    /// 重复插入不会增加计数（HashSet 语义）
    #[inline]
    pub fn insert(&mut self, url: impl AsRef<str>) {
        let url_ref = url.as_ref();
        self.bloom.insert(url_ref);
        self.links_visited.insert(url_ref.to_string());
    }

    /// 批量插入
    #[inline]
    pub fn extend<I, S>(&mut self, urls: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for u in urls {
            self.insert(u);
        }
    }

    /// 权威 membership 查询
    ///
    /// - Bloom 阴性 → 直接返回 `false`（无假阴性保证，fast path）
    /// - Bloom 阳性 → 查 HashSet 精确判定（always correct）
    #[inline]
    pub fn contains(&self, url: &str) -> bool {
        // Bloom filter says "definitely not present" → skip HashSet.
        if !self.bloom.contains(url) {
            return false;
        }
        self.links_visited.contains(url)
    }

    /// Bloom 是否判定为绝对不存在（fast path）
    ///
    /// 用于"绝对新 URL"快速入队决策：返回 `true` 时无需 DB/HashSet 校验。
    #[inline]
    pub fn definitely_absent(&self, url: &str) -> bool {
        !self.bloom.contains(url)
    }

    /// 从 interner 移除一个 URL（仅 HashSet 移除，Bloom 不支持移除）
    ///
    /// Bloom 不能移除单个元素——bloom filter 不支持删除。
    /// 移除后 Bloom 仍可能阳性，但 HashSet 查询会正确返回 false。
    /// 适用于"重试"场景：标记 URL 可重新入队。
    #[inline]
    pub fn remove(&mut self, url: &str) -> bool {
        self.links_visited.remove(url)
    }

    /// 已访问 URL 数量（精确）
    #[inline]
    pub fn len(&self) -> usize {
        self.links_visited.len()
    }

    /// 是否为空
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.links_visited.is_empty()
    }

    /// 清空 interner（HashSet 清空 + Bloom reset）
    ///
    /// Bloom 用 `clear()` 重置全部 bit（spider 同款语义），
    /// 而非仅清 HashSet，确保 Bloom 不会保留 stale 信息。
    pub fn clear(&mut self) {
        self.links_visited.clear();
        self.bloom.clear();
    }

    /// 获取所有已访问 URL 的克隆集合（用于持久化/调试）
    pub fn get_links(&self) -> HashSet<String> {
        self.links_visited.clone()
    }

    /// Bloom 内部引用（用于调试/统计）
    pub fn bloom(&self) -> &MmapBloom {
        &self.bloom
    }
}

impl Clone for UrlInterner {
    fn clone(&self) -> Self {
        Self {
            links_visited: self.links_visited.clone(),
            bloom: self.bloom.clone(),
        }
    }
}

impl PartialEq for UrlInterner {
    fn eq(&self, other: &Self) -> bool {
        self.links_visited == other.links_visited
    }
}

impl Eq for UrlInterner {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_is_empty() {
        let interner = UrlInterner::new();
        assert!(interner.is_empty());
        assert_eq!(interner.len(), 0);
    }

    #[test]
    fn test_insert_contains() {
        let mut interner = UrlInterner::new();
        interner.insert("https://example.com");
        assert!(interner.contains("https://example.com"));
        assert!(!interner.contains("https://other.com"));
    }

    #[test]
    fn test_definitely_absent() {
        let mut interner = UrlInterner::new();
        interner.insert("https://example.com");
        // 已插入的 URL 不算 definitely_absent（Bloom 阳性）
        assert!(!interner.definitely_absent("https://example.com"));
        // 未插入的 URL 应该 definitely_absent（Bloom 阴性，无假阴性）
        assert!(interner.definitely_absent("https://other.com"));
    }

    #[test]
    fn test_len_after_insert() {
        let mut interner = UrlInterner::new();
        interner.insert("https://a.com");
        interner.insert("https://b.com");
        assert_eq!(interner.len(), 2);
    }

    #[test]
    fn test_duplicate_insert_no_count_increase() {
        let mut interner = UrlInterner::new();
        interner.insert("https://a.com");
        interner.insert("https://a.com");
        assert_eq!(interner.len(), 1);
    }

    #[test]
    fn test_extend() {
        let mut interner = UrlInterner::new();
        interner.extend(["https://a.com", "https://b.com", "https://c.com"]);
        assert_eq!(interner.len(), 3);
        assert!(interner.contains("https://a.com"));
        assert!(interner.contains("https://b.com"));
        assert!(interner.contains("https://c.com"));
    }

    #[test]
    fn test_clear() {
        let mut interner = UrlInterner::new();
        interner.insert("https://a.com");
        interner.insert("https://b.com");
        assert_eq!(interner.len(), 2);

        interner.clear();
        assert!(interner.is_empty());
        assert_eq!(interner.len(), 0);
        // 清空后 Bloom 也应重置
        assert!(interner.definitely_absent("https://a.com"));
    }

    #[test]
    fn test_remove() {
        let mut interner = UrlInterner::new();
        interner.insert("https://a.com");
        assert!(interner.contains("https://a.com"));
        assert!(interner.remove("https://a.com"));
        // HashSet 中已移除
        assert!(!interner.contains("https://a.com"));
        // Bloom 仍阳性（bloom 不支持移除），但 contains 走 Bloom→HashSet 后正确返回 false
    }

    #[test]
    fn test_remove_nonexistent_returns_false() {
        let mut interner = UrlInterner::new();
        assert!(!interner.remove("https://nonexistent.com"));
    }

    #[test]
    fn test_get_links() {
        let mut interner = UrlInterner::new();
        interner.insert("https://a.com");
        interner.insert("https://b.com");
        let links = interner.get_links();
        assert_eq!(links.len(), 2);
        assert!(links.contains("https://a.com"));
        assert!(links.contains("https://b.com"));
    }

    #[test]
    fn test_clone() {
        let mut interner = UrlInterner::new();
        interner.insert("https://a.com");
        let cloned = interner.clone();
        assert_eq!(cloned.len(), 1);
        assert!(cloned.contains("https://a.com"));
    }

    #[test]
    fn test_clone_independence() {
        let mut interner = UrlInterner::new();
        interner.insert("https://a.com");
        let mut cloned = interner.clone();
        cloned.insert("https://b.com");

        // 修改 clone 不影响原 interner
        assert!(!interner.contains("https://b.com"));
        assert!(cloned.contains("https://b.com"));
    }

    #[test]
    fn test_with_capacity() {
        let interner = UrlInterner::with_capacity(1000);
        assert!(interner.is_empty());
        assert!(interner.bloom().size_bytes() > 0);
    }

    #[test]
    fn test_large_insert_no_false_negatives() {
        let mut interner = UrlInterner::with_capacity(5000);
        let urls: Vec<String> = (0..5000)
            .map(|i| format!("https://site.com/{}", i))
            .collect();
        for url in &urls {
            interner.insert(url);
        }
        // 全部应能命中（bloom 无假阴性，HashSet 精确）
        for url in &urls {
            assert!(interner.contains(url), "False negative for {}", url);
        }
    }

    #[test]
    fn test_bloom_pre_check_skips_hashset() {
        // 验证 Bloom 阴性时 fast path：未插入的 URL 不应进入 HashSet 路径
        let mut interner = UrlInterner::new();
        interner.insert("https://a.com");

        // definitely_absent 用 Bloom 阴性判断，未插入 URL 应返回 true
        assert!(interner.definitely_absent("https://b.com"));
        assert!(interner.definitely_absent("https://c.com"));
        // 已插入 URL 在 Bloom 中阳性，definitely_absent 返回 false
        assert!(!interner.definitely_absent("https://a.com"));
    }
}
