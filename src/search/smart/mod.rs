// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SmartSearchEngine — 并发聚合搜索引擎
//!
//! 同时查询 Baidu/Bing/Sogou 三个基础引擎，经 SimHash 去重 → RRF 融合 →
//! RelevanceScorer 评分后返回按综合分数排序的结果。

use async_trait::async_trait;
use log::{error, info, warn};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    /// 单引擎 fan-out 超时（F1）：超时引擎按失败降级，不拖死整体聚合
    engine_timeout: Duration,
    /// 出站 per-engine 最小间隔 gate（F4）：防止把搜索引擎自己打进封禁
    gates: [OutboundGate; 3],
    /// 附加引擎（如 SearXNG）：与三个基础引擎并发 fan-out 并参与 RRF 融合
    extra_engine: Option<Arc<dyn SearchEngine>>,
}

/// 出站请求节流 gate：每引擎维护上次请求时刻，wait_turn 睡到到期。
/// 等待在锁外进行（先算出目标时刻再释放锁），避免持锁睡眠。
struct OutboundGate {
    min_interval: Duration,
    last: Mutex<Option<Instant>>,
}

impl OutboundGate {
    fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last: Mutex::new(None),
        }
    }

    async fn wait_turn(&self) {
        let target = {
            let mut last = self.last.lock().unwrap();
            let now = Instant::now();
            let start = match *last {
                Some(prev) => prev + self.min_interval,
                None => now,
            };
            *last = Some(start.max(now));
            start.max(now)
        };
        let wait = target.saturating_duration_since(Instant::now());
        if wait > Duration::ZERO {
            tokio::time::sleep(wait).await;
        }
    }
}

/// 单引擎 fan-out 超时默认值（秒，对齐 SearchSettings::timeout_seconds 默认）
const DEFAULT_ENGINE_TIMEOUT_SECS: u64 = 30;

/// 引擎 gate 索引（与 engines 数组顺序一致）
const GATE_BING: usize = 0;
const GATE_BAIDU: usize = 1;
const GATE_SOGOU: usize = 2;

/// F1：单引擎 future 超时包裹——超时按失败降级（部分结果容忍）
async fn with_timeout<F, T>(timeout: Duration, fut: F) -> Result<T, SearchError>
where
    F: std::future::Future<Output = Result<T, SearchError>>,
{
    match tokio::time::timeout(timeout, fut).await {
        Ok(result) => result,
        Err(_) => {
            warn!(
                "{}",
                crate::i18n::tr_log_args(
                    "search-smart-engine-timeout",
                    &[(
                        "timeout",
                        fluent_bundle::FluentValue::from(format!("{timeout:?}")),
                    )],
                )
            );
            Err(SearchError::SmartRoutingTimeout(timeout.as_secs()))
        }
    }
}

