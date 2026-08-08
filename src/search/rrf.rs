// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Reciprocal Rank Fusion (RRF) 融合器
//!
//! 论文: Cormack, Clarke, Buettcher (2009) —
//! "Reciprocal Rank Fusion outperforms Condorcet and individual Rank Learning Methods"
//!
//! 公式: `RRF(d) = Σ 1/(k + rank_i(d))`，默认 k=60。
//! 同一 URL 出现在多个引擎的排名列表中时，RRF 分数累加。

use std::collections::HashMap;

use crate::domain::models::search_result::SearchResult;

/// 默认 RRF 常数 k
const DEFAULT_K: u32 = 60;

/// Reciprocal Rank Fusion 融合器
///
/// 将多个引擎的有序结果列表融合为单一有序列表。
/// URL（归一化后）作为文档唯一标识，同一 URL 的 RRF 分数跨引擎累加。
pub struct RRFFuser {
    k: u32,
}

impl RRFFuser {
    /// 创建 RRF 融合器
    ///
    /// # Arguments
    ///
    /// * `k` - RRF 常数，默认 60。k 越大，排名靠后的结果获得的分数越平滑。
    ///   原论文推荐 k=60。
    pub fn new(k: u32) -> Self {
        Self { k }
    }

    /// 融合多个引擎的有序结果列表
    ///
    /// # 数据流
    ///
    /// 1. 遍历每个引擎的结果列表，按排名位置（1-based）计算 `1/(k + rank)`
    /// 2. URL 归一化后作为 key，累加 RRF 分数
    /// 3. 同一 URL 首次出现时记录其 `SearchResult` 和来源引擎
    /// 4. 按 RRF 分数降序输出
    ///
    /// # Arguments
    ///
    /// * `ranked_lists` - 每个元素是一个引擎的结果列表（列表内已按该引擎排名排序）
    pub fn fuse(&self, ranked_lists: Vec<Vec<SearchResult>>) -> Vec<SearchResult> {
        // (accumulated_score, first-seen SearchResult)
        let mut scores: HashMap<String, (f64, SearchResult)> = HashMap::new();

        for results in ranked_lists {
            for (rank_0based, result) in results.into_iter().enumerate() {
                let rank = (rank_0based + 1) as u32; // 1-based rank
                let normalized_url = Self::normalize_url(&result.url);
                let rrf_score = 1.0 / (self.k + rank) as f64;

                let entry = scores
                    .entry(normalized_url)
                    .or_insert_with(|| (0.0, result.clone()));
                entry.0 += rrf_score;
            }
        }

        // 收集并按 RRF 分数降序排序
        let mut fused: Vec<SearchResult> = scores
            .into_values()
            .map(|(score, mut result)| {
                result.score = score;
                result
            })
            .collect();

        fused.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        fused
    }

    /// URL 归一化：小写 + 移除 fragment + 移除 query + 移除末尾斜杠
    fn normalize_url(url: &str) -> String {
        let mut normalized = url.to_string();

        // 移除 fragment
        if let Some(pos) = normalized.rfind('#') {
            normalized.truncate(pos);
        }

        // 移除 query params
        if let Some(pos) = normalized.rfind('?') {
            normalized.truncate(pos);
        }

        // 小写
        normalized = normalized.to_lowercase();

        // 移除末尾斜杠
        normalized = normalized.trim_end_matches('/').to_string();

        normalized
    }
}

