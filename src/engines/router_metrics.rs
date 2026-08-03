// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 路由层指标收集器
//!
//! 从 `router.rs` 拆分而来（ARC-003），降低单文件复杂度。
//! 包含 `RouterMetrics` 及其辅助类型 `EngineStats`。

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 路由层指标收集器
///
/// 收集引擎路由过程中的各种指标，用于监控和优化
///
/// # 安全提示
///
/// 所有字段都是内部实现细节，仅对 crate 可见。
/// 外部模块应使用提供的公共方法访问聚合统计数据。
#[derive(Debug, Default)]
pub struct RouterMetrics {
    /// 总请求数
    pub(crate) total_requests: AtomicU64,
    /// 成功请求数
    pub(crate) successful_requests: AtomicU64,
    /// 失败请求数
    pub(crate) failed_requests: AtomicU64,
    /// 候选引擎数量统计
    pub(crate) candidate_count_total: AtomicU64,
    /// 尝试次数统计
    pub(crate) attempt_count_total: AtomicU64,
    /// 引擎选择次数
    pub(crate) engine_selection_total: AtomicU64,
    /// 按引擎名称的延迟统计 (引擎名 -> 总延迟纳秒) - PERF-004: AtomicU64 无锁累加
    pub(crate) engine_latencies: Arc<DashMap<String, AtomicU64>>,
    /// 按引擎名称的成功次数 - PERF-004: AtomicU64 无锁累加
    pub(crate) engine_success_count: Arc<DashMap<String, AtomicU64>>,
    /// 按引擎名称的失败次数 - PERF-004: AtomicU64 无锁累加
    pub(crate) engine_failure_count: Arc<DashMap<String, AtomicU64>>,
    /// 失败类型统计 (错误类型 -> 次数) - 使用 DashMap 优化并发性能
    pub(crate) failure_classification: Arc<DashMap<String, u64>>,
}

impl RouterMetrics {
    /// 创建新的指标收集器
    pub fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            candidate_count_total: AtomicU64::new(0),
            attempt_count_total: AtomicU64::new(0),
            engine_selection_total: AtomicU64::new(0),
            engine_latencies: Arc::new(DashMap::with_capacity(8)),
            engine_success_count: Arc::new(DashMap::with_capacity(8)),
            engine_failure_count: Arc::new(DashMap::with_capacity(8)),
            failure_classification: Arc::new(DashMap::with_capacity(8)),
        }
    }

    /// 安全获取 latencies (PERF-004: AtomicU64 无锁累加)
    fn latencies(&self) -> &DashMap<String, AtomicU64> {
        &self.engine_latencies
    }

    /// 安全获取 success_count (PERF-004: AtomicU64 无锁累加)
    fn success_count(&self) -> &DashMap<String, AtomicU64> {
        &self.engine_success_count
    }

    /// 安全获取 failure_count (PERF-004: AtomicU64 无锁累加)
    fn failure_count(&self) -> &DashMap<String, AtomicU64> {
        &self.engine_failure_count
    }

    /// 安全获取 classification (DashMap 不需要 async 锁)
    fn classification(&self) -> &DashMap<String, u64> {
        &self.failure_classification
    }

    /// 对错误进行分类
    pub(crate) fn classify_error(error_type: &str) -> String {
        let lower = error_type.to_lowercase();
        if lower.contains("timeout") {
            "timeout".to_string()
        } else if lower.contains("ssrf") {
            "ssrf_protection".to_string()
        } else if lower.contains("network") {
            "network_error".to_string()
        } else if lower.contains("circuit") {
            "circuit_breaker".to_string()
        } else if lower.contains("browser") {
            "browser_error".to_string()
        } else {
            "other".to_string()
        }
    }

    /// 记录候选引擎数量
    pub fn record_candidates(&self, count: usize) {
        self.candidate_count_total
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// 记录单次尝试
    pub fn record_attempt(&self) {
        self.attempt_count_total.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录引擎选择
    ///
    /// 仅累加 selection 计数；延迟/成功/失败统计各自在 `record_engine_*` 中用 `entry().or_insert(0)`
    /// 自动初始化，避免重置已累计的值（架构审查 HIGH-1 修复）。
    ///
    /// 注：`engine_name` 参数保留以维持 API 兼容（调用方按语义传入），但本方法不再使用它
    /// 来初始化 latencies（原 bug：insert(..., 0) 会重置累计延迟）。
    pub fn record_engine_selection(&self, _engine_name: &str) {
        self.engine_selection_total.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录引擎延迟
    ///
    /// PERF-004: `entry().or_insert_with(AtomicU64::new).fetch_add` 无锁累加。
    pub fn record_engine_latency(&self, engine_name: &str, duration: Duration) {
        let total_ns = duration.as_nanos() as u64;
        self.latencies()
            .entry(engine_name.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(total_ns, Ordering::Relaxed);
    }

    /// 记录引擎成功
    ///
    /// PERF-004: `entry().or_insert_with(AtomicU64::new).fetch_add` 无锁累加。
    pub fn record_engine_success(&self, engine_name: &str) {
        self.success_count()
            .entry(engine_name.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    /// 记录引擎失败
    ///
    /// PERF-004: failure_count 用 AtomicU64 无锁累加；failure_classification 保持 DashMap<String, u64>。
    pub fn record_engine_failure(&self, engine_name: &str, error_type: &str) {
        self.failure_count()
            .entry(engine_name.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);

        let error_category = Self::classify_error(error_type);
        self.classification()
            .entry(error_category)
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }

    /// 获取按引擎名称的平均延迟（纳秒）
    pub fn get_avg_latency_ns(&self, engine_name: &str) -> Option<u64> {
        // PERF-004: AtomicU64 load 无锁读取
        let latencies = self.latencies();
        let success_count = self.success_count();

        if let (Some(total_ns_ref), Some(count_ref)) =
            (latencies.get(engine_name), success_count.get(engine_name))
        {
            let total_ns = total_ns_ref.load(Ordering::Relaxed);
            let count = count_ref.load(Ordering::Relaxed);
            return total_ns.checked_div(count);
        }
        None
    }

    /// 获取成功率
    pub fn get_success_rate(&self) -> f64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return 1.0;
        }
        self.successful_requests.load(Ordering::Relaxed) as f64 / total as f64
    }
}

/// 引擎性能统计
#[derive(Debug, Clone)]
pub struct EngineStats {
    /// 成功率 (0.0 - 1.0)
    pub success_rate: f64,
    /// 平均响应时间
    pub avg_response_time: Duration,
    /// 最近使用时间
    pub last_used: Option<Instant>,
    /// 使用次数
    pub usage_count: u64,
}

impl Default for EngineStats {
    fn default() -> Self {
        Self {
            success_rate: 1.0,
            avg_response_time: Duration::from_millis(500),
            last_used: None,
            usage_count: 0,
        }
    }
}
