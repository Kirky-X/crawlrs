// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

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
    fn apply_relevance_scoring(results: &mut [SearchResult], query: &str) {
        let scorer = RelevanceScorer::for_query(query);
        let date_parser = DateParserComponent::with_defaults();

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

            // RRF 分数权重 40%，相关性 42%，新鲜度 18%
            result.score = result.score * 0.4 + relevance_score * 0.42 + freshness_score * 0.18;
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
                bing_results = response.items.iter().map(Self::item_to_search_result).collect();
            }
            Err(e) => {
                warn!("Bing search failed: {}", e);
            }
        }

        match baidu_result {
            Ok(response) => {
                info!("Baidu returned {} results", response.items.len());
                baidu_results = response.items.iter().map(Self::item_to_search_result).collect();
            }
            Err(e) => {
                warn!("Baidu search failed: {}", e);
            }
        }

        match sogou_result {
            Ok(response) => {
                info!("Sogou returned {} results", response.items.len());
                sogou_results = response.items.iter().map(Self::item_to_search_result).collect();
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
}
