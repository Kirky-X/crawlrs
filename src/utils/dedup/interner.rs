// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! URL Interner — Bloom 预筛 + HashSet 精确校验双层缓存
//!
//! 简化移植自 spider `spider/src/utils/interner.rs`（ListBucket）。
//!
//! ## 与 spider 原版的差异
//!
//! | 维度 | spider | crawlrs |
//! |------|--------|---------|
//! | HashSet | `hashbrown::HashSet<SymbolUsize>` + `string_interner` | `hashbrown::HashSet<String>` |
//! | Key type | `CaseInsensitiveString` | `String`（区分大小写） |
//! | Bloom 集成 | spider 侧按其自有 feature 门控 | 强制启用（本 crate 无 bloom feature） |
//!
//! **不引入 `string_interner` 依赖**的原因：crawlrs 的 Bloom+Interner 是短生命
//! 周期缓存（每个爬取任务一个实例），string_interner 的内存优化收益有限；
//! 用 `HashSet<String>` 直接匹配 DB 返回类型（`HashSet<String>`），
//! 避免额外的 Symbol↔String 转换开销。
//!
//! ## 线程安全
//!
//! `UrlInterner` 是 `Send + Sync`——Bloom 为 oxcache `BloomFilter`
//! （`Arc<RwLock>` 库内线程安全，替代了原 `MmapBloom` 的
//! `unsafe impl Send/Sync`），HashSet 本身 Send+Sync。`insert` 仍需
//! `&mut self`（HashSet 写入需独占借用），多实例共享仍走外层锁。

use super::bloom::{UrlBloom, BLOOM_DEFAULT_CAPACITY, BLOOM_TARGET_FPR};
use hashbrown::HashSet;

/// URL interner —— Bloom 预筛 + HashSet 精确校验双层缓存
///
/// 提供两种语义的 membership 查询：
/// - `contains(&self, url) -> bool`：返回**权威结论**（Bloom 阴性 → 不存在；
///   Bloom 阳性 → 查 HashSet 精确判定）
/// - `definitely_absent(&self, url) -> bool`：返回 Bloom 是否判定为绝对不存在
///   （用于"绝对新 URL"快速路径，无需 HashSet 查询）
pub struct UrlInterner {
    /// 已访问 URL 的精确集合（HashSet）
    links_visited: HashSet<String>,
    /// oxcache BloomFilter 预筛（L1 快速负向缓存）
    bloom: UrlBloom,
}

impl std::fmt::Debug for UrlInterner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // oxcache BloomFilter 未实现 Debug，按需输出可观测统计量
        f.debug_struct("UrlInterner")
            .field("links_visited_len", &self.links_visited.len())
            .field("bloom_len", &self.bloom.len())
            .field("bloom_capacity", &self.bloom.capacity())
            .finish()
    }
}

impl Default for UrlInterner {
    fn default() -> Self {
        Self {
            links_visited: HashSet::new(),
            bloom: UrlBloom::new(BLOOM_DEFAULT_CAPACITY, BLOOM_TARGET_FPR),
        }
    }
}

impl UrlInterner {
    /// 创建空 interner（默认 Bloom 容量 1M URLs，假阳性率 0.01）
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建指定 Bloom 容量的 interner
    ///
    /// `capacity == 0` 归一为 [`BLOOM_DEFAULT_CAPACITY`]（库对 0 容量直接 panic，
    /// 此处保留迁移前 `MmapBloom::new(0)` 的归一语义）。
    pub fn with_capacity(capacity: usize) -> Self {
        let cap = if capacity == 0 {
            BLOOM_DEFAULT_CAPACITY
        } else {
            capacity
        };
        Self {
            links_visited: HashSet::new(),
            bloom: UrlBloom::new(cap, BLOOM_TARGET_FPR),
        }
    }

    /// 添加一个 URL 到 interner（同时插入 Bloom 和 HashSet）
    ///
    /// 重复插入不会增加计数（HashSet 语义）。
    ///
    /// 装载率 ≥90% 时先以权威 HashSet 重建 2× 容量布隆（P11）——
    /// 布隆接近满载后误判率持续攀升，会把越来越多的新 URL 错误地
    /// 打入 DB 校验慢路径；重建后 m/k 按新容量重算，FP 率回到目标值。
    #[inline]
    pub fn insert(&mut self, url: impl AsRef<str>) {
        let url_ref = url.as_ref();
        // 阈值用 HashSet 精确长度而非 bloom.len()（后者计每次 insert 调用，
        // 重复插入/remove 流量下会提前触发多余重建）。溢出安全：usize 下安全。
        if self.links_visited.len() * 10 >= self.bloom.capacity() * 9 {
            self.rebuild_bloom_scaled();
        }
        self.bloom.insert(url_ref);
        self.links_visited.insert(url_ref.to_string());
    }

    /// 以权威 HashSet 重建 2 倍容量的布隆过滤器
    ///
    /// 权威集合是精确真源（HashSet 全量在内存），重建只重放哈希，
    /// 位图按新容量重算后 FP 率恢复到构造目标（BLOOM_TARGET_FPR）。
    /// 库 `BloomFilter` 的 `Clone` 共享位图（非深拷贝），故重建走
    /// "新实例 + HashSet 重放"而非克隆。
    fn rebuild_bloom_scaled(&mut self) {
        let new_cap = self.bloom.capacity().saturating_mul(2);
        let new_bloom = UrlBloom::new(new_cap, BLOOM_TARGET_FPR);
        for url in &self.links_visited {
            new_bloom.insert(url);
        }
        self.bloom = new_bloom;
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
    pub fn bloom(&self) -> &UrlBloom {
        &self.bloom
    }
}

impl Clone for UrlInterner {
    /// 深拷贝语义：库 `BloomFilter::clone` 共享位图（Arc 克隆），故此处以
    /// 权威 HashSet 重放构建独立位图，保持"克隆体互不影响"的原语义。
    fn clone(&self) -> Self {
        let capability = self.bloom.capacity();
        let bloom = UrlBloom::new(capability, BLOOM_TARGET_FPR);
        for url in &self.links_visited {
            bloom.insert(url);
        }
        Self {
            links_visited: self.links_visited.clone(),
            bloom,
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

    /// P11 回归钉：装载率 ≥90% 时布隆自动 2× 扩容重建，且重建后
    /// 所有已插入元素必须仍然命中（无假阴性）
    #[test]
    fn test_interner_rebuilds_bloom_when_near_capacity() {
        let mut interner = UrlInterner::with_capacity(16);
        for i in 0..20 {
            interner.insert(format!("https://example.com/page{i}"));
        }
        assert_eq!(
            interner.bloom().capacity(),
            32,
            "bloom must double capacity once load passes 90% of 16"
        );
        assert_eq!(interner.bloom().len(), 20);
        assert_eq!(interner.len(), 20, "HashSet count must be exact");
        for i in 0..20 {
            assert!(
                interner.contains(&format!("https://example.com/page{i}")),
                "rebuild must not introduce false negatives for page{i}"
            );
        }
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
        assert_eq!(interner.bloom().capacity(), 1000);
        assert_eq!(interner.bloom().len(), 0);
    }

    /// capacity=0 归一为默认 1M（库对 0 容量 panic，保留迁移前归一语义）
    #[test]
    fn test_with_capacity_zero_normalized_to_default() {
        let interner = UrlInterner::with_capacity(0);
        assert!(interner.is_empty());
        assert_eq!(interner.bloom().capacity(), BLOOM_DEFAULT_CAPACITY);
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
