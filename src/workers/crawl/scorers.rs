// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! URL 评分器实现（T064，R-frontier-003）
//!
//! 参考 crawl4ai `deep_crawling/scorers.py`，提供三个具体 scorer：
//!
//! - [`KeywordRelevanceScorer`]：基于关键词列表评分（关键词命中率）
//! - [`PathDepthScorer`]：基于 URL path 深度评分（浅路径高分，hub 页面优先）
//! - [`CompositeScorer`]：加权聚合多个 scorer，归一化输出
//!
//! 三者实现 [`crate::workers::crawl::UrlScorer`] trait，
//! 经 [`CompositeScorer`] 聚合后供 Frontier（T065）排序出队优先级。

use url::Url;

use super::{ScoringContext, UrlScorer};

// =============================================================================
// KeywordRelevanceScorer
// =============================================================================

/// 关键词相关性评分器（T064，R-frontier-003）
///
/// 基于 [`ScoringContext::keywords`] 关键词列表，统计在 URL 中命中的比例。
/// 命中判定为大小写不敏感的子串包含（`url.to_lowercase().contains(keyword)`）。
///
/// # 评分公式
///
/// - `keywords` 为空 → 返回 `0.5`（中性，无法判定相关性，符合 trait 文档约定）
/// - `keywords` 非空 → `matched_count / total_count`，归一化到 `[0.0, 1.0]`
///   - 全部命中 → `1.0`
///   - 部分命中 → `matched / total`
///   - 全不命中 → `0.0`
///
/// # 设计动机
///
/// crawl4ai `KeywordRelevanceScorer` 用关键词命中数衡量 URL 与爬取目标的相关性。
/// 关键词通常由 CrawlConfigDto 的 `keywords` 字段或 LLM 查询扩展生成。
/// 高相关性的 URL 应优先出队，避免队列被无关页面占满。
///
/// # 示例
///
/// ```ignore
/// use crate::workers::crawl::{KeywordRelevanceScorer, ScoringContext, UrlScorer};
///
/// let scorer = KeywordRelevanceScorer::new();
/// let ctx = ScoringContext::new().with_keywords(vec!["rust".to_string(), "crawler".to_string()]);
/// assert_eq!(scorer.score("https://example.com/rust-crawler-guide", &ctx), 1.0);
/// assert_eq!(scorer.score("https://example.com/rust-tutorial", &ctx), 0.5);
/// assert_eq!(scorer.score("https://example.com/python-guide", &ctx), 0.0);
/// ```
#[derive(Debug, Clone, Default)]
pub struct KeywordRelevanceScorer;