impl Default for RRFFuser {
    fn default() -> Self {
        Self { k: DEFAULT_K }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::search_result::SearchResult;

    fn make_result(url: &str, title: &str, engine: &str) -> SearchResult {
        SearchResult {
            title: title.to_string(),
            url: url.to_string(),
            description: None,
            engine: engine.to_string(),
            score: 0.0,
            published_time: None,
        }
    }

    #[test]
    fn test_single_engine_input() {
        let fuser = RRFFuser::default();
        let results = vec![
            make_result("https://example.com/1", "First", "bing"),
            make_result("https://example.com/2", "Second", "bing"),
            make_result("https://example.com/3", "Third", "bing"),
        ];

        let fused = fuser.fuse(vec![results]);
        assert_eq!(fused.len(), 3);
        // rank 1 → 1/(60+1) ≈ 0.01639, rank 2 → 1/(60+2) ≈ 0.01613, rank 3 → 1/(60+3) ≈ 0.01587
        assert!(fused[0].score > fused[1].score);
        assert!(fused[1].score > fused[2].score);
        assert_eq!(fused[0].url, "https://example.com/1");
    }

    #[test]
    fn test_cross_engine_duplicate_url_accumulates() {
        let fuser = RRFFuser::default();
        let bing_results = vec![
            make_result("https://example.com/shared", "Bing First", "bing"),
            make_result("https://example.com/bing-only", "Bing Second", "bing"),
        ];
        let baidu_results = vec![
            make_result("https://example.com/shared", "Baidu First", "baidu"),
            make_result("https://example.com/baidu-only", "Baidu Second", "baidu"),
        ];

        let fused = fuser.fuse(vec![bing_results, baidu_results]);

        // 3 unique URLs: shared (appears in both), bing-only, baidu-only
        assert_eq!(fused.len(), 3);

        // "shared" URL should have highest score (accumulated from both engines)
        assert_eq!(fused[0].url, "https://example.com/shared");
        // Its score should be 1/(60+1) + 1/(60+1) = 2/61
        let expected_shared_score = 2.0 / 61.0;
        assert!(
            (fused[0].score - expected_shared_score).abs() < 1e-10,
            "shared URL score should be {}, got {}",
            expected_shared_score,
            fused[0].score
        );

        // "bing-only" and "baidu-only" both have rank 2 in their respective lists
        // so they should have equal scores
        assert!((fused[1].score - fused[2].score).abs() < 1e-10);
    }

    #[test]
    fn test_empty_input() {
        let fuser = RRFFuser::default();
        let fused = fuser.fuse(vec![]);
        assert!(fused.is_empty());
    }

    #[test]
    fn test_empty_inner_lists() {
        let fuser = RRFFuser::default();
        let fused = fuser.fuse(vec![vec![], vec![]]);
        assert!(fused.is_empty());
    }

    #[test]
    fn test_k_parameter_affects_scores() {
        let fuser_small_k = RRFFuser::new(10);
        let fuser_large_k = RRFFuser::new(1000);

        let results = vec![make_result("https://example.com/1", "First", "bing")];

        let fused_small = fuser_small_k.fuse(vec![results.clone()]);
        let fused_large = fuser_large_k.fuse(vec![results]);

        // smaller k → higher score for same rank
        assert!(fused_small[0].score > fused_large[0].score);
        // small k: 1/(10+1) ≈ 0.0909
        assert!((fused_small[0].score - 1.0 / 11.0).abs() < 1e-10);
        // large k: 1/(1000+1) ≈ 0.000999
        assert!((fused_large[0].score - 1.0 / 1001.0).abs() < 1e-10);
    }

    #[test]
    fn test_url_normalization_deduplicates() {
        let fuser = RRFFuser::default();
        let list1 = vec![make_result(
            "https://Example.com/Page?utm=x#section",
            "Title",
            "bing",
        )];
        let list2 = vec![make_result(
            "https://example.com/page",
            "Title",
            "baidu",
        )];

        let fused = fuser.fuse(vec![list1, list2]);
        // Both normalize to "https://example.com/page" → 1 result
        assert_eq!(fused.len(), 1);
        // Score should be accumulated from both engines
        let expected = 2.0 / 61.0; // both at rank 1
        assert!((fused[0].score - expected).abs() < 1e-10);
    }

    #[test]
    fn test_source_engine_preserved() {
        let fuser = RRFFuser::default();
        let bing = vec![make_result("https://example.com/1", "Title", "bing")];
        let baidu = vec![make_result("https://example.com/2", "Title2", "baidu")];

        let fused = fuser.fuse(vec![bing, baidu]);
        assert_eq!(fused.len(), 2);
        // Each result should retain its original engine
        let engines: Vec<&str> = fused.iter().map(|r| r.engine.as_str()).collect();
        assert!(engines.contains(&"bing"));
        assert!(engines.contains(&"baidu"));
    }

    #[test]
    fn test_default_k_is_60() {
        let fuser = RRFFuser::default();
        assert_eq!(fuser.k, 60);
    }
}
