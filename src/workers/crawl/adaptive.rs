// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 自适应爬取策略与停止条件（T067，R-frontier-004）
//!
//! 参考 crawl4ai `adaptive_crawler.py`，提供：
//!
//! - [`AdaptiveStrategy`]：综合评估爬取进度（BM25 相关性 / 覆盖率 / 饱和度）
//! - [`StopCondition`]：可配置停止条件（最大页数 / 置信度阈值 / 饱和度 / 无待处理链接）
//! - [`StopReason`]：停止原因记录（便于日志与决策追溯）
//!
//! # BM25 简化说明
//!
//! 设计文档 §16 要求"BM25 相关性用现有 `relevance_scorer.rs`（TF-IDF）扩展"。
//! 当前阶段 `AdaptiveContext` 仅含 URL 列表（无正文），故 BM25 降级为
//! **URL 关键词命中率**（复用 [`scorers::KeywordRelevanceScorer`] 逻辑）。
//! 后续正文提取（T049）接入后可升级为正文 TF-IDF 评分。

use super::scorers::KeywordRelevanceScorer;
use super::{ScoringContext, UrlScorer};

// =============================================================================
// StrategyResult
// =============================================================================

/// 自适应策略评估结果（T067，R-frontier-004）
///
/// 由 [`AdaptiveStrategy::evaluate`] 返回，包含三个维度的爬取进度指标。
///
/// # 字段说明
///
/// - `confidence`: BM25 相关性。crawled URLs 的平均关键词命中率 `[0, 1]`。
///   高 = 已爬取页面与目标关键词高度相关。
/// - `coverage`: 关键词覆盖率。已发现的独立关键词占目标关键词总数的比例 `[0, 1]`。
///   高 = 已覆盖大部分目标主题。
/// - `saturation`: 饱和度（新内容发现率）。`new_links / total_links`，`[0, 1]`。
///   低 = 新链接发现率下降，爬取趋于饱和。
#[derive(Debug, Clone, Default)]
pub struct StrategyResult {
    /// BM25 相关性（URL 关键词命中率均值）`[0, 1]`
    pub confidence: f32,
    /// 关键词覆盖率 `[0, 1]`
    pub coverage: f32,
    /// 饱和度（新内容发现率）`[0, 1]`，低=饱和
    pub saturation: f32,
}

// =============================================================================
// AdaptiveContext
// =============================================================================

/// 自适应评估上下文（T067）
///
/// 提供 [`AdaptiveStrategy`] 评估所需的输入数据。
#[derive(Debug, Clone, Default)]
pub struct AdaptiveContext {
    /// 已爬取的 URL 列表（已归一化）
    pub crawled_urls: Vec<String>,
    /// 目标关键词列表
    pub keywords: Vec<String>,
    /// 累计发现的总链接数（含重复）
    pub total_links_discovered: usize,
    /// 其中新链接数（去重后首次发现）
    pub new_links_discovered: usize,
}

impl AdaptiveContext {
    /// 构造空上下文
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置已爬取 URL 列表
    #[must_use]
    pub fn with_crawled_urls(mut self, urls: Vec<String>) -> Self {
        self.crawled_urls = urls;
        self
    }

    /// 设置关键词列表
    #[must_use]
    pub fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = keywords;
        self
    }

    /// 设置链接发现统计
    #[must_use]
    pub fn with_link_stats(mut self, total: usize, new: usize) -> Self {
        self.total_links_discovered = total;
        self.new_links_discovered = new;
        self
    }
}

// =============================================================================
// AdaptiveStrategy
// =============================================================================

