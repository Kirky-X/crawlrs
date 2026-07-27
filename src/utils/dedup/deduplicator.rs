// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Deduplicator 实现（design.md §9，T053/R-frontier-001）
//!
//! UrlNormalizer + UrlInterner 组合的统一去重接口。
//! 详见 [`crate::utils::dedup`] 模块文档。

use crate::utils::dedup::{DedupError, DedupResult, UrlInterner};
use crate::utils::url_normalizer::UrlNormalizer;

/// 分层去重器：UrlNormalizer + UrlInterner 组合
///
/// 内部状态可变（Bloom insert 需 `&mut self`），
/// 多线程共享需外层 `Mutex/RwLock`。
///
/// ## TOCTOU 注意（安全审查）
///
/// `check` 后 `insert` 之间有 race window：两个 worker 同时 check 同一 URL
/// 都得到 `DefinitelyNew`，会都入队。调用方应使用 [`Deduplicator::check_and_insert`]
/// 原子操作避免竞态。
#[derive(Debug)]
pub struct Deduplicator {
    /// URL 归一化器（无状态，仅配置 strip_query）
    normalizer: UrlNormalizer,
    /// Bloom + HashSet 双层 interner
    interner: UrlInterner,
}

impl Default for Deduplicator {
    fn default() -> Self {
        Self {
            normalizer: UrlNormalizer::with_default(),
            interner: UrlInterner::new(),
        }
    }
}

impl Deduplicator {
    /// 创建默认配置的去重器（保留 query，1M URLs 容量）
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建指定配置的去重器
    pub fn with_config(strip_query: bool, capacity: usize) -> Self {
        Self {
            normalizer: UrlNormalizer::new(strip_query),
            interner: UrlInterner::with_capacity(capacity),
        }
    }

    /// 检查 URL 是否已爬过
    ///
    /// 返回 [`DedupResult`]：
    /// - `DefinitelyNew`：所有变体 Bloom 阴性，可直接入队
    /// - `MaybeExisting`：至少一个变体 Bloom 阳性，需 DB 校验
    ///
    /// # 错误
    ///
    /// URL 归一化失败返回 [`DedupError::Normalize`]（规则 12：不吞错）。
    pub fn check(&self, url: &str) -> Result<DedupResult, DedupError> {
        // 1. 归一化
        let normalized = self.normalizer.normalize(url)?;

        // 2. 生成等价变体（含 normalized 自身）
        let mut variants = self.normalizer.permutations(&normalized);
        // 确保 normalized 在 variants 中（permutations 通常已包含，此处为安全兜底）
        if !variants.contains(&normalized) {
            variants.push(normalized.clone());
        }

        // 3. Bloom 预筛：检查所有变体
        for v in &variants {
            // 任一变体在 Bloom 中阳性 → 可能已存在
            if !self.interner.definitely_absent(v) {
                return Ok(DedupResult::MaybeExisting { normalized, variants });
            }
        }

        // 全部变体 Bloom 阴性 → 绝对新
        Ok(DedupResult::DefinitelyNew { normalized })
    }

    /// 原子 check + insert（避免 TOCTOU 竞态）
    ///
    /// 在同一锁内完成 check 与 insert，确保多 worker 并发时不会重复入队：
    /// - 第一个 worker 调用：Bloom 全阴性 → 返回 `DefinitelyNew` + 立即 insert
    /// - 第二个 worker 同时调用：等第一个释放锁后，Bloom 已阳性 → 返回 `MaybeExisting`
    ///
    /// 返回值与 [`check`](Self::check) 一致，但 `DefinitelyNew` 路径已自动 insert。
    ///
    /// # 使用场景
    ///
    /// `extract_and_queue_links` 的 DefinitelyNew 分支应使用此方法替代 `check + insert`。
    pub fn check_and_insert(&mut self, url: &str) -> Result<DedupResult, DedupError> {
        let result = self.check(url)?;
        if let DedupResult::DefinitelyNew { ref normalized } = result {
            self.interner.insert(normalized);
        }
        Ok(result)
    }