/// F2：从用户请求构造引擎共享请求（query/limit + locale/offset 贯通）
fn build_base_request(request: &SearchRequest) -> SearchRequest {
    let mut base = SearchRequest::new(&request.query).with_limit(request.limit);
    if request.offset > 0 {
        base = base.with_offset(request.offset);
    }
    if let Some(lang) = &request.lang {
        base = base.with_lang(lang);
    }
    if let Some(country) = &request.country {
        base = base.with_country(country);
    }
    base
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
            engine_timeout: Duration::from_secs(DEFAULT_ENGINE_TIMEOUT_SECS),
            gates: [
                OutboundGate::new(Duration::from_secs(1)), // bing
                OutboundGate::new(Duration::from_secs(2)), // baidu
                OutboundGate::new(Duration::from_secs(2)), // sogou
            ],
            extra_engine: None,
        }
    }

    /// 注入一个附加引擎参与聚合（如启用 SearXNG 时的 `SearxngSearchEngine`）。
    ///
    /// 附加引擎与 bing/baidu/sogou 并发 fan-out，逐引擎去重后一并交给 RRF
    /// 融合；“至少一个引擎返回结果”的判定同样纳入它。
    #[must_use]
    pub fn with_additional_engine(mut self, engine: Arc<dyn SearchEngine>) -> Self {
        self.extra_engine = Some(engine);
        self
    }

    /// 注入单引擎 fan-out 超时（秒，来自 `SearchSettings.timeout_seconds`）
    #[must_use]
    pub fn with_engine_timeout(mut self, secs: u64) -> Self {
        self.engine_timeout = Duration::from_secs(secs);
        self
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

        info!(
            "{}",
            crate::i18n::tr_log_args(
                "search-smart-start",
                &[
                    ("query", fluent_bundle::FluentValue::from(query.as_str())),
                    ("limit", fluent_bundle::FluentValue::from(limit as i64)),
                    (
                        "lang",
                        fluent_bundle::FluentValue::from(format!("{:?}", request.lang)),
                    ),
                    (
                        "country",
                        fluent_bundle::FluentValue::from(format!("{:?}", request.country)),
                    ),
                    (
                        "offset",
                        fluent_bundle::FluentValue::from(request.offset as i64)
                    ),
                ],
            )
        );

        // 构建共享请求（每个基础引擎使用相同 query/limit/locale/offset）。
        // F2：lang/country/offset 端到端贯通——此前在此处被丢弃。
        let base_request = build_base_request(request);

        // F1：每个引擎 future 独立超时——超时引擎按失败降级（部分结果容忍），
        // 消除"单引擎 hang 拖死整体聚合"（引擎路由层的瀑布重试会叠加耗时）
        macro_rules! engine_with_timeout {
            ($gate:expr, $fut:expr) => {{
                self.gates[$gate].wait_turn().await;
                with_timeout(self.engine_timeout, $fut).await
            }};
        }

        // 并发查询三个基础引擎 + 可选附加引擎（SearXNG）
        let extra_future = async {
            match &self.extra_engine {
                Some(engine) => {
                    Some(with_timeout(self.engine_timeout, engine.search(&base_request)).await)
                }
                None => None,
            }
        };
        let (bing_result, baidu_result, sogou_result, extra_result) = tokio::join!(
            async { engine_with_timeout!(GATE_BING, self.bing_engine.search(&base_request)) },
            async { engine_with_timeout!(GATE_BAIDU, self.baidu_engine.search(&base_request)) },
            async { engine_with_timeout!(GATE_SOGOU, self.sogou_engine.search(&base_request)) },
            extra_future,
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

        // 附加引擎结果（启用 SearXNG 时参与融合）
        let mut extra_results: Vec<SearchResult> = Vec::new();
        if let Some(result) = extra_result {
            match result {
                Ok(response) => {
                    info!(
                        "{} returned {} results",
                        self.extra_engine
                            .as_ref()
                            .map(|e| e.name())
                            .unwrap_or("extra engine"),
                        response.items.len()
                    );
                    extra_results = response
                        .items
                        .iter()
                        .map(Self::item_to_search_result)
                        .collect();
                }
                Err(e) => {
                    warn!(
                        "{} search failed: {}",
                        self.extra_engine
                            .as_ref()
                            .map(|e| e.name())
                            .unwrap_or("extra engine"),
                        e
                    );
                }
            }
        }

        // 至少一个引擎需要返回结果
        if bing_results.is_empty()
            && baidu_results.is_empty()
            && sogou_results.is_empty()
            && extra_results.is_empty()
        {
            error!("All search engines failed or returned empty results");
            return Err(SearchError::AllEnginesFailed);
        }

        info!(
            "Collected results: bing={}, baidu={}, sogou={}, extra={}",
            bing_results.len(),
            baidu_results.len(),
            sogou_results.len(),
            extra_results.len()
        );

        // Step 1: SimHash + URL 去重（每个引擎内部先去重）
        let mut deduplicator = ResultDeduplicator::with_default_config();
        bing_results = deduplicator.deduplicate(bing_results);
        deduplicator.reset();
        baidu_results = deduplicator.deduplicate(baidu_results);
        deduplicator.reset();
        sogou_results = deduplicator.deduplicate(sogou_results);
        deduplicator.reset();
        extra_results = deduplicator.deduplicate(extra_results);

        // Step 2: RRF 融合（跨引擎去重 + 排名融合）
        let mut fused = self.rrf_fuser.fuse(vec![
            bing_results,
            baidu_results,
            sogou_results,
            extra_results,
        ]);

        info!("RRF fused {} unique results", fused.len());

        // F3：跨引擎标题近重复去重——RRF 只按 URL 归一合并，两引擎返回
        // 同内容不同 URL 时会双双存活；SimHash 标题去重兜底（保序）。
        let mut cross_dedup = ResultDeduplicator::with_default_config();
        let before = fused.len();
        fused = cross_dedup.deduplicate(fused);
        if fused.len() != before {
            info!(
                "{}",
                crate::i18n::tr_log_args(
                    "search-smart-cross-dedup",
                    &[(
                        "count",
                        fluent_bundle::FluentValue::from((before - fused.len()) as i64)
                    )],
                )
            );
        }

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

    // ==== r2：F1 超时降级 / F2 locale 贯通 / F3 跨引擎去重 / F4 出站限速 ====

    /// F1：超时 future 映射 SmartRoutingTimeout，正常 future 原样透传
    #[tokio::test]
    async fn test_with_timeout_degrades_on_slow_future() {
        use std::time::Duration;

        // SmartRoutingTimeout 载荷为秒：用秒级差值断言语义
        let slow = async {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            Ok::<_, SearchError>(1u8)
        };
        let result = with_timeout(Duration::from_secs(1), slow).await;
        assert!(
            matches!(result, Err(SearchError::SmartRoutingTimeout(1))),
            "slow future must degrade to SmartRoutingTimeout(1), got {result:?}"
        );

        let fast = async { Ok::<_, SearchError>(2u8) };
        assert_eq!(with_timeout(Duration::from_secs(1), fast).await.unwrap(), 2);
    }

    /// F2：base_request 贯通 lang/country/offset（此前被丢弃）
    #[test]
    fn test_build_base_request_carries_locale_and_offset() {
        let mut request = SearchRequest::new("rust");
        request.limit = 20;
        request.offset = 10;
        request.lang = Some("en".to_string());
        request.country = Some("US".to_string());

        let base = build_base_request(&request);
        assert_eq!(base.query, "rust");
        assert_eq!(base.limit, 20);
        assert_eq!(base.offset, 10);
        assert_eq!(base.lang.as_deref(), Some("en"));
        assert_eq!(base.country.as_deref(), Some("US"));

        // 无 locale/offset 时字段保持缺省
        let plain = build_base_request(&SearchRequest::new("go"));
        assert_eq!(plain.offset, 0);
        assert!(plain.lang.is_none());
        assert!(plain.country.is_none());
    }

    /// F3：跨引擎同标题不同 URL 的结果经 SimHash 去重只存活一条
    #[test]
    fn test_cross_engine_title_dedup_merges_same_title() {
        let mut dedup = ResultDeduplicator::with_default_config();
        let merged = vec![
            SearchResult {
                title: "Rust Async Programming Guide".to_string(),
                url: "https://a.example/rust".to_string(),
                description: None,
                engine: "bing".to_string(),
                score: 0.9,
                published_time: None,
            },
            SearchResult {
                title: "Rust Async Programming Guide".to_string(),
                url: "https://b.example/rust-async".to_string(),
                description: None,
                engine: "baidu".to_string(),
                score: 0.8,
                published_time: None,
            },
        ];
        let out = dedup.deduplicate(merged);
        assert_eq!(
            out.len(),
            1,
            "same-title cross-engine duplicates must merge"
        );
    }

    /// F4：同一 gate 连续两次 wait_turn，第二次等待 ≥ min_interval
    ///
    /// 断言对齐 wait_turn 自身契约（对既有 last 睡眠 `min_interval - 已流逝`
    /// 且 tokio sleep 为 at-least 语义），不做固定裕度的间接推断——套件重负载下
    /// 语句间调度间隙（锁+时钟读数）可吞掉固定裕度造成假阳性。
    #[tokio::test]
    async fn test_outbound_gate_enforces_min_interval() {
        let min_interval = Duration::from_millis(120);
        let gate = OutboundGate::new(min_interval);
        // 首次无记录放行：上界取 min_interval 本身（「未等满间隔」即性质）
        let start = Instant::now();
        gate.wait_turn().await;
        assert!(start.elapsed() < min_interval, "first turn must not wait");
        // 第二次必须等满 min_interval：直接度量本次调用耗时，
        // 与「进入时刻相对首记录的余量」对齐（1ms 吸收 Instant 读数间距）
        let first_record = gate.last.lock().unwrap().expect("first turn must record");
        let second_start = Instant::now();
        gate.wait_turn().await;
        let waited = second_start.elapsed();
        let expected = min_interval.saturating_sub(second_start.duration_since(first_record));
        assert!(
            waited + Duration::from_millis(1) >= expected,
            "second turn must wait out the min interval (waited {waited:?}, expected ≥ {expected:?})"
        );
    }

    // ==== T019：附加引擎（SearXNG）参与并发 fan-out + RRF 融合 ====

    /// 固定返回结果的附加引擎替身
    struct StubExtraEngine {
        engine_type: SearchEngineType,
        name: &'static str,
        items: Vec<ResponseItem>,
    }

    #[async_trait]
    impl SearchEngine for StubExtraEngine {
        fn name(&self) -> &'static str {
            self.name
        }

        fn engine_type(&self) -> SearchEngineType {
            self.engine_type
        }

        fn health(&self) -> EngineHealth {
            EngineHealth::Healthy
        }

        async fn search(
            &self,
            _request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            Ok(Response {
                items: self.items.clone(),
                total_results: Some(self.items.len() as u64),
                engine: self.engine_type,
            })
        }
    }

    #[tokio::test]
    async fn test_additional_engine_participates_in_fusion() {
        let stub = StubExtraEngine {
            engine_type: SearchEngineType::Searxng,
            name: "SearxNG",
            items: vec![ResponseItem {
                title: "SearXNG result".to_string(),
                url: "https://searx-result.example/1".to_string(),
                description: "from the extra engine".to_string(),
                engine: SearchEngineType::Searxng,
            }],
        };

        let engine = SmartSearchEngine::new(Arc::new(EngineClient::new()))
            .with_engine_timeout(1)
            .with_additional_engine(Arc::new(stub));

        // 三个内置引擎在无引擎路由的 EngineClient 下失败，仅附加引擎有结果：
        // 聚合仍应成功，证明附加引擎既是“至少一个引擎有结果”的判定成员，也进入了融合。
        let response = engine
            .search(&SearchRequest::new("rust").with_limit(5))
            .await
            .expect("aggregation must succeed with the extra engine only");

        assert_eq!(response.engine, SearchEngineType::Smart);
        let fused = response
            .items
            .iter()
            .find(|item| item.url == "https://searx-result.example/1")
            .expect("extra engine result must be fused");
        assert_eq!(fused.engine, SearchEngineType::Searxng);
        assert_eq!(fused.title, "SearXNG result");
    }
}
