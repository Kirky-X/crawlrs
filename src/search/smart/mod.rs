// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SmartSearchEngine — 并发聚合搜索引擎
//!
//! 同时查询 Baidu/Bing/Sogou 三个基础引擎，经 SimHash 去重 → RRF 融合 →
//! RelevanceScorer 评分后返回按综合分数排序的结果。

use async_trait::async_trait;
use log::{error, info, warn};
use std::sync::Arc;

use crate::domain::models::search_result::SearchResult;
use crate::domain::services::relevance_scorer::{DateParserComponent, RelevanceScorer};
use crate::engines::engine_client::EngineClient;
use crate::search::client::{BaiduSearchEngine, BingSearchEngine, SogouSearchEngine};
use crate::search::dedup::ResultDeduplicator;
use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::rrf::RRFFuser;
use crate::search::types::{EngineHealth, SearchEngineType};

/// 并发聚合搜索引擎
///
/// 数据流:
/// ```text
/// SearchRequest
///   ↓
/// tokio::join! 并发查询 Baidu + Bing + Sogou
///   ↓
/// ResultDeduplicator (SimHash + URL 归一化)
///   ↓
/// RRFFuser (Reciprocal Rank Fusion, k=60)
///   ↓
/// RelevanceScorer (TF-IDF + 新鲜度)
///   ↓
/// 按综合分数降序，截取 limit
///   ↓
/// Response<ResponseItem> (engine 字段标记来源引擎)
/// ```
pub struct SmartSearchEngine {
    baidu_engine: BaiduSearchEngine,
    bing_engine: BingSearchEngine,
    sogou_engine: SogouSearchEngine,
    rrf_fuser: RRFFuser,
}

impl SmartSearchEngine {
    /// 创建 SmartSearchEngine
    ///
    /// 内部构造三个基础引擎实例和 RRF 融合器（k=60）。
    pub fn new(engine_client: Arc<EngineClient>) -> Self {
        Self {
            baidu_engine: BaiduSearchEngine::new(engine_client.clone()),
            bing_engine: BingSearchEngine::new(engine_client.clone()),
            sogou_engine: SogouSearchEngine::new(engine_client),
            rrf_fuser: RRFFuser::default(),
        }
    }

    /// 将 ResponseItem 转换为 SearchResult（用于内部处理）
    fn item_to_search_result(item: &ResponseItem) -> SearchResult {
        SearchResult {
            title: item.title.clone(),
            url: item.url.clone(),
            description: Some(item.description.clone()),
            engine: item.engine.name().to_string(),
            score: 0.0,
            published_time: None,
        }
    }

    /// 在 RRF 分数基础上叠加 RelevanceScorer 相关度 + 新鲜度评分
    ///
    /// P13：RRF 原始分数量级仅 ~0.016-0.05（k=60），而 relevance 无上界
    /// （多词命中可达 10+），直接加权时 0.4 权重的 RRF 贡献 ≤0.02、会被
    /// 0.42 权重的 relevance 完全淹没——多引擎共识信号名存实亡。
    /// 修正：RRF 与 relevance 分别按集合内 min-max 归一化到 [0,1]
    /// （freshness 本身有界），分量间量纲可比后再按现权重加权。
    fn apply_relevance_scoring(results: &mut [SearchResult], query: &str) {
        let scorer = RelevanceScorer::for_query(query);
        let date_parser = DateParserComponent::with_defaults();

        // 第一遍：收集各分量原始值（relevance, freshness）
        let mut components: Vec<(f64, f64)> = Vec::with_capacity(results.len());
        for result in results.iter_mut() {
            let relevance_score =
                scorer.calculate_score(&result.title, result.description.as_deref(), &result.url);

            // 从描述中提取发布日期
            if let Some(ref description) = result.description {
                if let Some(published_date) =
                    RelevanceScorer::extract_published_date_with_parser(description, &date_parser)
                {
                    result.published_time = Some(published_date);
                }
            }

            let freshness_score = if let Some(published_time) = result.published_time {
                RelevanceScorer::calculate_freshness_score(published_time)
            } else {
                0.5
            };
            components.push((relevance_score, freshness_score));
        }

        let max_rrf = results.iter().map(|r| r.score).fold(0.0_f64, f64::max);
        let min_rrf = results
            .iter()
            .map(|r| r.score)
            .fold(f64::INFINITY, f64::min);
        let max_rel = components.iter().map(|c| c.0).fold(0.0_f64, f64::max);
        let min_rel = components.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);

        // min-max 归一化：max==min（空集/单条/全同分）时分量取 0，
        // 对排序无影响且避免除零
        let normalize = |v: f64, min: f64, max: f64| {
            if max > min {
                (v - min) / (max - min)
            } else {
                0.0
            }
        };