/// 自适应策略（T067，R-frontier-004）
///
/// 参考 crawl4ai `adaptive_crawler.py`，综合评估爬取进度。
///
/// # 评估维度
///
/// 1. **BM25 相关性**（`confidence`）：crawled URLs 的平均关键词命中率。
///    当前用 URL 关键词匹配（简化 BM25），后续接入正文后升级为 TF-IDF。
/// 2. **覆盖率**（`coverage`）：已发现关键词占目标关键词的比例。
/// 3. **饱和度**（`saturation`）：新链接 / 总链接。低值表示新内容发现率下降。
///
/// # 示例
///
/// ```ignore
/// use crate::workers::crawl::adaptive::{AdaptiveContext, AdaptiveStrategy};
///
/// let ctx = AdaptiveContext::new()
///     .with_crawled_urls(vec!["https://example.com/rust".to_string()])
///     .with_keywords(vec!["rust".to_string(), "crawler".to_string()])
///     .with_link_stats(10, 3);
///
/// let result = AdaptiveStrategy::evaluate(&ctx);
/// assert!(result.confidence > 0.0);
/// assert!(result.coverage > 0.0);
/// assert!((result.saturation - 0.3).abs() < 1e-6);
/// ```
#[derive(Debug, Clone, Default)]
pub struct AdaptiveStrategy;

impl AdaptiveStrategy {
    /// 评估爬取进度，返回三维指标
    ///
    /// # 参数
    ///
    /// - `context`: 已爬取 URL + 关键词 + 链接统计
    ///
    /// # 返回
    ///
    /// [`StrategyResult`] 含 confidence / coverage / saturation
    #[must_use]
    pub fn evaluate(context: &AdaptiveContext) -> StrategyResult {
        let confidence = Self::compute_confidence(&context.crawled_urls, &context.keywords);
        let coverage = Self::compute_coverage(&context.crawled_urls, &context.keywords);
        let saturation =
            Self::compute_saturation(context.total_links_discovered, context.new_links_discovered);
        StrategyResult {
            confidence,
            coverage,
            saturation,
        }
    }

    /// 计算 BM25 相关性（URL 关键词命中率均值）
    ///
    /// - 空 URL 列表 → 0.0（无数据）
    /// - 空关键词列表 → 0.5（中性，无法判定）
    /// - 否则 → 各 URL 的 [`KeywordRelevanceScorer`] 分数均值
    fn compute_confidence(urls: &[String], keywords: &[String]) -> f32 {
        if urls.is_empty() {
            return 0.0;
        }
        let scorer = KeywordRelevanceScorer::new();
        let scoring_ctx = ScoringContext::new().with_keywords(keywords.to_vec());
        let sum: f32 = urls.iter().map(|url| scorer.score(url, &scoring_ctx)).sum();
        sum / urls.len() as f32
    }

    /// 计算关键词覆盖率
    ///
    /// 统计在所有 crawled URLs 中至少命中一次的关键词比例。
    /// - 空关键词 → 0.0
    /// - 否则 → 命中关键词数 / 总关键词数
    fn compute_coverage(urls: &[String], keywords: &[String]) -> f32 {
        if keywords.is_empty() {
            return 0.0;
        }
        let keywords_filtered: Vec<&String> = keywords.iter().filter(|k| !k.is_empty()).collect();
        if keywords_filtered.is_empty() {
            return 0.0;
        }
        let all_urls_lower = urls
            .iter()
            .map(|u| u.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        let matched = keywords_filtered
            .iter()
            .filter(|kw| all_urls_lower.contains(&kw.to_ascii_lowercase()))
            .count();
        matched as f32 / keywords_filtered.len() as f32
    }

    /// 计算饱和度（新内容发现率）
    ///
    /// - total=0 → 1.0（无数据，默认未饱和）
    /// - 否则 → new / total
    fn compute_saturation(total: usize, new: usize) -> f32 {
        if total == 0 {
            return 1.0;
        }
        // new 可能 > total（统计口径不同），clamp 到 [0, 1]
        (new as f32 / total as f32).clamp(0.0, 1.0)
    }
}

// =============================================================================
// StopReason
// =============================================================================

/// 停止原因（T067，R-frontier-004）
///
/// 由 [`StopCondition::should_stop`] 返回，记录停止的具体原因与阈值，
/// 便于日志输出与决策追溯（规则 12：失败/停止必须显性化）。
#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// 已爬取页数达到上限
    MaxPagesReached {
        /// 已爬取页数
        crawled: usize,
        /// 配置的最大页数
        max: usize,
    },
    /// 平均置信度达到阈值
    ConfidenceReached {
        /// 当前置信度
        confidence: f32,
        /// 配置的阈值
        threshold: f32,
    },
    /// 饱和度低于阈值（新内容发现率下降）
    SaturationReached {
        /// 当前饱和度
        saturation: f32,
        /// 配置的阈值
        threshold: f32,
    },
    /// 无待处理链接（Frontier 为空）
    NoPendingLinks,
    /// KG 覆盖率达到阈值（Chao1 估计）
    CoverageReached {
        /// 当前覆盖率估计
        coverage: f64,
        /// 配置的阈值
        threshold: f64,
    },
}