impl KeywordRelevanceScorer {
    /// 构造关键词相关性评分器
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl UrlScorer for KeywordRelevanceScorer {
    fn score(&self, url: &str, context: &ScoringContext) -> f32 {
        // 过滤空关键词（空串是噪音，不计入 total）
        let keywords: Vec<&String> = context.keywords.iter().filter(|kw| !kw.is_empty()).collect();
        // 空关键词列表 → 中性 0.5（无法判定相关性）
        if keywords.is_empty() {
            return 0.5;
        }
        let url_lower = url.to_ascii_lowercase();
        let total = keywords.len();
        let matched = keywords
            .iter()
            .filter(|kw| url_lower.contains(&kw.to_ascii_lowercase()))
            .count();
        // matched / total ∈ [0.0, 1.0]
        matched as f32 / total as f32
    }
}

// =============================================================================
// PathDepthScorer
// =============================================================================

/// 路径深度评分器（T064，R-frontier-003）
///
/// 基于 URL path 的段数（depth）评分，**浅路径高分**（hub/index 页面优先出队），
/// 因为浅路径通常是导航页/列表页，爬取后能发现更多链接，提升爬取覆盖效率。
///
/// # 评分公式
///
/// `score = 1.0 / (1.0 + depth)`，其中 `depth` = path 非空段数。
///
/// | URL path | depth | score |
/// |----------|-------|-------|
/// | `/`      | 0     | 1.0   |
/// | `/blog`  | 1     | 0.5   |
/// | `/blog/2024` | 2 | 0.333 |
/// | `/blog/2024/01/post` | 3 | 0.25 |
///
/// 自然归一化到 `(0.0, 1.0]`，depth 越大分数越低但永不为 0。
///
/// # URL 解析失败
///
/// 解析失败时返回 `0.5`（中性），避免无效 URL 被极端排序。
///
/// # 设计动机
///
/// crawl4ai `PathDepthScorer` 鼓励先爬浅层页面，BFS 式扩展覆盖。
/// 与 [`KeywordRelevanceScorer`] 组合可平衡"相关性"与"发现广度"。
///
/// # 示例
///
/// ```ignore
/// use crate::workers::crawl::{PathDepthScorer, ScoringContext, UrlScorer};
///
/// let scorer = PathDepthScorer::new();
/// let ctx = ScoringContext::new();
/// assert_eq!(scorer.score("https://example.com/", &ctx), 1.0);
/// assert_eq!(scorer.score("https://example.com/blog", &ctx), 0.5);
/// ```
#[derive(Debug, Clone, Default)]
pub struct PathDepthScorer;

impl PathDepthScorer {
    /// 构造路径深度评分器
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// 计算 URL path 深度（非空段数）
    ///
    /// - `https://example.com/` → depth 0
    /// - `https://example.com/blog` → depth 1
    /// - `https://example.com/blog/2024/` → depth 2（trailing slash 不计）
    /// - `https://example.com/a/b/c` → depth 3
    fn path_depth(url_str: &str) -> Option<usize> {
        let parsed = Url::parse(url_str).ok()?;
        let depth = parsed
            .path_segments()?
            .filter(|seg| !seg.is_empty())
            .count();
        Some(depth)
    }
}

impl UrlScorer for PathDepthScorer {
    fn score(&self, url: &str, _context: &ScoringContext) -> f32 {
        match Self::path_depth(url) {
            Some(depth) => 1.0 / (1.0 + depth as f32),
            None => 0.5, // 解析失败 → 中性
        }
    }
}

// =============================================================================
// CompositeScorer
// =============================================================================

/// 组合评分器（T064，R-frontier-003）
///
/// 加权聚合多个 [`UrlScorer`]，输出归一化加权平均分数。
///
/// # 评分公式
///
/// `composite_score = Σ(scorer_i.score * weight_i) / Σ(weight_i)`
///
/// - 权重总和为 0 时返回 `0.5`（中性，无 scorer 贡献）
/// - 每个子 scorer 的分数已归一化到 `[0.0, 1.0]`，加权平均仍在该区间
///
/// # 线程安全
///
/// 内部用 `Vec<(Arc<dyn UrlScorer>, f32)>` 共享 scorer 实例，
/// `CompositeScorer` 自身是 `Clone`（仅 clone Arc 指针），可被多 worker 共享。
///
/// # 示例
///
/// ```ignore
/// use crate::workers::crawl::{
///     CompositeScorer, KeywordRelevanceScorer, PathDepthScorer,
///     ScoringContext, UrlScorer,
/// };
///
/// let scorer = CompositeScorer::new()
///     .with_scorer(KeywordRelevanceScorer::new(), 0.7)
///     .with_scorer(PathDepthScorer::new(), 0.3);
///
/// let ctx = ScoringContext::new().with_keywords(vec!["rust".to_string()]);
/// let score = scorer.score("https://example.com/rust", &ctx);
/// assert!((0.0..=1.0).contains(&score));
/// ```
#[derive(Clone, Default)]
pub struct CompositeScorer {
    /// 加权 scorer 列表：(scorer, weight)
    scorers: Vec<(std::sync::Arc<dyn UrlScorer>, f32)>,
}

impl CompositeScorer {
    /// 构造空的组合评分器（无 scorer 时 `score` 返回 0.5）
    #[must_use]
    pub fn new() -> Self {
        Self { scorers: Vec::new() }
    }