        // 第二遍：归一化后加权——RRF 40%，相关性 42%，新鲜度 18%
        for (result, (relevance_score, freshness_score)) in results.iter_mut().zip(&components) {
            let rrf_norm = normalize(result.score, min_rrf, max_rrf);
            let rel_norm = normalize(*relevance_score, min_rel, max_rel);
            result.score = rrf_norm * 0.4 + rel_norm * 0.42 + freshness_score * 0.18;
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

#[async_trait]
impl SearchEngine for SmartSearchEngine {
    fn name(&self) -> &'static str {
        "smart"
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Smart
    }

    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        let query = &request.query;
        let limit = request.limit;

        info!("Smart search start: query={}, limit={}", query, limit);

        // 构建共享请求（每个基础引擎使用相同 query/limit）
        let base_request = SearchRequest::new(query).with_limit(limit);

        // 并发查询三个基础引擎
        let (bing_result, baidu_result, sogou_result) = tokio::join!(
            self.bing_engine.search(&base_request),
            self.baidu_engine.search(&base_request),
            self.sogou_engine.search(&base_request),
        );

        // 收集各引擎结果，记录来源和排名顺序
        let mut bing_results: Vec<SearchResult> = Vec::new();
        let mut baidu_results: Vec<SearchResult> = Vec::new();
        let mut sogou_results: Vec<SearchResult> = Vec::new();

        match bing_result {
            Ok(response) => {
                info!("Bing returned {} results", response.items.len());
                bing_results = response
                    .items
                    .iter()
                    .map(Self::item_to_search_result)
                    .collect();
            }
            Err(e) => {
                warn!("Bing search failed: {}", e);
            }
        }

        match baidu_result {
            Ok(response) => {
                info!("Baidu returned {} results", response.items.len());
                baidu_results = response
                    .items
                    .iter()
                    .map(Self::item_to_search_result)
                    .collect();
            }
            Err(e) => {
                warn!("Baidu search failed: {}", e);
            }
        }

        match sogou_result {
            Ok(response) => {
                info!("Sogou returned {} results", response.items.len());
                sogou_results = response
                    .items
                    .iter()
                    .map(Self::item_to_search_result)
                    .collect();
            }
            Err(e) => {
                warn!("Sogou search failed: {}", e);
            }
        }

        // 至少一个引擎需要返回结果
        if bing_results.is_empty() && baidu_results.is_empty() && sogou_results.is_empty() {
            error!("All search engines failed or returned empty results");
            return Err(SearchError::AllEnginesFailed);
        }

        info!(
            "Collected results: bing={}, baidu={}, sogou={}",
            bing_results.len(),
            baidu_results.len(),
            sogou_results.len()
        );

        // Step 1: SimHash + URL 去重（每个引擎内部先去重）
        let mut deduplicator = ResultDeduplicator::with_default_config();
        bing_results = deduplicator.deduplicate(bing_results);
        deduplicator.reset();
        baidu_results = deduplicator.deduplicate(baidu_results);
        deduplicator.reset();
        sogou_results = deduplicator.deduplicate(sogou_results);

        // Step 2: RRF 融合（跨引擎去重 + 排名融合）
        let mut fused = self
            .rrf_fuser
            .fuse(vec![bing_results, baidu_results, sogou_results]);

        info!("RRF fused {} unique results", fused.len());

        // Step 3: 叠加 RelevanceScorer 评分
        Self::apply_relevance_scoring(&mut fused, query);

        // Step 4: 截取 limit
        fused.truncate(limit as usize);

        info!("Returning {} final results", fused.len());

        // 转换为 ResponseItem，保留来源引擎标记
        let items: Vec<ResponseItem> = fused
            .into_iter()
            .map(|r| {
                let engine_type =
                    SearchEngineType::from_name(&r.engine).unwrap_or(SearchEngineType::Smart);
                ResponseItem {
                    title: r.title,
                    url: r.url,
                    description: r.description.unwrap_or_default(),
                    engine: engine_type,
                }
            })
            .collect();

        Ok(Response {
            items,
            total_results: None,
            engine: SearchEngineType::Smart,
        })
    }
}

