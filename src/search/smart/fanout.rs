// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SmartSearchEngine 并发 fan-out 与结果收集 — 从 `smart/mod.rs` 拆分
//!
//! 基础引擎并发查询、逐引擎结果收集/去重、融合结果转换。

use super::{with_timeout, SmartSearchEngine, GATE_BAIDU, GATE_BING, GATE_SOGOU};
use crate::domain::models::search_result::SearchResult;
use crate::search::dedup::ResultDeduplicator;
use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::types::SearchEngineType;
use log::{info, warn};

/// 单引擎 fan-out 结果类型。
pub(super) type EngineSearchResult = Result<Response<ResponseItem>, SearchError>;

impl SmartSearchEngine {
    /// 并发查询三个基础引擎 + 可选附加引擎（自 [`SmartSearchEngine::search`] 提取）。
    pub(super) async fn fan_out_engines(
        &self,
        base_request: &SearchRequest,
    ) -> (
        EngineSearchResult,
        EngineSearchResult,
        EngineSearchResult,
        Option<EngineSearchResult>,
    ) {
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
                    Some(with_timeout(self.engine_timeout, engine.search(base_request)).await)
                }
                None => None,
            }
        };
        tokio::join!(
            async { engine_with_timeout!(GATE_BING, self.bing_engine.search(base_request)) },
            async { engine_with_timeout!(GATE_BAIDU, self.baidu_engine.search(base_request)) },
            async { engine_with_timeout!(GATE_SOGOU, self.sogou_engine.search(base_request)) },
            extra_future,
        )
    }
}

/// 收集单引擎结果并记录日志（自 [`SmartSearchEngine::search`] 提取）；
/// 失败按降级处理为空结果。
pub(super) fn collect_named_results(label: &str, result: EngineSearchResult) -> Vec<SearchResult> {
    match result {
        Ok(response) => {
            info!("{} returned {} results", label, response.items.len());
            response
                .items
                .iter()
                .map(SmartSearchEngine::item_to_search_result)
                .collect()
        }
        Err(e) => {
            warn!("{} search failed: {}", label, e);
            Vec::new()
        }
    }
}

/// 收集附加引擎结果并记录日志（自 [`SmartSearchEngine::search`] 提取）；
/// 未启用附加引擎或失败时返回空结果。
pub(super) fn collect_extra_results(
    engine: &SmartSearchEngine,
    extra_result: Option<EngineSearchResult>,
) -> Vec<SearchResult> {
    let Some(result) = extra_result else {
        return Vec::new();
    };

    let label = engine
        .extra_engine
        .as_ref()
        .map(|e| e.name())
        .unwrap_or("extra engine");
    match result {
        Ok(response) => {
            info!("{} returned {} results", label, response.items.len());
            response
                .items
                .iter()
                .map(SmartSearchEngine::item_to_search_result)
                .collect()
        }
        Err(e) => {
            warn!("{} search failed: {}", label, e);
            Vec::new()
        }
    }
}

/// 单引擎结果去重并重置去重器状态（自 [`SmartSearchEngine::search`] 提取）。
pub(super) fn dedup_engine_results(
    deduplicator: &mut ResultDeduplicator,
    results: Vec<SearchResult>,
) -> Vec<SearchResult> {
    let deduped = deduplicator.deduplicate(results);
    deduplicator.reset();
    deduped
}

/// 融合结果 → 响应条目（自 [`SmartSearchEngine::search`] 提取）：
/// 保留来源引擎标记，未知引擎按 Smart 回退。
pub(super) fn to_response_items(fused: Vec<SearchResult>) -> Vec<ResponseItem> {
    fused
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
        .collect()
}