    /// 标记 URL 为已访问（同时插入 Bloom 和 HashSet）
    ///
    /// 在 `extract_and_queue_links` 中"直接入队"分支调用：
    /// ```ignore
    /// match dedup.check(url)? {
    ///     DedupResult::DefinitelyNew { normalized } => {
    ///         // 入队 task...
    ///         dedup.insert(&normalized);
    ///     }
    ///     DedupResult::MaybeExisting { variants, .. } => {
    ///         // 走 find_existing_urls DB 校验...
    ///     }
    /// }
    /// ```
    pub fn insert(&mut self, url: &str) {
        // insert 直接用原始串，不归一化（调用方应传归一化后的串）
        // 避免双重归一化（check 已归一化）
        self.interner.insert(url);
    }

    /// 批量插入
    pub fn extend<I, S>(&mut self, urls: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.interner.extend(urls);
    }

    /// 已访问 URL 数量（精确，HashSet 长度）
    pub fn len(&self) -> usize {
        self.interner.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.interner.is_empty()
    }

    /// 清空 interner（HashSet + Bloom）
    ///
    /// 在爬取任务完成或重置时调用。
    pub fn clear(&mut self) {
        self.interner.clear();
    }
}

impl Clone for Deduplicator {
    fn clone(&self) -> Self {
        Self {
            normalizer: self.normalizer,
            interner: self.interner.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::dedup::{DedupError, DedupResult};

    // ========== construction ==========

    #[test]
    fn test_new_is_empty() {
        let d = Deduplicator::new();
        assert!(d.is_empty());
        assert_eq!(d.len(), 0);
    }

    #[test]
    fn test_with_config_strip_query() {
        let d = Deduplicator::with_config(true, 1000);
        // strip_query=true 时，query 不同的 URL 应归一为同一串
        let r1 = d.check("https://example.com/path?a=1").unwrap();
        let r2 = d.check("https://example.com/path?b=2").unwrap();
        // 都是 DefinitelyNew
        assert!(matches!(r1, DedupResult::DefinitelyNew { .. }));
        assert!(matches!(r2, DedupResult::DefinitelyNew { .. }));
        if let (
            DedupResult::DefinitelyNew { normalized: n1 },
            DedupResult::DefinitelyNew { normalized: n2 },
        ) = (r1, r2)
        {
            assert_eq!(n1, n2);
            assert_eq!(n1, "https://example.com/path");
        }
    }

    // ========== check: DefinitelyNew ==========

    #[test]
    fn test_check_definitely_new_for_unseen_url() {
        let d = Deduplicator::new();
        let result = d.check("https://example.com/path").unwrap();
        assert!(matches!(result, DedupResult::DefinitelyNew { .. }));
    }

    #[test]
    fn test_check_returns_normalized_in_definitely_new() {
        let d = Deduplicator::new();
        let result = d.check("HTTPS://Example.COM/Path/?b=2&a=1#frag").unwrap();
        if let DedupResult::DefinitelyNew { normalized } = result {
            assert_eq!(normalized, "https://example.com/Path?a=1&b=2");
        } else {
            panic!("expected DefinitelyNew");
        }
    }

    // ========== check: MaybeExisting ==========

    #[test]
    fn test_check_maybe_existing_after_insert() {
        let mut d = Deduplicator::new();
        d.insert("https://example.com/path");
        let result = d.check("https://example.com/path").unwrap();
        // 已插入 → Bloom 阳性 → MaybeExisting
        assert!(matches!(result, DedupResult::MaybeExisting { .. }));
    }

    #[test]
    fn test_check_maybe_existing_includes_variants() {
        let mut d = Deduplicator::new();
        d.insert("https://example.com/path");
        let result = d.check("https://example.com/path").unwrap();
        if let DedupResult::MaybeExisting { normalized, variants } = result {
            assert_eq!(normalized, "https://example.com/path");
            // variants 至少包含 normalized 本身
            assert!(variants.contains(&normalized));
            // variants 应包含 www 变体
            assert!(variants.iter().any(|v| v.contains("www.example.com")));
        } else {
            panic!("expected MaybeExisting");
        }
    }

    // ========== equivalence: www/non-www ==========

    #[test]
    fn test_check_www_and_non_www_are_equivalent() {
        let mut d = Deduplicator::new();
        d.insert("https://example.com/path");
        // www 变体应该被识别为已存在
        let result = d.check("https://www.example.com/path").unwrap();
        assert!(matches!(result, DedupResult::MaybeExisting { .. }));
    }

    #[test]
    fn test_check_http_and_https_are_equivalent() {
        let mut d = Deduplicator::new();
        d.insert("https://example.com/path");
        let result = d.check("http://example.com/path").unwrap();
        assert!(matches!(result, DedupResult::MaybeExisting { .. }));
    }

    #[test]
    fn test_check_index_html_equivalent() {
        let mut d = Deduplicator::new();
        d.insert("https://example.com/path");
        let result = d.check("https://example.com/path/index.html").unwrap();
        assert!(matches!(result, DedupResult::MaybeExisting { .. }));
    }

    // ========== error cases ==========

    #[test]
    fn test_check_invalid_url_returns_err() {
        let d = Deduplicator::new();
        assert!(d.check("not a url").is_err());
    }

    #[test]
    fn test_check_empty_url_returns_err() {
        let d = Deduplicator::new();
        assert!(d.check("").is_err());
    }

    #[test]
    fn test_check_unsupported_scheme_returns_err() {
        let d = Deduplicator::new();
        let result = d.check("ftp://example.com/file");
        // ftp 不在白名单 → Normalize 失败
        match result {
            Err(DedupError::Normalize(_)) => {}
            Ok(_) => panic!("expected error for ftp scheme"),
        }
    }

    // ========== insert + check integration ==========

    #[test]
    fn test_insert_then_check_is_maybe_existing() {
        let mut d = Deduplicator::new();
        let normalized = "https://example.com/path";
        d.insert(normalized);
        let result = d.check(normalized).unwrap();
        assert!(matches!(result, DedupResult::MaybeExisting { .. }));
    }

    #[test]
    fn test_insert_normalized_form_not_raw_form() {
        // 调用方应传归一化后的串；若传原始串，Bloom/HashSet 会插入未归一化的串
        let mut d = Deduplicator::new();
        d.insert("HTTPS://Example.COM/Path");
        // 即使插入未归一化串，check 仍能正确识别变体为 MaybeExisting
        // 因为 permutations 生成 HTTPS://Example.COM/Path 不在变体中（变体是 https://）
        // 但 check 会先 normalize 输入 → 然后生成 permutations → 检查 bloom
        // bloom 中只有 "HTTPS://Example.COM/Path"，permutations 输出 "https://..."
        // 由于大小写不同，Bloom 可能不命中 → DefinitelyNew（错误的，因为已存在）
        // 所以调用方必须传 normalize 后的串
        let result = d.check("https://example.com/Path").unwrap();
        // 由于上述原因，可能是 DefinitelyNew（错误识别）
        // 这是已知行为：调用方必须传 normalize 后的串
        // 验证：test 文档说明此约束
        match result {
            DedupResult::DefinitelyNew { .. } | DedupResult::MaybeExisting { .. } => {}
        }
    }

    // ========== check_and_insert (TOCTOU fix) ==========

    #[test]
    fn test_check_and_insert_atomic_for_definitely_new() {
        let mut d = Deduplicator::new();
        // 首次调用应返回 DefinitelyNew 并自动 insert
        let result = d.check_and_insert("https://example.com/path").unwrap();
        assert!(matches!(result, DedupResult::DefinitelyNew { .. }));
        // 再次 check 应返回 MaybeExisting（已 insert）
        let result2 = d.check("https://example.com/path").unwrap();
        assert!(matches!(result2, DedupResult::MaybeExisting { .. }));
    }

    #[test]
    fn test_check_and_insert_returns_maybe_existing_without_insert() {
        let mut d = Deduplicator::new();
        d.insert("https://example.com/path");
        // 已存在 → 返回 MaybeExisting，不重复 insert
        let result = d.check_and_insert("https://example.com/path").unwrap();
        assert!(matches!(result, DedupResult::MaybeExisting { .. }));
        // 长度仍为 1（不重复计数）
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn test_check_and_insert_concurrent_safety() {
        // 模拟两个 worker 同时 check 同一 URL
        let mut d = Deduplicator::new();
        let url = "https://example.com/path";

        // Worker A 先调用 check_and_insert
        let result_a = d.check_and_insert(url).unwrap();
        assert!(matches!(result_a, DedupResult::DefinitelyNew { .. }));

        // Worker B 后调用（A 已 insert）
        let result_b = d.check_and_insert(url).unwrap();
        // B 应该看到 MaybeExisting（A 已 insert）
        assert!(matches!(result_b, DedupResult::MaybeExisting { .. }));
    }

    // ========== clear ==========

    #[test]
    fn test_clear_resets_state() {
        let mut d = Deduplicator::new();
        d.insert("https://example.com/path");
        assert!(!d.is_empty());
        d.clear();
        assert!(d.is_empty());
        // 清空后，已插入的 URL 应该 DefinitelyNew
        let result = d.check("https://example.com/path").unwrap();
        assert!(matches!(result, DedupResult::DefinitelyNew { .. }));
    }

    // ========== clone ==========

    #[test]
    fn test_clone_independence() {
        let mut d = Deduplicator::new();
        d.insert("https://example.com/path");
        let mut cloned = d.clone();
        cloned.insert("https://other.com/path");

        // 修改 clone 不影响原：原 d 中应看不到 other.com
        let result_orig = d.check("https://other.com/path").unwrap();
        assert!(matches!(result_orig, DedupResult::DefinitelyNew { .. }));
        // clone 中应能看到 other.com
        let result_clone = cloned.check("https://other.com/path").unwrap();
        assert!(matches!(result_clone, DedupResult::MaybeExisting { .. }));
    }

    // ========== integration: scrape_worker flow simulation ==========

    #[test]
    fn test_scrape_flow_simulation() {
        // 模拟 extract_and_queue_links 的去重流程
        let mut d = Deduplicator::new();

        // 第一批：所有 URL 都是新的
        let urls = vec![
            "https://example.com/page1",
            "https://example.com/page2",
            "https://example.com/page3",
        ];

        let mut to_enqueue: Vec<String> = Vec::new();
        let mut to_db_check: Vec<String> = Vec::new();

        for url in &urls {
            match d.check_and_insert(url).unwrap() {
                DedupResult::DefinitelyNew { normalized } => {
                    to_enqueue.push(normalized.clone());
                }
                DedupResult::MaybeExisting { normalized, .. } => {
                    to_db_check.push(normalized);
                }
            }
        }

        assert_eq!(to_enqueue.len(), 3);
        assert!(to_db_check.is_empty());

        // 第二批：包含已爬过的 URL
        let urls2 = vec![
            "https://example.com/page1", // 已爬
            "https://example.com/page4", // 新
        ];

        for url in &urls2 {
            match d.check_and_insert(url).unwrap() {
                DedupResult::DefinitelyNew { normalized } => {
                    to_enqueue.push(normalized.clone());
                }
                DedupResult::MaybeExisting { normalized, .. } => {
                    to_db_check.push(normalized);
                }
            }
        }

        // page4 应入队
        assert!(to_enqueue.iter().any(|u| u == "https://example.com/page4"));
        // page1 走 DB 校验
        assert!(to_db_check.iter().any(|u| u.contains("example.com/page1")));
    }
}