    /// 追加一个 scorer 及其权重，返回 `self`（builder 模式）
    ///
    /// # 参数
    ///
    /// - `scorer`: 实现 [`UrlScorer`] 的评分器
    /// - `weight`: 权重（建议 > 0，负值会导致加权平均语义异常）
    #[must_use]
    pub fn with_scorer(mut self, scorer: impl UrlScorer + 'static, weight: f32) -> Self {
        self.scorers.push((std::sync::Arc::new(scorer), weight));
        self
    }

    /// 追加一个已擦除类型的 scorer（用于动态拼装）
    #[must_use]
    pub fn with_shared_scorer(
        mut self,
        scorer: std::sync::Arc<dyn UrlScorer>,
        weight: f32,
    ) -> Self {
        self.scorers.push((scorer, weight));
        self
    }

    /// 当前 scorer 数量
    #[must_use]
    pub fn len(&self) -> usize {
        self.scorers.len()
    }

    /// 是否为空（空时 `score` 返回 0.5）
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scorers.is_empty()
    }
}

impl UrlScorer for CompositeScorer {
    fn score(&self, url: &str, context: &ScoringContext) -> f32 {
        if self.scorers.is_empty() {
            return 0.5;
        }
        let (weighted_sum, weight_sum) = self
            .scorers
            .iter()
            .fold((0.0_f32, 0.0_f32), |(ws, wsum), (scorer, w)| {
                let s = scorer.score(url, context);
                (ws + s * w, wsum + w)
            });
        // 权重总和为 0 → 中性 0.5（避免除零）
        if weight_sum == 0.0 {
            return 0.5;
        }
        let result = weighted_sum / weight_sum;
        // clamp 到 [0.0, 1.0] 防止浮点误差或负权重溢出
        result.clamp(0.0, 1.0)
    }
}