impl StopReason {
    /// 人类可读的停止原因描述
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::MaxPagesReached { crawled, max } => {
                format!("max pages reached ({crawled}/{max})")
            }
            Self::ConfidenceReached {
                confidence,
                threshold,
            } => {
                format!("confidence threshold reached ({confidence:.3} >= {threshold:.3})")
            }
            Self::SaturationReached {
                saturation,
                threshold,
            } => {
                format!("saturation threshold reached ({saturation:.3} < {threshold:.3})")
            }
            Self::NoPendingLinks => "no pending links".to_string(),
            Self::CoverageReached { coverage, threshold } => {
                format!("coverage threshold reached ({coverage:.3} >= {threshold:.3})")
            }
        }
    }
}

// =============================================================================
// CrawlStats
// =============================================================================

/// 爬取统计（T067）
///
/// 提供 [`StopCondition::should_stop`] 决策所需的运行时数据。
#[derive(Debug, Clone, Default)]
pub struct CrawlStats {
    /// 已爬取页数
    pub pages_crawled: usize,
    /// 待处理链接数（Frontier 中剩余 URL 数）
    pub pending_links: usize,
    /// 自适应策略评估结果
    pub result: StrategyResult,
    /// KG 覆盖率估计（Chao1，0.0-1.0，None = 未计算）
    pub kg_coverage: Option<f64>,
}

impl CrawlStats {
    /// 构造空统计
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置已爬取页数
    #[must_use]
    pub fn with_pages(mut self, pages: usize) -> Self {
        self.pages_crawled = pages;
        self
    }

    /// 设置待处理链接数
    #[must_use]
    pub fn with_pending(mut self, pending: usize) -> Self {
        self.pending_links = pending;
        self
    }

    /// 设置策略评估结果
    #[must_use]
    pub fn with_result(mut self, result: StrategyResult) -> Self {
        self.result = result;
        self
    }

    /// 设置 KG 覆盖率估计
    #[must_use]
    pub fn with_kg_coverage(mut self, coverage: f64) -> Self {
        self.kg_coverage = Some(coverage);
        self
    }
}

// =============================================================================
// StopCondition
// =============================================================================

/// 停止条件（T067，R-frontier-004）
///
/// 可配置多个停止条件，任一满足即停止。检查优先级：
///
/// 1. **NoPendingLinks**：待处理链接为 0（立即停止）
/// 2. **MaxPagesReached**：已爬取页数 ≥ `max_pages`
/// 3. **ConfidenceReached**：置信度 ≥ `min_confidence`
/// 4. **SaturationReached**：饱和度 < `saturation_threshold`（新内容发现率过低）
///
/// # 示例
///
/// ```ignore
/// use crate::workers::crawl::adaptive::{
///     CrawlStats, StopCondition, StrategyResult,
/// };
///
/// let condition = StopCondition::new()
///     .with_max_pages(100)
///     .with_min_confidence(0.8)
///     .with_saturation_threshold(0.1);
///
/// let stats = CrawlStats::new()
///     .with_pages(50)
///     .with_pending(10)
///     .with_result(StrategyResult { confidence: 0.9, coverage: 0.8, saturation: 0.3 });
///
/// // confidence >= 0.8 → 停止
/// assert!(condition.should_stop(&stats).is_some());
/// ```
#[derive(Debug, Clone, Default)]
pub struct StopCondition {
    /// 最大爬取页数（None = 不限制）
    max_pages: Option<usize>,
    /// 最小置信度阈值（None = 不限制）
    min_confidence: Option<f32>,
    /// 饱和度阈值，低于此值停止（None = 不限制）
    saturation_threshold: Option<f32>,
    /// KG 覆盖率阈值，达到即停止（None = 不限制）
    min_coverage: Option<f64>,
}

