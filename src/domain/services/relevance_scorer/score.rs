// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! RelevanceScorer 分项评分 — 从 `relevance_scorer.rs` 拆分
//!
//! 标题 / 描述 / URL 三个字段的加权命中分按原顺序累加到调用方提供的
//! 累加器上（保持与原实现一致的浮点求和顺序）。

use super::RelevanceScorer;

impl RelevanceScorer {
    /// 标题命中（最高权重）：contains 2.0 / 前缀 1.5 / 词边界 1.2。
    pub(super) fn accumulate_title_score(&self, title_lower: &str, score: &mut f64) {
        for term in &self.query_terms {
            let weight = self.term_weights.get(term).unwrap_or(&1.0);

            // Exact title match
            if title_lower.contains(term) {
                *score += 2.0 * weight;
            }

            // Title starts with term
            if title_lower.starts_with(term) {
                *score += 1.5 * weight;
            }

            // Title word boundary match
            if self.has_word_boundary_match(title_lower, term) {
                *score += 1.2 * weight;
            }
        }
    }

    /// 描述命中（中权重）：contains 0.8 / 词边界 0.6。
    pub(super) fn accumulate_description_score(&self, desc_lower: &str, score: &mut f64) {
        for term in &self.query_terms {
            let weight = self.term_weights.get(term).unwrap_or(&1.0);

            if desc_lower.contains(term) {
                *score += 0.8 * weight;
            }

            if self.has_word_boundary_match(desc_lower, term) {
                *score += 0.6 * weight;
            }
        }
    }

    /// URL 命中（低权重）：contains 0.4。
    pub(super) fn accumulate_url_score(&self, url_lower: &str, score: &mut f64) {
        for term in &self.query_terms {
            let weight = self.term_weights.get(term).unwrap_or(&1.0);

            if url_lower.contains(term) {
                *score += 0.4 * weight;
            }
        }
    }
}
