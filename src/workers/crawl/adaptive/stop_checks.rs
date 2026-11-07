// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 停止条件检查（从 `adaptive.rs` 拆分的 partial impl block）

use super::{CrawlStats, StopCondition, StopReason};

impl StopCondition {
    /// 1. 无待处理链接 → 立即停止（自 [`StopCondition::should_stop`] 提取）
    pub(super) fn check_pending_links(&self, stats: &CrawlStats) -> Option<StopReason> {
        if stats.pending_links == 0 {
            return Some(StopReason::NoPendingLinks);
        }
        None
    }

    /// 2. 最大页数（自 [`StopCondition::should_stop`] 提取）
    pub(super) fn check_max_pages(&self, stats: &CrawlStats) -> Option<StopReason> {
        if let Some(max) = self.max_pages {
            if stats.pages_crawled >= max {
                return Some(StopReason::MaxPagesReached {
                    crawled: stats.pages_crawled,
                    max,
                });
            }
        }
        None
    }

    /// 3. 置信度阈值（自 [`StopCondition::should_stop`] 提取）
    pub(super) fn check_confidence(&self, stats: &CrawlStats) -> Option<StopReason> {
        if let Some(threshold) = self.min_confidence {
            if stats.result.confidence >= threshold {
                return Some(StopReason::ConfidenceReached {
                    confidence: stats.result.confidence,
                    threshold,
                });
            }
        }
        None
    }

    /// 检查 4：饱和度阈值（低于阈值 = 新内容发现率过低）。
    /// 自 [`StopCondition::should_stop`] 提取。
    pub(super) fn check_saturation(&self, stats: &CrawlStats) -> Option<StopReason> {
        if let Some(threshold) = self.saturation_threshold {
            if stats.result.saturation < threshold {
                return Some(StopReason::SaturationReached {
                    saturation: stats.result.saturation,
                    threshold,
                });
            }
        }
        None
    }

    /// 检查 5：KG 覆盖率阈值（达到阈值 = 知识图谱已充分覆盖）。
    /// 自 [`StopCondition::should_stop`] 提取。
    pub(super) fn check_coverage(&self, stats: &CrawlStats) -> Option<StopReason> {
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