impl StopCondition {
    /// 构造空停止条件（无任何限制，永不停止）
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置最大爬取页数
    #[must_use]
    pub fn with_max_pages(mut self, max: usize) -> Self {
        self.max_pages = Some(max);
        self
    }

    /// 设置最小置信度阈值（达到即停止）
    #[must_use]
    pub fn with_min_confidence(mut self, threshold: f32) -> Self {
        self.min_confidence = Some(threshold.clamp(0.0, 1.0));
        self
    }

    /// 设置饱和度阈值（低于即停止）
    #[must_use]
    pub fn with_saturation_threshold(mut self, threshold: f32) -> Self {
        self.saturation_threshold = Some(threshold.clamp(0.0, 1.0));
        self
    }

    /// 设置 KG 覆盖率阈值（达到即停止）
    #[must_use]
    pub fn with_min_coverage(mut self, threshold: f64) -> Self {
        self.min_coverage = Some(threshold.clamp(0.0, 1.0));
        self
    }

    /// 检查是否应停止爬取
    ///
    /// 按优先级依次检查各条件，返回第一个满足的 [`StopReason`]。
    /// 全部不满足时返回 `None`（继续爬取）。
    #[must_use]
    pub fn should_stop(&self, stats: &CrawlStats) -> Option<StopReason> {
        // 1. 无待处理链接 → 立即停止
        if stats.pending_links == 0 {
            return Some(StopReason::NoPendingLinks);
        }

        // 2. 最大页数
        if let Some(max) = self.max_pages {
            if stats.pages_crawled >= max {
                return Some(StopReason::MaxPagesReached {
                    crawled: stats.pages_crawled,
                    max,
                });
            }
        }

        // 3. 置信度阈值
        if let Some(threshold) = self.min_confidence {
            if stats.result.confidence >= threshold {
                return Some(StopReason::ConfidenceReached {
                    confidence: stats.result.confidence,
                    threshold,
                });
            }
        }

        // 4. 饱和度阈值（低于阈值 = 新内容发现率过低）
        if let Some(threshold) = self.saturation_threshold {
            if stats.result.saturation < threshold {
                return Some(StopReason::SaturationReached {
                    saturation: stats.result.saturation,
                    threshold,
                });
            }
        }

        // 5. KG 覆盖率阈值（达到阈值 = 知识图谱已充分覆盖）
        if let Some(threshold) = self.min_coverage {
            if let Some(coverage) = stats.kg_coverage {
                if coverage >= threshold {
                    return Some(StopReason::CoverageReached {
                        coverage,
                        threshold,
                    });
                }
            }
        }

        None
    }
}

// =============================================================================
// DRL 配置（T087）
// =============================================================================

/// DRL 策略配置
///
/// 控制是否启用 DRL 策略替代启发式规则调整并发度和 URL 优先级。
#[derive(Debug, Clone)]
pub struct DrlConfig {
    /// 是否启用 DRL 策略（默认 false）
    pub drl_policy_enabled: bool,
    /// ONNX 模型路径（可选，为空时使用启发式策略）
    pub model_path: Option<String>,
}

impl Default for DrlConfig {
    fn default() -> Self {
        Self {
            drl_policy_enabled: false,
            model_path: None,
        }
    }
}

impl DrlConfig {
    /// 创建默认配置（DRL 关闭）
    pub fn new() -> Self {
        Self::default()
    }

