// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 优先级前沿队列（T065，R-frontier-003）
//!
//! 参考 crawl4ai `deep_crawling/frontier.py` 与 spider frontier 设计：
//!
//! - [`ScoredUrl`]：带分数的 URL，实现 `Ord`（高分优先）
//! - [`Frontier`]：按域名分组 `BinaryHeap` + round-robin 出队
//!
//! # 域名 round-robin 设计
//!
//! 纯按分数排序的 `BinaryHeap` 会导致**单域名饥饿**：若域名 A 有 1000 个
//! score=0.9 的 URL，域名 B 有 1 个 score=0.8 的 URL，则 B 永远无法出队。
//!
//! Frontier 将 URL 按**域名分组**，出队时在域名间 **round-robin 轮转**，
//! 每次从下一个域名的堆顶取出 score 最高的 URL。这样每个域名都能公平地
//! 获得出队机会，域内仍按分数优先排序。
//!
//! # 线程安全
//!
//! 内部用 `parking_lot::Mutex` 保护，`Frontier` 自身可被 `Arc` 共享给多 worker。
//! push/pop 是短临界区（无 I/O、无 await），适合 mutex 而非 DashMap。

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use parking_lot::Mutex;
use url::Url;

// =============================================================================
// ScoredUrl
// =============================================================================

/// 带分数的 URL（T065，R-frontier-003）
///
/// 由 [`crate::workers::crawl::UrlScorer`] 评分后包装为 `ScoredUrl`，
/// 推入 [`Frontier`] 等待出队。
///
/// # Ord 语义
///
/// `BinaryHeap` 是 max-heap，需要**高分优先**。`Ord::cmp` 按 score 降序排列：
/// `other.score.cmp(&self.score)`，使高分 URL 排在堆顶。
///
/// # 域名提取
///
/// [`ScoredUrl::new`] 从 URL 解析域名（小写化）。解析失败时返回 `Err`，
/// 由调用方决定是否丢弃（规则 12：失败必须显性化）。
#[derive(Debug, Clone)]
pub struct ScoredUrl {
    /// 已归一化的 URL
    pub url: String,
    /// 相关性分数 `[0.0, 1.0]`，越高越优先
    pub score: f32,
    /// URL 的域名（小写化，用于域名分组 round-robin）
    pub domain: String,
}

impl ScoredUrl {
    /// 从 URL 和分数构造 `ScoredUrl`，自动提取域名（小写化）
    ///
    /// # 错误
    ///
    /// URL 解析失败或无 host（如 `file:///`、`mailto:`）时返回 `Err`。
    ///
    /// # 示例
    ///
    /// ```ignore
    /// use crate::workers::crawl::frontier::ScoredUrl;
    ///
    /// let scored = ScoredUrl::new("https://Example.COM/blog/post".to_string(), 0.8)?;
    /// assert_eq!(scored.domain, "example.com");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(url: String, score: f32) -> Result<Self, FrontierError> {
        let domain = Self::extract_domain(&url)?;
        Ok(Self { url, score, domain })
    }

    /// 直接用已知域名构造（跳过 URL 解析，用于测试或已预解析场景）
    #[must_use]
    pub fn with_domain(url: String, score: f32, domain: String) -> Self {
        Self { url, score, domain }
    }

    /// 从 URL 提取域名（小写化）
    ///
    /// 返回 `None` 的情况：
    /// - URL 解析失败
    /// - URL 无 host（如 `file:///`、`mailto:`、`javascript:`）
    /// - host 为空字符串
    fn extract_domain(url_str: &str) -> Result<String, FrontierError> {
        let parsed = Url::parse(url_str).map_err(|e| FrontierError::UrlParse(e.to_string()))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| FrontierError::NoHost(url_str.to_string()))?;
        if host.is_empty() {
            return Err(FrontierError::NoHost(url_str.to_string()));
        }
        Ok(host.to_ascii_lowercase())
    }
}

impl PartialEq for ScoredUrl {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
    }
}

impl Eq for ScoredUrl {}

impl PartialOrd for ScoredUrl {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredUrl {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap 是 max-heap，高分应在堆顶（greatest）。
        // 因此 self.score > other.score → 返回 Greater → self 排在堆顶。
        // f32 的 partial_cmp 返回 None 时（NaN）用 Equal 兜底。
        self.score
            .partial_cmp(&other.score)
            .unwrap_or(Ordering::Equal)
    }
}

