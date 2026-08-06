// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! T034: 引擎选择逻辑 — 从 `EngineRouter` 拆分的 partial impl block
//!
//! 包含引擎候选收集、特征过滤、综合评分计算与策略排序。

use super::{EngineRouter, EngineStats, LoadBalancingStrategy};
use crate::engines::engine_client::{InternalScrapeRequest, ScraperEngine};
use rand::seq::SliceRandom;
use std::sync::Arc;
use std::time::Duration;

impl EngineRouter {
    /// 选择最优引擎
    ///
    /// # 参数
    ///
    /// * `request` - 抓取请求
    ///
    /// # 返回值
    ///
    /// 返回最优引擎列表（按优先级排序）
    pub(super) fn select_optimal_engines(
        &self,
        request: &InternalScrapeRequest,
    ) -> Vec<(f64, Arc<dyn ScraperEngine>)> {
        let mut candidates = Vec::new();

        // First pass: collect engine info without holding lock for circuit breaker checks
        let engine_infos: Vec<_> = self.engines.iter().enumerate().collect();

        for (_, engine) in &engine_infos {
            let engine_name = engine.name();

            // Check circuit breaker status FIRST (outside of stats lock)
            if self.circuit_breaker.is_open(engine_name) {
                continue;
            }

            // Feature detection filtering
            if self.feature_filter_enabled {
                if let Some(reason) = self.should_filter_by_feature(request, engine) {
                    log::debug!(
                        "Engine {} filtered by feature detection: {}",
                        engine_name,
                        reason
                    );
                    continue;
                }
            }

            // Get support score
            let support_score = engine.support_score(request) as f64;
            if support_score == 0.0 {
                continue;
            }

            candidates.push((support_score, engine_name.to_string(), Arc::clone(engine)));
        }

        // PERF-04/MEDIUM-2：一次性收集 DashMap 为 HashMap，避免循环内多次 Ref 借用，
        // 同时供 Second pass（calculate_engine_score）和 sort_candidates_by_strategy 复用，
        // DashMap 全局只遍历一次。
        let stats: std::collections::HashMap<String, EngineStats> = self
            .engine_stats
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect();

        // Second pass: calculate scores（从 HashMap 取 EngineStats，无 DashMap 借用开销）
        let mut scored_candidates = Vec::new();

        for (support_score, engine_name, engine) in candidates {
            // 性能审查 M-1 修复：循环内不 clone EngineStats，直接借用 stats HashMap
            // （原 .cloned().unwrap_or_default() 每次循环都分配 EngineStats）
            let engine_stat = stats.get(&engine_name);
            let default_stat;
            let engine_stat_ref: &EngineStats = match engine_stat {
                Some(s) => s,
                None => {
                    default_stat = EngineStats::default();
                    &default_stat
                }
            };

            // Apply dynamic threshold factor
            let adjusted_score = support_score * self.dynamic_threshold_factor;

            // Calculate final score
            let final_score = self.calculate_engine_score(adjusted_score, engine_stat_ref);

            scored_candidates.push((final_score, engine));
        }

        // Sort by strategy（复用上方已收集的 stats，无需再次遍历 DashMap）
        self.sort_candidates_by_strategy(&mut scored_candidates, &stats);

        scored_candidates
    }

    /// 特征检测过滤
    /// 根据请求特征直接过滤不适合的引擎（使用能力方法替代硬编码引擎名）
    pub(super) fn should_filter_by_feature(
        &self,
        request: &InternalScrapeRequest,
        engine: &Arc<dyn ScraperEngine>,
    ) -> Option<String> {
        // 如果需要截图，排除得分很低的引擎
        if request.needs_screenshot && engine.support_score(request) < 50 {
            return Some(format!(
                "Engine {} does not support screenshots",
                engine.name()
            ));
        }

        // 如果需要 JS 或交互动作，排除得分很低的引擎
        if (request.needs_js || !request.actions.is_empty()) && engine.support_score(request) < 50 {
            return Some(format!(
                "Engine {} does not support JavaScript",
                engine.name()
            ));
        }

        // 如果明确需要 TLS 指纹，检查得分
        if request.needs_tls_fingerprint && engine.support_score(request) < 50 {
            return Some(format!(
                "Engine {} is not optimized for TLS fingerprinting",
                engine.name()
            ));
        }

        None
    }

    /// 计算引擎综合评分
    pub(super) fn calculate_engine_score(&self, support_score: f64, stats: &EngineStats) -> f64 {
        let mut score = support_score;

        // 成功率权重 (30%)
        score *= 0.3 + (stats.success_rate * 0.7);

        // 响应时间权重 (20%)
        let response_time_score = 1.0 - (stats.avg_response_time.as_secs_f64() / 10.0).min(1.0);
        score *= 0.8 + (response_time_score * 0.2);

        // 使用频率权重 (10%)
        let usage_penalty = (stats.usage_count as f64 / 1000.0).min(0.1);
        score *= 1.0 - usage_penalty;

        score
    }

    /// 根据策略排序候选引擎
    pub(super) fn sort_candidates_by_strategy(
        &self,
        candidates: &mut Vec<(f64, Arc<dyn ScraperEngine>)>,
        stats: &std::collections::HashMap<String, EngineStats>,
    ) {
        match self.strategy {
            LoadBalancingStrategy::RoundRobin => {
                // 保持原有顺序，由外部轮询索引控制
            }
            LoadBalancingStrategy::WeightedRoundRobin => {
                // 按综合评分排序
                candidates
                    .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            }
            LoadBalancingStrategy::LeastConnections => {
                // 按使用次数升序排序
                candidates.sort_by(|a, b| {
                    let usage_a = stats.get(a.1.name()).map(|s| s.usage_count).unwrap_or(0);
                    let usage_b = stats.get(b.1.name()).map(|s| s.usage_count).unwrap_or(0);
                    usage_a.cmp(&usage_b)
                });
            }
            LoadBalancingStrategy::FastestResponse => {
                // 按响应时间升序排序
                candidates.sort_by(|a, b| {
                    let time_a = stats
                        .get(a.1.name())
                        .map(|s| s.avg_response_time)
                        .unwrap_or(Duration::MAX);
                    let time_b = stats
                        .get(b.1.name())
                        .map(|s| s.avg_response_time)
                        .unwrap_or(Duration::MAX);
                    time_a.cmp(&time_b)
                });
            }
            LoadBalancingStrategy::Random => {
                // 随机打乱
                candidates.shuffle(&mut rand::rng());
            }
            LoadBalancingStrategy::SmartHybrid => {
                // 智能混合策略：综合评分 + 最少使用 + 响应时间
                candidates.sort_by(|a, b| {
                    let score_a = a.0;
                    let score_b = b.0;

                    let usage_a = stats.get(a.1.name()).map(|s| s.usage_count).unwrap_or(0);
                    let usage_b = stats.get(b.1.name()).map(|s| s.usage_count).unwrap_or(0);

                    let time_a = stats
                        .get(a.1.name())
                        .map(|s| s.avg_response_time)
                        .unwrap_or(Duration::MAX);
                    let time_b = stats
                        .get(b.1.name())
                        .map(|s| s.avg_response_time)
                        .unwrap_or(Duration::MAX);

                    // 综合排序：评分优先，然后使用次数，最后响应时间
                    score_b
                        .partial_cmp(&score_a)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| usage_a.cmp(&usage_b))
                        .then_with(|| time_a.cmp(&time_b))
                });
            }
        }
    }
}