    /// 启用 DRL 策略
    #[must_use]
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.drl_policy_enabled = enabled;
        self
    }

    /// 设置 ONNX 模型路径
    #[must_use]
    pub fn with_model_path(mut self, path: impl Into<String>) -> Self {
        self.model_path = Some(path.into());
        self
    }

    /// 创建 DrlPolicy 实例
    pub fn build_policy(&self) -> super::drl_policy::DrlPolicy {
        if self.drl_policy_enabled {
            // 如果有模型路径，加载 ONNX 模型
            // 当前实现使用启发式策略作为退化
            super::drl_policy::DrlPolicy::heuristic(true)
        } else {
            super::drl_policy::DrlPolicy::heuristic(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============ AdaptiveStrategy ============

    #[test]
    fn adaptive_confidence_empty_urls_returns_zero() {
        let ctx = AdaptiveContext::new().with_keywords(vec!["rust".to_string()]);
        let result = AdaptiveStrategy::evaluate(&ctx);
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn adaptive_confidence_empty_keywords_returns_neutral() {
        let ctx =
            AdaptiveContext::new().with_crawled_urls(vec!["https://example.com/page".to_string()]);
        let result = AdaptiveStrategy::evaluate(&ctx);
        // 空关键词 → KeywordRelevanceScorer 返回 0.5
        assert!((result.confidence - 0.5).abs() < 1e-6);
    }

    #[test]
    fn adaptive_confidence_all_urls_match_keywords() {
        let ctx = AdaptiveContext::new()
            .with_crawled_urls(vec![
                "https://example.com/rust-crawler".to_string(),
                "https://example.com/rust-async".to_string(),
            ])
            .with_keywords(vec!["rust".to_string()]);
        let result = AdaptiveStrategy::evaluate(&ctx);
        // 每个 URL 都含 "rust" → confidence = 1.0
        assert!((result.confidence - 1.0).abs() < 1e-6);
    }

    #[test]
    fn adaptive_confidence_partial_match() {
        let ctx = AdaptiveContext::new()
            .with_crawled_urls(vec![
                "https://example.com/rust-guide".to_string(),   // 命中
                "https://example.com/python-guide".to_string(), // 未命中
            ])
            .with_keywords(vec!["rust".to_string()]);
        let result = AdaptiveStrategy::evaluate(&ctx);
        // 平均: (1.0 + 0.0) / 2 = 0.5
        assert!((result.confidence - 0.5).abs() < 1e-6);
    }

    #[test]
    fn adaptive_coverage_all_keywords_found() {
        let ctx = AdaptiveContext::new()
            .with_crawled_urls(vec!["https://example.com/rust-crawler".to_string()])
            .with_keywords(vec!["rust".to_string(), "crawler".to_string()]);
        let result = AdaptiveStrategy::evaluate(&ctx);
        assert!((result.coverage - 1.0).abs() < 1e-6);
    }

    #[test]
    fn adaptive_coverage_partial_keywords_found() {
        let ctx = AdaptiveContext::new()
            .with_crawled_urls(vec!["https://example.com/rust-guide".to_string()])
            .with_keywords(vec!["rust".to_string(), "crawler".to_string()]);
        let result = AdaptiveStrategy::evaluate(&ctx);
        // rust 命中, crawler 未命中 → 0.5
        assert!((result.coverage - 0.5).abs() < 1e-6);
    }

    #[test]
    fn adaptive_coverage_empty_keywords_returns_zero() {
        let ctx =
            AdaptiveContext::new().with_crawled_urls(vec!["https://example.com/page".to_string()]);
        let result = AdaptiveStrategy::evaluate(&ctx);
        assert_eq!(result.coverage, 0.0);
    }

    #[test]
    fn adaptive_saturation_all_new_returns_one() {
        let ctx = AdaptiveContext::new()
            .with_crawled_urls(vec!["https://example.com".to_string()])
            .with_link_stats(10, 10); // 全是新链接
        let result = AdaptiveStrategy::evaluate(&ctx);
        assert!((result.saturation - 1.0).abs() < 1e-6);
    }

    #[test]
    fn adaptive_saturation_half_new() {
        let ctx = AdaptiveContext::new().with_link_stats(10, 5);
        let result = AdaptiveStrategy::evaluate(&ctx);
        assert!((result.saturation - 0.5).abs() < 1e-6);
    }

    #[test]
    fn adaptive_saturation_no_new_returns_zero() {
        let ctx = AdaptiveContext::new().with_link_stats(10, 0);
        let result = AdaptiveStrategy::evaluate(&ctx);
        assert_eq!(result.saturation, 0.0);
    }

    #[test]
    fn adaptive_saturation_no_links_returns_one() {
        let ctx = AdaptiveContext::new().with_link_stats(0, 0);
        let result = AdaptiveStrategy::evaluate(&ctx);
        assert!((result.saturation - 1.0).abs() < 1e-6);
    }

    #[test]
    fn adaptive_saturation_new_exceeds_total_clamped() {
        // new > total（统计口径不同），clamp 到 [0, 1]
        let ctx = AdaptiveContext::new().with_link_stats(5, 10);
        let result = AdaptiveStrategy::evaluate(&ctx);
        assert!((result.saturation - 1.0).abs() < 1e-6);
    }

    #[test]
    fn adaptive_combined_metrics() {
        let ctx = AdaptiveContext::new()
            .with_crawled_urls(vec![
                "https://example.com/rust-crawler".to_string(),
                "https://example.com/rust-async".to_string(),
            ])
            .with_keywords(vec!["rust".to_string(), "crawler".to_string()])
            .with_link_stats(20, 5);
        let result = AdaptiveStrategy::evaluate(&ctx);
        // confidence: 两个 URL 都含 "rust"，第一个还含 "crawler"
        // URL1: 2/2=1.0, URL2: 1/2=0.5 → avg = 0.75
        assert!((result.confidence - 0.75).abs() < 1e-6);
        // coverage: rust + crawler 都在某个 URL 中 → 1.0
        assert!((result.coverage - 1.0).abs() < 1e-6);
        // saturation: 5/20 = 0.25
        assert!((result.saturation - 0.25).abs() < 1e-6);
    }

    // ============ StopCondition ============

    #[test]
    fn stop_condition_empty_never_stops() {
        let condition = StopCondition::new();
        let stats = CrawlStats::new().with_pages(1000).with_pending(0);
        // 空条件但 pending=0 → NoPendingLinks
        assert_eq!(
            condition.should_stop(&stats),
            Some(StopReason::NoPendingLinks)
        );
    }

    #[test]
    fn stop_condition_empty_with_pending_never_stops() {
        let condition = StopCondition::new();
        let stats = CrawlStats::new().with_pages(1000).with_pending(10);
        assert!(condition.should_stop(&stats).is_none());
    }

    #[test]
    fn stop_condition_no_pending_links() {
        let condition = StopCondition::new().with_max_pages(100);
        let stats = CrawlStats::new().with_pages(10).with_pending(0);
        assert_eq!(
            condition.should_stop(&stats),
            Some(StopReason::NoPendingLinks)
        );
    }

    #[test]
    fn stop_condition_max_pages_reached() {
        let condition = StopCondition::new().with_max_pages(50);
        let stats = CrawlStats::new().with_pages(50).with_pending(10);
        assert_eq!(
            condition.should_stop(&stats),
            Some(StopReason::MaxPagesReached {
                crawled: 50,
                max: 50
            })
        );
    }

    #[test]
    fn stop_condition_max_pages_not_reached() {
        let condition = StopCondition::new().with_max_pages(50);
        let stats = CrawlStats::new().with_pages(49).with_pending(10);
        assert!(condition.should_stop(&stats).is_none());
    }

    #[test]
    fn stop_condition_confidence_reached() {
        let condition = StopCondition::new().with_min_confidence(0.8);
        let stats = CrawlStats::new()
            .with_pages(10)
            .with_pending(10)
            .with_result(StrategyResult {
                confidence: 0.85,
                coverage: 0.5,
                saturation: 0.3,
            });
        assert_eq!(
            condition.should_stop(&stats),
            Some(StopReason::ConfidenceReached {
                confidence: 0.85,
                threshold: 0.8
            })
        );
    }

    #[test]
    fn stop_condition_confidence_not_reached() {
        let condition = StopCondition::new().with_min_confidence(0.8);
        let stats = CrawlStats::new()
            .with_pages(10)
            .with_pending(10)
            .with_result(StrategyResult {
                confidence: 0.79,
                coverage: 0.5,
                saturation: 0.3,
            });
        assert!(condition.should_stop(&stats).is_none());
    }

    #[test]
    fn stop_condition_saturation_reached() {
        let condition = StopCondition::new().with_saturation_threshold(0.1);
        let stats = CrawlStats::new()
            .with_pages(10)
            .with_pending(10)
            .with_result(StrategyResult {
                confidence: 0.3,
                coverage: 0.5,
                saturation: 0.05, // 低于 0.1
            });
        assert_eq!(
            condition.should_stop(&stats),
            Some(StopReason::SaturationReached {
                saturation: 0.05,
                threshold: 0.1
            })
        );
    }

    #[test]
    fn stop_condition_saturation_not_reached() {
        let condition = StopCondition::new().with_saturation_threshold(0.1);
        let stats = CrawlStats::new()
            .with_pages(10)
            .with_pending(10)
            .with_result(StrategyResult {
                confidence: 0.3,
                coverage: 0.5,
                saturation: 0.15, // 高于 0.1
            });
        assert!(condition.should_stop(&stats).is_none());
    }

    #[test]
    fn stop_condition_priority_no_pending_first() {
        // 多个条件同时满足时，NoPendingLinks 优先
        let condition = StopCondition::new()
            .with_max_pages(10)
            .with_min_confidence(0.8);
        let stats = CrawlStats::new()
            .with_pages(20)
            .with_pending(0) // NoPendingLinks 优先
            .with_result(StrategyResult {
                confidence: 0.9,
                coverage: 0.5,
                saturation: 0.3,
            });
        assert_eq!(
            condition.should_stop(&stats),
            Some(StopReason::NoPendingLinks)
        );
    }

    #[test]
    fn stop_condition_priority_max_pages_before_confidence() {
        // MaxPages 优先于 Confidence
        let condition = StopCondition::new()
            .with_max_pages(10)
            .with_min_confidence(0.8);
        let stats = CrawlStats::new()
            .with_pages(15)
            .with_pending(10)
            .with_result(StrategyResult {
                confidence: 0.9,
                coverage: 0.5,
                saturation: 0.3,
            });
        assert_eq!(
            condition.should_stop(&stats),
            Some(StopReason::MaxPagesReached {
                crawled: 15,
                max: 10
            })
        );
    }

    #[test]
    fn stop_condition_priority_confidence_before_saturation() {
        // Confidence 优先于 Saturation
        let condition = StopCondition::new()
            .with_min_confidence(0.8)
            .with_saturation_threshold(0.1);
        let stats = CrawlStats::new()
            .with_pages(10)
            .with_pending(10)
            .with_result(StrategyResult {
                confidence: 0.9, // 满足
                coverage: 0.5,
                saturation: 0.05, // 也满足
            });
        assert_eq!(
            condition.should_stop(&stats),
            Some(StopReason::ConfidenceReached {
                confidence: 0.9,
                threshold: 0.8
            })
        );
    }

    #[test]
    fn stop_condition_all_conditions_combined() {
        let condition = StopCondition::new()
            .with_max_pages(100)
            .with_min_confidence(0.8)
            .with_saturation_threshold(0.1);
        // 全部不满足 → 继续
        let stats = CrawlStats::new()
            .with_pages(50)
            .with_pending(10)
            .with_result(StrategyResult {
                confidence: 0.5,
                coverage: 0.3,
                saturation: 0.3,
            });
        assert!(condition.should_stop(&stats).is_none());
    }

    #[test]
    fn stop_reason_description() {
        let r1 = StopReason::MaxPagesReached {
            crawled: 100,
            max: 100,
        };
        assert!(r1.description().contains("100"));

        let r2 = StopReason::ConfidenceReached {
            confidence: 0.85,
            threshold: 0.8,
        };
        assert!(r2.description().contains("0.850"));
        assert!(r2.description().contains("0.800"));

        let r3 = StopReason::SaturationReached {
            saturation: 0.05,
            threshold: 0.1,
        };
        assert!(r3.description().contains("0.050"));

        let r4 = StopReason::NoPendingLinks;
        assert!(r4.description().contains("no pending"));
    }

    #[test]
    fn stop_condition_clamp_confidence_threshold() {
        // 阈值超出 [0, 1] 时 clamp
        let condition = StopCondition::new().with_min_confidence(1.5);
        let stats = CrawlStats::new()
            .with_pages(10)
            .with_pending(10)
            .with_result(StrategyResult {
                confidence: 1.0, // 满足 clamp 后的 1.0
                coverage: 0.5,
                saturation: 0.3,
            });
        assert_eq!(
            condition.should_stop(&stats),
            Some(StopReason::ConfidenceReached {
                confidence: 1.0,
                threshold: 1.0 // clamped
            })
        );
    }
}