// 注：UrlScorer trait + ScoringContext 在 mod.rs 定义，此处仅 re-export 具体 scorer
// 使用时通过 `crate::workers::crawl::UrlScorer` 访问 trait

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::crawl::ScoringContext;

    // ============ KeywordRelevanceScorer ============

    #[test]
    fn keyword_relevance_all_matched_returns_one() {
        let scorer = KeywordRelevanceScorer::new();
        let ctx = ScoringContext::new()
            .with_keywords(vec!["rust".to_string(), "crawler".to_string()]);
        assert_eq!(scorer.score("https://example.com/rust-crawler", &ctx), 1.0);
    }

    #[test]
    fn keyword_relevance_partial_match_returns_ratio() {
        let scorer = KeywordRelevanceScorer::new();
        let ctx = ScoringContext::new()
            .with_keywords(vec!["rust".to_string(), "crawler".to_string()]);
        // 仅 rust 命中 → 1/2 = 0.5
        assert_eq!(scorer.score("https://example.com/rust-tutorial", &ctx), 0.5);
    }

    #[test]
    fn keyword_relevance_no_match_returns_zero() {
        let scorer = KeywordRelevanceScorer::new();
        let ctx = ScoringContext::new()
            .with_keywords(vec!["rust".to_string(), "crawler".to_string()]);
        assert_eq!(scorer.score("https://example.com/python-guide", &ctx), 0.0);
    }

    #[test]
    fn keyword_relevance_empty_keywords_returns_neutral() {
        let scorer = KeywordRelevanceScorer::new();
        let ctx = ScoringContext::new(); // 无关键词
        assert_eq!(scorer.score("https://example.com/anything", &ctx), 0.5);
    }

    #[test]
    fn keyword_relevance_case_insensitive() {
        let scorer = KeywordRelevanceScorer::new();
        let ctx = ScoringContext::new().with_keywords(vec!["Rust".to_string()]);
        // URL 小写包含 "rust"
        assert_eq!(scorer.score("https://example.com/RUST-guide", &ctx), 1.0);
        assert_eq!(scorer.score("https://example.com/rust-guide", &ctx), 1.0);
    }

    #[test]
    fn keyword_relevance_single_keyword() {
        let scorer = KeywordRelevanceScorer::new();
        let ctx = ScoringContext::new().with_keywords(vec!["blog".to_string()]);
        assert_eq!(scorer.score("https://example.com/blog/post", &ctx), 1.0);
        assert_eq!(scorer.score("https://example.com/news", &ctx), 0.0);
    }

    #[test]
    fn keyword_relevance_empty_string_keyword_ignored() {
        let scorer = KeywordRelevanceScorer::new();
        // 空字符串关键词应被忽略（不计入 total）
        let ctx = ScoringContext::new()
            .with_keywords(vec!["".to_string(), "rust".to_string()]);
        // 空关键词被忽略 → total=1，rust 命中 → 1.0
        assert_eq!(scorer.score("https://example.com/rust", &ctx), 1.0);
        // rust 不命中 → 0.0
        assert_eq!(scorer.score("https://example.com/python", &ctx), 0.0);
    }

    #[test]
    fn keyword_relevance_keyword_in_domain() {
        let scorer = KeywordRelevanceScorer::new();
        let ctx = ScoringContext::new().with_keywords(vec!["example".to_string()]);
        // 关键词出现在域名中也算命中
        assert_eq!(scorer.score("https://example.com/page", &ctx), 1.0);
    }

    #[test]
    fn keyword_relevance_keyword_in_query() {
        let scorer = KeywordRelevanceScorer::new();
        let ctx = ScoringContext::new().with_keywords(vec!["search".to_string()]);
        // 关键词出现在 query 中也算命中
        assert_eq!(scorer.score("https://example.com/?q=search", &ctx), 1.0);
    }

    #[test]
    fn keyword_relevance_three_keywords_ratio() {
        let scorer = KeywordRelevanceScorer::new();
        let ctx = ScoringContext::new().with_keywords(vec![
            "rust".to_string(),
            "crawler".to_string(),
            "async".to_string(),
        ]);
        // 2/3 命中
        let score = scorer.score("https://example.com/rust-async-guide", &ctx);
        assert!((score - 2.0 / 3.0).abs() < 1e-6);
    }

    // ============ PathDepthScorer ============

    #[test]
    fn path_depth_root_returns_one() {
        let scorer = PathDepthScorer::new();
        let ctx = ScoringContext::new();
        assert_eq!(scorer.score("https://example.com/", &ctx), 1.0);
    }

    #[test]
    fn path_depth_no_path_returns_one() {
        let scorer = PathDepthScorer::new();
        let ctx = ScoringContext::new();
        // 无 path（如 https://example.com）→ depth 0
        assert_eq!(scorer.score("https://example.com", &ctx), 1.0);
    }

    #[test]
    fn path_depth_single_segment_returns_half() {
        let scorer = PathDepthScorer::new();
        let ctx = ScoringContext::new();
        assert_eq!(scorer.score("https://example.com/blog", &ctx), 0.5);
    }

    #[test]
    fn path_depth_two_segments() {
        let scorer = PathDepthScorer::new();
        let ctx = ScoringContext::new();
        // depth 2 → 1/3 ≈ 0.333
        let score = scorer.score("https://example.com/blog/2024", &ctx);
        assert!((score - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn path_depth_three_segments() {
        let scorer = PathDepthScorer::new();
        let ctx = ScoringContext::new();
        // depth 3 → 1/4 = 0.25
        let score = scorer.score("https://example.com/blog/2024/post", &ctx);
        assert!((score - 0.25).abs() < 1e-6);
    }

    #[test]
    fn path_depth_trailing_slash_not_counted() {
        let scorer = PathDepthScorer::new();
        let ctx = ScoringContext::new();
        // /blog/ → depth 1（trailing slash 不计）
        assert_eq!(scorer.score("https://example.com/blog/", &ctx), 0.5);
        // /blog/2024/ → depth 2
        let score = scorer.score("https://example.com/blog/2024/", &ctx);
        assert!((score - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn path_depth_shallow_higher_than_deep() {
        let scorer = PathDepthScorer::new();
        let ctx = ScoringContext::new();
        let shallow = scorer.score("https://example.com/blog", &ctx);
        let deep = scorer.score("https://example.com/blog/2024/01/post", &ctx);
        assert!(shallow > deep);
    }

    #[test]
    fn path_depth_invalid_url_returns_neutral() {
        let scorer = PathDepthScorer::new();
        let ctx = ScoringContext::new();
        // 无效 URL → 0.5（中性）
        assert_eq!(scorer.score("not a url", &ctx), 0.5);
        assert_eq!(scorer.score("javascript:void(0)", &ctx), 0.5);
    }

    #[test]
    fn path_depth_with_query_and_fragment() {
        let scorer = PathDepthScorer::new();
        let ctx = ScoringContext::new();
        // query 和 fragment 不影响 depth 计算
        assert_eq!(scorer.score("https://example.com/blog?q=1#top", &ctx), 0.5);
    }

    #[test]
    fn path_depth_empty_segments_not_counted() {
        let scorer = PathDepthScorer::new();
        let ctx = ScoringContext::new();
        // //a//b// → 非空段 a, b → depth 2
        let score = scorer.score("https://example.com//a//b//", &ctx);
        assert!((score - 1.0 / 3.0).abs() < 1e-6);
    }

    // ============ CompositeScorer ============

    #[test]
    fn composite_empty_returns_neutral() {
        let scorer = CompositeScorer::new();
        let ctx = ScoringContext::new();
        assert!(scorer.is_empty());
        assert_eq!(scorer.len(), 0);
        assert_eq!(scorer.score("https://example.com/anything", &ctx), 0.5);
    }

    #[test]
    fn composite_single_scorer_returns_its_score() {
        let scorer = CompositeScorer::new()
            .with_scorer(PathDepthScorer::new(), 1.0);
        let ctx = ScoringContext::new();
        // 单 scorer 权重 1.0 → 等于该 scorer 的分数
        assert_eq!(scorer.score("https://example.com/", &ctx), 1.0);
        assert_eq!(scorer.score("https://example.com/blog", &ctx), 0.5);
    }

    #[test]
    fn composite_weighted_average() {
        // KeywordRelevanceScorer: rust-crawler → 1.0 (两词全命中)
        // PathDepthScorer: /rust-crawler → depth 1 → 0.5
        // 权重 0.7 / 0.3 → 0.7*1.0 + 0.3*0.5 = 0.85
        let scorer = CompositeScorer::new()
            .with_scorer(KeywordRelevanceScorer::new(), 0.7)
            .with_scorer(PathDepthScorer::new(), 0.3);
        let ctx = ScoringContext::new()
            .with_keywords(vec!["rust".to_string(), "crawler".to_string()]);
        let score = scorer.score("https://example.com/rust-crawler", &ctx);
        assert!((score - 0.85).abs() < 1e-6);
    }

    #[test]
    fn composite_multiple_scorers_normalized() {
        let scorer = CompositeScorer::new()
            .with_scorer(KeywordRelevanceScorer::new(), 2.0)
            .with_scorer(PathDepthScorer::new(), 1.0);
        let ctx = ScoringContext::new()
            .with_keywords(vec!["rust".to_string()]);
        // Keyword: rust 命中 → 1.0; PathDepth: /rust → depth 1 → 0.5
        // 加权: (2*1.0 + 1*0.5) / 3 = 2.5/3 ≈ 0.833
        let score = scorer.score("https://example.com/rust", &ctx);
        assert!((score - 5.0 / 6.0).abs() < 1e-6);
    }

    #[test]
    fn composite_zero_weight_returns_neutral() {
        let scorer = CompositeScorer::new()
            .with_scorer(PathDepthScorer::new(), 0.0);
        let ctx = ScoringContext::new();
        // 权重总和为 0 → 0.5
        assert_eq!(scorer.score("https://example.com/", &ctx), 0.5);
    }

    #[test]
    fn composite_score_always_in_range() {
        let scorer = CompositeScorer::new()
            .with_scorer(KeywordRelevanceScorer::new(), 0.5)
            .with_scorer(PathDepthScorer::new(), 0.5);
        let ctx = ScoringContext::new()
            .with_keywords(vec!["rust".to_string()]);
        // 多个 URL 的分数都应在 [0.0, 1.0]
        for url in &[
            "https://example.com/",
            "https://example.com/rust",
            "https://example.com/deep/path/to/page",
            "https://example.com/unrelated",
        ] {
            let s = scorer.score(url, &ctx);
            assert!((0.0..=1.0).contains(&s), "score {s} out of range for {url}");
        }
    }

    #[test]
    fn composite_len_and_is_empty() {
        let empty = CompositeScorer::new();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let scorer = CompositeScorer::new()
            .with_scorer(KeywordRelevanceScorer::new(), 1.0)
            .with_scorer(PathDepthScorer::new(), 1.0);
        assert!(!scorer.is_empty());
        assert_eq!(scorer.len(), 2);
    }

    #[test]
    fn composite_clone_shares_scorers() {
        let scorer = CompositeScorer::new()
            .with_scorer(PathDepthScorer::new(), 1.0);
        let cloned = scorer.clone();
        let ctx = ScoringContext::new();
        // clone 后行为一致
        assert_eq!(cloned.score("https://example.com/", &ctx), 1.0);
        assert_eq!(scorer.score("https://example.com/", &ctx), 1.0);
    }

    #[test]
    fn composite_with_shared_scorer_arc() {
        let shared: std::sync::Arc<dyn UrlScorer> =
            std::sync::Arc::new(PathDepthScorer::new());
        let scorer = CompositeScorer::new().with_shared_scorer(shared, 1.0);
        let ctx = ScoringContext::new();
        assert_eq!(scorer.score("https://example.com/", &ctx), 1.0);
    }

    #[test]
    fn composite_three_scorers_combined() {
        // 三个 scorer 聚合
        let scorer = CompositeScorer::new()
            .with_scorer(KeywordRelevanceScorer::new(), 0.5)
            .with_scorer(PathDepthScorer::new(), 0.3)
            .with_scorer(KeywordRelevanceScorer::new(), 0.2);
        let ctx = ScoringContext::new()
            .with_keywords(vec!["rust".to_string(), "blog".to_string()]);
        // URL: https://example.com/rust → Keyword 0.5 (1/2), PathDepth 0.5
        // 加权: (0.5*0.5 + 0.5*0.3 + 0.5*0.2) / 1.0 = 0.25+0.15+0.1 = 0.5
        let score = scorer.score("https://example.com/rust", &ctx);
        assert!((score - 0.5).abs() < 1e-6);
    }

    #[test]
    fn composite_negative_weight_clamped() {
        // 负权重会导致加权平均异常，clamp 保证结果在 [0.0, 1.0]
        let scorer = CompositeScorer::new()
            .with_scorer(PathDepthScorer::new(), -1.0)
            .with_scorer(PathDepthScorer::new(), 1.0);
        let ctx = ScoringContext::new();
        // 两个 PathDepth 对 / 返回 1.0
        // (-1*1.0 + 1*1.0) / 0 = 除零 → 返回 0.5
        assert_eq!(scorer.score("https://example.com/", &ctx), 0.5);
    }
}
