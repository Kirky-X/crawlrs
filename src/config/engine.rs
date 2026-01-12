// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 引擎评分配置
//!
//! 配置引擎选择和负载均衡的评分参数

use serde::Deserialize;

/// 引擎评分配置
///
/// 控制引擎选择的评分算法参数，包括基础分数、权重和阈值
///
/// # 字段说明
///
/// * `success_rate_weight` - 成功率权重，默认 0.4
/// * `latency_weight` - 延迟权重，默认 0.3
/// * `freshness_weight` - 新鲜度权重，默认 0.2
/// * `reliability_weight` - 可靠性权重，默认 0.1
/// * `min_success_rate` - 最小成功率阈值，默认 0.5
/// * `max_latency_ms` - 最大可接受延迟（毫秒），默认 10000
/// * `score_threshold` - 引擎启用分数阈值，默认 0.3
#[derive(Debug, Clone, Deserialize)]
pub struct EngineScoringConfig {
    /// 成功率权重 (0.0 - 1.0)
    pub success_rate_weight: f64,
    /// 延迟权重 (0.0 - 1.0)
    pub latency_weight: f64,
    /// 新鲜度权重 (0.0 - 1.0) - 优先使用最近成功的引擎
    pub freshness_weight: f64,
    /// 可靠性权重 (0.0 - 1.0)
    pub reliability_weight: f64,
    /// 最小成功率阈值 - 低于此值的引擎将被禁用
    pub min_success_rate: f64,
    /// 最大可接受延迟（毫秒）- 超过此值的引擎将被降级
    pub max_latency_ms: u64,
    /// 引擎启用分数阈值 - 低于此值的引擎将被禁用
    pub score_threshold: f64,
}

impl Default for EngineScoringConfig {
    fn default() -> Self {
        Self {
            success_rate_weight: 0.40,
            latency_weight: 0.30,
            freshness_weight: 0.20,
            reliability_weight: 0.10,
            min_success_rate: 0.50,
            max_latency_ms: 10000,
            score_threshold: 0.30,
        }
    }
}

/// 引擎选择配置
#[derive(Debug, Clone, Deserialize)]
pub struct EngineSelectionConfig {
    /// 启用引擎评分选择
    pub enable_scoring: bool,
    /// 最大候选引擎数量
    pub max_candidates: usize,
    /// 启用负载均衡
    pub enable_load_balancing: bool,
    /// 启用故障转移
    pub enable_failover: bool,
    /// 最大重试次数
    pub max_retries: u32,
    /// 请求超时时间（毫秒）
    pub request_timeout_ms: u64,
}

impl Default for EngineSelectionConfig {
    fn default() -> Self {
        Self {
            enable_scoring: true,
            max_candidates: 3,
            enable_load_balancing: true,
            enable_failover: true,
            max_retries: 3,
            request_timeout_ms: 30000,
        }
    }
}