// =============================================================================
// Frontier
// =============================================================================

/// 优先级前沿队列（T065，R-frontier-003）
///
/// 按域名分组 `BinaryHeap` + round-robin 出队，避免单域名饥饿。
///
/// # 设计
///
/// - **push**：按 `ScoredUrl.domain` 分组，推入对应域名的 `BinaryHeap`
/// - **pop**：从 `cursor` 位置开始扫描域名列表，找到第一个非空堆并弹出堆顶，
///   然后 `cursor` 前进到下一域名（round-robin）
///
/// 域内按分数排序（[`ScoredUrl::Ord`]），域名间按 round-robin 公平调度。
///
/// # 线程安全
///
/// `Frontier` 内部用 `parking_lot::Mutex` 保护所有状态。`Arc<Frontier>`
/// 可被多个 scrape worker 共享。push/pop 临界区极短（无 I/O），mutex 足够。
///
/// # 示例
///
/// ```ignore
/// use crate::workers::crawl::frontier::{Frontier, ScoredUrl};
///
/// let frontier = Frontier::new();
/// frontier.push(ScoredUrl::with_domain("https://a.com/1".to_string(), 0.9, "a.com".to_string()));
/// frontier.push(ScoredUrl::with_domain("https://b.com/1".to_string(), 0.5, "b.com".to_string()));
///
/// // round-robin: 先弹 a.com（cursor=0），再弹 b.com（cursor=1）
/// let first = frontier.pop().unwrap();
/// let second = frontier.pop().unwrap();
/// assert_eq!(first.domain, "a.com");
/// assert_eq!(second.domain, "b.com");
/// ```
pub struct Frontier {
    inner: Mutex<FrontierInner>,
}

struct FrontierInner {
    /// 每域名的优先级队列
    domains: HashMap<String, BinaryHeap<ScoredUrl>>,
    /// 域名插入顺序（round-robin 按此顺序轮转）
    domain_order: Vec<String>,
    /// round-robin 游标（指向下一个待出队的域名索引）
    cursor: usize,
    /// 总 URL 数
    len: usize,
}

impl Frontier {
    /// 构造空的前沿队列
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(FrontierInner {
                domains: HashMap::new(),
                domain_order: Vec::new(),
                cursor: 0,
                len: 0,
            }),
        }
    }

    /// 推入一个已评分的 URL
    ///
    /// 按 `scored.domain` 分组推入对应域名的 `BinaryHeap`。
    /// 新域名会追加到 `domain_order` 末尾。
    pub fn push(&self, scored: ScoredUrl) {
        let mut inner = self.inner.lock();
        let domain = scored.domain.clone();
        if !inner.domains.contains_key(&domain) {
            inner.domain_order.push(domain.clone());
            inner.domains.insert(domain.clone(), BinaryHeap::new());
        }
        inner.domains.get_mut(&domain).unwrap().push(scored);
        inner.len += 1;
    }

    /// 弹出下一个 URL（round-robin 域名轮转 + 域内高分优先）
    ///
    /// 从 `cursor` 位置开始扫描 `domain_order`，找到第一个非空堆并弹出堆顶。
    /// 然后 `cursor` 前进到 `(found_index + 1) % domain_count`。
    ///
    /// 所有域名为空时返回 `None`。
    pub fn pop(&self) -> Option<ScoredUrl> {
        let mut inner = self.inner.lock();
        if inner.len == 0 || inner.domain_order.is_empty() {
            return None;
        }
        let domain_count = inner.domain_order.len();
        // 从 cursor 开始扫描所有域名，最多扫描 domain_count 次
        for offset in 0..domain_count {
            let idx = (inner.cursor + offset) % domain_count;
            let domain = inner.domain_order[idx].clone();
            if let Some(heap) = inner.domains.get_mut(&domain) {
                if let Some(scored) = heap.pop() {
                    // cursor 前进到下一域名
                    inner.cursor = (idx + 1) % domain_count;
                    inner.len -= 1;
                    return Some(scored);
                }
            }
        }
        None
    }

    /// 当前队列中的 URL 总数
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().len
    }

    /// 队列是否为空
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 当前已注册的域名数（含空堆域名）
    ///
    /// 用于测试和调试。空堆的域名仍保留在 `domain_order` 中
    /// （避免 cursor 调整复杂度，且域名数有限）。
    #[must_use]
    pub fn domain_count(&self) -> usize {
        self.inner.lock().domain_order.len()
    }

    /// 指定域名的待处理 URL 数
    ///
    /// 域名不存在时返回 0。用于测试验证 round-robin 公平性。
    #[must_use]
    pub fn domain_len(&self, domain: &str) -> usize {
        let inner = self.inner.lock();
        inner
            .domains
            .get(&domain.to_ascii_lowercase())
            .map_or(0, |h| h.len())
    }
}