/// 创建 Smart 聚合搜索引擎
pub fn create_smart_search(engine_client: Arc<EngineClient>) -> Arc<dyn SearchEngine> {
    Arc::new(SmartSearchEngine::new(engine_client))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_engine_type() {
        let engine_client = Arc::new(EngineClient::new());
        let engine = SmartSearchEngine::new(engine_client);
        assert_eq!(engine.engine_type(), SearchEngineType::Smart);
        assert_eq!(engine.name(), "smart");
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    #[test]
    fn test_create_smart_search_factory() {
        let engine_client = Arc::new(EngineClient::new());
        let engine = create_smart_search(engine_client);
        assert_eq!(engine.engine_type(), SearchEngineType::Smart);
        assert_eq!(engine.name(), "smart");
    }

    #[test]
    fn test_item_to_search_result() {
        let item = ResponseItem {
            title: "Test Title".to_string(),
            url: "https://example.com".to_string(),
            description: "Test desc".to_string(),
            engine: SearchEngineType::Bing,
        };
        let result = SmartSearchEngine::item_to_search_result(&item);
        assert_eq!(result.title, "Test Title");
        assert_eq!(result.url, "https://example.com");
        assert_eq!(result.description, Some("Test desc".to_string()));
        assert_eq!(result.engine, "Bing");
        assert_eq!(result.score, 0.0);
    }

    #[test]
    fn test_apply_relevance_scoring_sorts_by_score() {
        let mut results = vec![
            SearchResult {
                title: "Rust Programming".to_string(),
                url: "https://example.com/rust".to_string(),
                description: Some("Learn Rust".to_string()),
                engine: "bing".to_string(),
                score: 0.01, // low RRF score
                published_time: None,
            },
            SearchResult {
                title: "Rust Programming Guide Tutorial".to_string(),
                url: "https://example.com/rust-guide".to_string(),
                description: Some("Complete Rust guide".to_string()),
                engine: "baidu".to_string(),
                score: 0.005, // even lower RRF score
                published_time: None,
            },
        ];

        SmartSearchEngine::apply_relevance_scoring(&mut results, "Rust Programming");

        // After scoring, results should be sorted by combined score (descending)
        assert!(
            results[0].score >= results[1].score,
            "results should be sorted by score descending"
        );
        // Scores should be non-zero after relevance scoring
        assert!(results[0].score > 0.0);
    }

    /// P13 回归钉：双引擎共识结果（RRF 最高、relevance 约为满分一半）
    /// 总分必须超过单引擎满分 relevance 结果。
    ///
    /// 修复前：RRF 原始量级 ~0.016-0.05 经 0.4 权重后 ≤0.02，被 0.42 权重的
    /// relevance 完全淹没；修复后双分量 min-max 归一化，量纲可比。
    #[test]
    fn test_apply_relevance_scoring_consensus_beats_single_engine_relevance() {
        let mut results = vec![
            // A：单引擎、relevance 满分（title 命中全部查询词）
            SearchResult {
                title: "Rust Programming".to_string(),
                url: "https://example.com/rust".to_string(),
                description: None,
                engine: "bing".to_string(),
                score: 1.0 / 61.0, // 单引擎 rank1 的 RRF ≈ 0.0164
                published_time: None,
            },
            // B：双引擎共识（两引擎都返回，RRF 更高）、relevance 居中
            // （命中 "Programming"，未命中 "Rust"）
            SearchResult {
                title: "Guide to Programming".to_string(),
                url: "https://example.com/guide".to_string(),
                description: None,
                engine: "baidu".to_string(),
                score: 1.0 / 61.0 + 1.0 / 62.0, // 双引擎共识 RRF ≈ 0.0325
                published_time: None,
            },
            // C：完全不相关（relevance 集合内最低，做归一化下界）
            SearchResult {
                title: "Unrelated Cooking Tips".to_string(),
                url: "https://example.com/cooking".to_string(),
                description: None,
                engine: "sogou".to_string(),
                score: 1.0 / 65.0, // 单引擎低排名
                published_time: None,
            },
        ];

        SmartSearchEngine::apply_relevance_scoring(&mut results, "Rust Programming");

        let top = &results[0];
        assert!(
            top.url == "https://example.com/guide",
            "multi-engine consensus result must outrank single-engine max-relevance result, got order: {:?}",
            results.iter().map(|r| (&r.url, r.score)).collect::<Vec<_>>()
        );
    }

    /// P13 除零保护：单条结果（max==min）时归一化分量取 0，不 panic
    #[test]
    fn test_apply_relevance_scoring_single_result_no_panic() {
        let mut results = vec![SearchResult {
            title: "Only Result".to_string(),
            url: "https://example.com/only".to_string(),
            description: None,
            engine: "bing".to_string(),
            score: 0.016,
            published_time: None,
        }];
        SmartSearchEngine::apply_relevance_scoring(&mut results, "Only Result");
        assert_eq!(results.len(), 1);
        assert!(results[0].score.is_finite());
    }

    #[test]
    fn test_apply_relevance_scoring_extracts_date_from_description() {
        let mut results = vec![SearchResult {
            title: "Recent Article".to_string(),
            url: "https://example.com/recent".to_string(),
            description: Some("Published on 2025-08-01, a comprehensive guide".to_string()),
            engine: "bing".to_string(),
            score: 0.01,
            published_time: None,
        }];

        SmartSearchEngine::apply_relevance_scoring(&mut results, "guide");

        // Date extraction should have populated published_time
        assert!(
            results[0].published_time.is_some(),
            "date should be extracted from description"
        );
        assert!(results[0].score > 0.0);
    }

    #[test]
    fn test_apply_relevance_scoring_no_description_uses_default_freshness() {
        let mut results = vec![SearchResult {
            title: "No Description".to_string(),
            url: "https://example.com/no-desc".to_string(),
            description: None,
            engine: "bing".to_string(),
            score: 0.01,
            published_time: None,
        }];

        SmartSearchEngine::apply_relevance_scoring(&mut results, "test");

        // No description → no date extraction → freshness defaults to 0.5
        assert!(results[0].published_time.is_none());
        assert!(results[0].score > 0.0);
    }
}