impl Default for Frontier {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// FrontierError
// =============================================================================

/// Frontier 错误类型（T065）
///
/// URL 解析失败或无 host 时由 [`ScoredUrl::new`] 返回。
#[derive(Debug, thiserror::Error)]
pub enum FrontierError {
    /// URL 解析失败
    #[error("URL parse error: {0}")]
    UrlParse(String),

    /// URL 无 host（如 `file:///`、`mailto:`、`javascript:`）
    #[error("URL has no host: {0}")]
    NoHost(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============ ScoredUrl ============

    #[test]
    fn scored_url_new_extracts_domain() {
        let scored = ScoredUrl::new("https://Example.COM/blog/post".to_string(), 0.8).unwrap();
        assert_eq!(scored.domain, "example.com");
        assert_eq!(scored.url, "https://Example.COM/blog/post");
        assert!((scored.score - 0.8).abs() < 1e-6);
    }

    #[test]
    fn scored_url_new_subdomain() {
        let scored = ScoredUrl::new("https://blog.example.org/page".to_string(), 0.5).unwrap();
        assert_eq!(scored.domain, "blog.example.org");
    }

    #[test]
    fn scored_url_new_invalid_url_returns_err() {
        assert!(ScoredUrl::new("not a url".to_string(), 0.5).is_err());
        assert!(ScoredUrl::new("".to_string(), 0.5).is_err());
    }

    #[test]
    fn scored_url_new_no_host_returns_err() {
        assert!(ScoredUrl::new("file:///etc/passwd".to_string(), 0.5).is_err());
        assert!(ScoredUrl::new("mailto:test@example.com".to_string(), 0.5).is_err());
        assert!(ScoredUrl::new("javascript:void(0)".to_string(), 0.5).is_err());
    }

    #[test]
    fn scored_url_ord_high_score_first() {
        // BinaryHeap 是 max-heap，高分应在堆顶
        let heap = BinaryHeap::from(vec![
            ScoredUrl::with_domain("https://a.com/low".to_string(), 0.1, "a.com".to_string()),
            ScoredUrl::with_domain("https://a.com/high".to_string(), 0.9, "a.com".to_string()),
            ScoredUrl::with_domain("https://a.com/mid".to_string(), 0.5, "a.com".to_string()),
        ]);
        assert_eq!(heap.peek().unwrap().url, "https://a.com/high");
    }

    #[test]
    fn scored_url_eq_by_score() {
        let a = ScoredUrl::with_domain("https://a.com/1".to_string(), 0.5, "a.com".to_string());
        let b = ScoredUrl::with_domain("https://b.com/2".to_string(), 0.5, "b.com".to_string());
        assert_eq!(a, b); // 同分数即相等
    }

    // ============ Frontier 基本操作 ============

    #[test]
    fn frontier_new_is_empty() {
        let f = Frontier::new();
        assert!(f.is_empty());
        assert_eq!(f.len(), 0);
        assert_eq!(f.domain_count(), 0);
        assert!(f.pop().is_none());
    }

    #[test]
    fn frontier_push_increments_len() {
        let f = Frontier::new();
        f.push(ScoredUrl::with_domain("https://a.com/1".to_string(), 0.5, "a.com".to_string()));
        assert_eq!(f.len(), 1);
        assert!(!f.is_empty());
        assert_eq!(f.domain_count(), 1);
    }

    #[test]
    fn frontier_push_multiple_domains() {
        let f = Frontier::new();
        f.push(ScoredUrl::with_domain("https://a.com/1".to_string(), 0.5, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://b.com/1".to_string(), 0.5, "b.com".to_string()));
        f.push(ScoredUrl::with_domain("https://c.com/1".to_string(), 0.5, "c.com".to_string()));
        assert_eq!(f.len(), 3);
        assert_eq!(f.domain_count(), 3);
    }

    #[test]
    fn frontier_push_same_domain_multiple_urls() {
        let f = Frontier::new();
        f.push(ScoredUrl::with_domain("https://a.com/1".to_string(), 0.5, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://a.com/2".to_string(), 0.9, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://a.com/3".to_string(), 0.1, "a.com".to_string()));
        assert_eq!(f.len(), 3);
        assert_eq!(f.domain_count(), 1);
        assert_eq!(f.domain_len("a.com"), 3);
    }

    // ============ Frontier 高分优先出队 ============

    #[test]
    fn frontier_pop_single_domain_high_score_first() {
        let f = Frontier::new();
        f.push(ScoredUrl::with_domain("https://a.com/low".to_string(), 0.1, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://a.com/high".to_string(), 0.9, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://a.com/mid".to_string(), 0.5, "a.com".to_string()));

        // 单域名：域内按分数排序，高分优先
        let first = f.pop().unwrap();
        assert_eq!(first.url, "https://a.com/high");
        let second = f.pop().unwrap();
        assert_eq!(second.url, "https://a.com/mid");
        let third = f.pop().unwrap();
        assert_eq!(third.url, "https://a.com/low");
        assert!(f.is_empty());
        assert!(f.pop().is_none());
    }

    #[test]
    fn frontier_pop_decrements_len() {
        let f = Frontier::new();
        f.push(ScoredUrl::with_domain("https://a.com/1".to_string(), 0.5, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://a.com/2".to_string(), 0.9, "a.com".to_string()));
        assert_eq!(f.len(), 2);
        f.pop();
        assert_eq!(f.len(), 1);
        f.pop();
        assert_eq!(f.len(), 0);
        assert!(f.is_empty());
    }

    // ============ Frontier 域名 round-robin ============

    #[test]
    fn frontier_round_robin_two_domains() {
        let f = Frontier::new();
        // a.com 有 3 个 URL（都高分），b.com 有 1 个 URL（低分）
        f.push(ScoredUrl::with_domain("https://a.com/1".to_string(), 0.9, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://a.com/2".to_string(), 0.8, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://a.com/3".to_string(), 0.7, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://b.com/1".to_string(), 0.1, "b.com".to_string()));

        // round-robin: a.com → b.com → a.com → a.com
        let pop1 = f.pop().unwrap();
        assert_eq!(pop1.domain, "a.com");
        assert_eq!(pop1.url, "https://a.com/1"); // a.com 域内高分优先

        let pop2 = f.pop().unwrap();
        assert_eq!(pop2.domain, "b.com"); // b.com 轮转到
        assert_eq!(pop2.url, "https://b.com/1");

        let pop3 = f.pop().unwrap();
        assert_eq!(pop3.domain, "a.com"); // 回到 a.com
        assert_eq!(pop3.url, "https://a.com/2");

        let pop4 = f.pop().unwrap();
        assert_eq!(pop4.domain, "a.com"); // b.com 已空，继续 a.com
        assert_eq!(pop4.url, "https://a.com/3");

        assert!(f.is_empty());
    }

    #[test]
    fn frontier_round_robin_three_domains() {
        let f = Frontier::new();
        f.push(ScoredUrl::with_domain("https://a.com/1".to_string(), 0.9, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://b.com/1".to_string(), 0.5, "b.com".to_string()));
        f.push(ScoredUrl::with_domain("https://c.com/1".to_string(), 0.1, "c.com".to_string()));
        f.push(ScoredUrl::with_domain("https://a.com/2".to_string(), 0.8, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://b.com/2".to_string(), 0.4, "b.com".to_string()));

        // round-robin: a → b → c → a → b
        let domains: Vec<String> = vec![
            f.pop().unwrap().domain,
            f.pop().unwrap().domain,
            f.pop().unwrap().domain,
            f.pop().unwrap().domain,
            f.pop().unwrap().domain,
        ];
        assert_eq!(domains, vec!["a.com", "b.com", "c.com", "a.com", "b.com"]);
    }

    #[test]
    fn frontier_round_robin_skips_empty_domain() {
        let f = Frontier::new();
        f.push(ScoredUrl::with_domain("https://a.com/1".to_string(), 0.9, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://b.com/1".to_string(), 0.5, "b.com".to_string()));
        f.push(ScoredUrl::with_domain("https://a.com/2".to_string(), 0.8, "a.com".to_string()));

        // a → b → a（b 空后跳过）
        let pop1 = f.pop().unwrap();
        assert_eq!(pop1.domain, "a.com");
        let pop2 = f.pop().unwrap();
        assert_eq!(pop2.domain, "b.com");
        // b.com 已空，cursor 在 b.com+1，但 b.com 已空 → 跳到 a.com
        let pop3 = f.pop().unwrap();
        assert_eq!(pop3.domain, "a.com");
        assert!(f.is_empty());
    }

    #[test]
    fn frontier_round_robin_no_starvation() {
        // 验证：即使 a.com 有大量高分 URL，b.com 的低分 URL 也能公平出队
        let f = Frontier::new();
        // a.com 有 5 个 score=0.9 的 URL
        for i in 0..5 {
            f.push(ScoredUrl::with_domain(
                format!("https://a.com/{i}"),
                0.9,
                "a.com".to_string(),
            ));
        }
        // b.com 有 1 个 score=0.1 的 URL
        f.push(ScoredUrl::with_domain("https://b.com/1".to_string(), 0.1, "b.com".to_string()));

        // 第一次出队 a.com，第二次出队 b.com（不会被 a.com 饥饿）
        let pop1 = f.pop().unwrap();
        assert_eq!(pop1.domain, "a.com");
        let pop2 = f.pop().unwrap();
        assert_eq!(pop2.domain, "b.com"); // b.com 没有被饥饿
    }

    // ============ Frontier 边界场景 ============

    #[test]
    fn frontier_empty_pop_returns_none() {
        let f = Frontier::new();
        assert!(f.pop().is_none());
    }

    #[test]
    fn frontier_all_domains_empty_pop_returns_none() {
        let f = Frontier::new();
        f.push(ScoredUrl::with_domain("https://a.com/1".to_string(), 0.5, "a.com".to_string()));
        f.pop();
        // 所有域名已空
        assert!(f.pop().is_none());
        assert_eq!(f.domain_count(), 1); // 空堆域名仍在
        assert_eq!(f.domain_len("a.com"), 0);
    }

    #[test]
    fn frontier_domain_len_nonexistent_returns_zero() {
        let f = Frontier::new();
        f.push(ScoredUrl::with_domain("https://a.com/1".to_string(), 0.5, "a.com".to_string()));
        assert_eq!(f.domain_len("a.com"), 1);
        assert_eq!(f.domain_len("b.com"), 0);
        assert_eq!(f.domain_len("nonexistent"), 0);
    }

    #[test]
    fn frontier_mixed_scores_and_domains() {
        // 混合场景：多域名、多分数、域内排序 + 域间 round-robin
        let f = Frontier::new();
        f.push(ScoredUrl::with_domain("https://a.com/low".to_string(), 0.1, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://b.com/high".to_string(), 0.9, "b.com".to_string()));
        f.push(ScoredUrl::with_domain("https://a.com/high".to_string(), 0.8, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://b.com/low".to_string(), 0.2, "b.com".to_string()));

        // round-robin: a.com(high=0.8) → b.com(high=0.9) → a.com(low=0.1) → b.com(low=0.2)
        let pop1 = f.pop().unwrap();
        assert_eq!(pop1.domain, "a.com");
        assert_eq!(pop1.url, "https://a.com/high");

        let pop2 = f.pop().unwrap();
        assert_eq!(pop2.domain, "b.com");
        assert_eq!(pop2.url, "https://b.com/high");

        let pop3 = f.pop().unwrap();
        assert_eq!(pop3.domain, "a.com");
        assert_eq!(pop3.url, "https://a.com/low");

        let pop4 = f.pop().unwrap();
        assert_eq!(pop4.domain, "b.com");
        assert_eq!(pop4.url, "https://b.com/low");

        assert!(f.is_empty());
    }

    #[test]
    fn frontier_same_score_fifo_within_domain() {
        // 同分数的 URL 在 BinaryHeap 中顺序不保证（同分时 heap 内部顺序不定）
        // 但都应能被弹出
        let f = Frontier::new();
        f.push(ScoredUrl::with_domain("https://a.com/1".to_string(), 0.5, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://a.com/2".to_string(), 0.5, "a.com".to_string()));
        f.push(ScoredUrl::with_domain("https://a.com/3".to_string(), 0.5, "a.com".to_string()));

        let mut urls = Vec::new();
        while let Some(s) = f.pop() {
            urls.push(s.url);
        }
        assert_eq!(urls.len(), 3);
        assert!(urls.contains(&"https://a.com/1".to_string()));
        assert!(urls.contains(&"https://a.com/2".to_string()));
        assert!(urls.contains(&"https://a.com/3".to_string()));
    }
}
