// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0
//!
//! # S6 吸收设计（selfdev-lib-adoption-r1，2026-10-11）
//!
//! 熔断状态机下沉 limiteron `ManualCircuitBreaker`（薄封装，库侧缺口见
//! docs/ARCHITECTURE.md「自研库反向需求」）：
//!
//! - **键控池**：库无键控熔断器，per-engine 池由 crawlrs `DashMap` 承载
//!   （`KeyedCircuitBreaker` 登记为库侧需求，补齐后二次收敛）。
//! - **同步读点**：`record_*` 为 async 库调用，调用点转 async；`is_open`
//!   保持同步签名——record 路径异步更新后把库 `state()` 镜像到
//!   `AtomicU8`（+ Open 进入时刻），selector 同步读镜像（禁止 sync 上下文
//!   block_on）。镜像的 Open→HalfOpen 冷却迁移与库惰性迁移同规则
//!   （均以 `recovery_timeout`/`half_open_interval` 计）。
//! - **语义差异（显式接受，CHANGELOG 已标注）**：滑窗绝对失败计数
//!   （`failure_timestamps` 窗口内计数触发）→ 库累计比例触发
//!   （`total >= min_requests` 且 `failures/total >= 0.5`）；映射取
//!   `min_requests = failure_threshold`、比例 0.5，保留「连续 N 次失败
//!   必跳闸」性质；混合流量下灵敏度提高（行为变化）。
//! - **保留 crawlrs 侧**：`CircuitConfig` 配置面（`failure_window` 仅用于
//!   统计口径）、`CircuitStats` 统计（库无计数器面）、engine 标签指标
//!   埋点（`circuit_breaker_*` 系列）。`set_config` 只影响之后新建的
//!   引擎条目（库 config 构造后不可变；生产无运行时改配调用方）。

use dashmap::DashMap;
use limiteron::circuit::{ManualCircuitBreaker, ManualCircuitBreakerConfig};
use limiteron::error::CircuitState;
#[cfg(feature = "metrics")]
use metrics::{counter, gauge};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 熔断器配置
#[derive(Clone, Debug)]
pub struct CircuitConfig {
    /// 失败阈值（连续失败次数口径；库化后同时作为比例触发的采样下限）
    pub failure_threshold: u32,
    /// 恢复超时时间（Open→HalfOpen 冷却时长，映射库 half_open_interval）
    pub recovery_timeout: Duration,
    /// 失败时间窗口（仅保留于统计口径 `CircuitStats::failure_count`；
    /// 库状态机为累计比例触发，不使用窗口淘汰）
    pub failure_window: Duration,
}

impl Default for CircuitConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(30),
            failure_window: Duration::from_secs(60),
        }
    }
}

/// 库熔断比例触发阈值（累计失败占比）。
///
/// 与库默认值对齐；配合 `min_requests = failure_threshold` 保留
/// 「连续 N 次失败必跳闸」的迁移前性质。
const LIBRARY_FAILURE_RATIO: f64 = 0.5;

/// 镜像状态编码（`AtomicU8`）：Closed / Open / HalfOpen。
const MIRROR_CLOSED: u8 = 0;
const MIRROR_OPEN: u8 = 1;
const MIRROR_HALF_OPEN: u8 = 2;

/// 熔断器状态枚举
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    /// 关闭状态
    Closed,
    /// 打开状态
    Open,
    /// 半开状态
    HalfOpen,
}

/// 熔断器统计信息
#[derive(Clone, Debug, Default)]
pub struct CircuitStats {
    /// 是否处于打开状态
    pub is_open: bool,
    /// 时间窗口内的失败次数
    pub failure_count: u32,
    /// 总请求数
    pub total_requests: u64,
    /// 总失败数
    pub total_failures: u64,
    /// 总成功数
    pub total_successes: u64,
}

/// 每引擎统计簿（crawlrs 侧保留：库 `ManualCircuitBreaker` 无计数器面）
#[derive(Debug, Default)]
struct EngineLedger {
    total_requests: u64,
    total_failures: u64,
    total_successes: u64,
    /// 窗口内失败时间戳（仅服务 `CircuitStats::failure_count` 口径）
    failure_timestamps: VecDeque<Instant>,
}

/// 每引擎同步读镜像：record 路径异步更新后写入，selector 同步读。
#[derive(Debug, Default)]
struct EngineMirror {
    /// 状态编码（MIRROR_*）
    state: AtomicU8,
    /// 进入 Open 的 unix 毫秒（冷却判定用；0 表示未打开）
    opened_at_ms: AtomicU64,
}

/// crawlrs `CircuitConfig` → 库 `ManualCircuitBreakerConfig` 映射。
///
/// `failure_threshold`（绝对计数）映射为采样下限 `min_requests`，
/// 触发判定改经库累计比例（`LIBRARY_FAILURE_RATIO`）；
/// `half_open_max_successes = 1` 保留迁移前「HalfOpen 一次成功即闭合」语义。
fn to_library_config(config: &CircuitConfig) -> ManualCircuitBreakerConfig {
    ManualCircuitBreakerConfig {
        failure_threshold: LIBRARY_FAILURE_RATIO,
        min_requests: (config.failure_threshold as u64).max(1),
        half_open_interval: config.recovery_timeout,
        half_open_max_successes: 1,
    }
}

/// 当前 unix 毫秒（镜像 Open 时刻用）
fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 镜像编码 → crawlrs `Status`（指标埋点用）
fn mirror_code_to_status(code: u8) -> Status {
    match code {
        MIRROR_OPEN => Status::Open,
        MIRROR_HALF_OPEN => Status::HalfOpen,
        _ => Status::Closed,
    }
}

/// 熔断器（limiteron `ManualCircuitBreaker` 薄封装）
///
/// 实现熔断器模式，防止系统因故障而崩溃。
///
/// PERF-009 精神延续：按引擎粒度隔离——库熔断器、镜像、统计簿均为
/// per-key DashMap 条目，不同引擎的状态变更互不干扰。
#[derive(Clone)]
pub struct CircuitBreaker {
    /// per-engine 库熔断器（键控池由 crawlrs DashMap 承载）
    breakers: Arc<DashMap<String, Arc<ManualCircuitBreaker>>>,
    /// per-engine 同步读镜像（selector 读，无 await）
    mirrors: Arc<DashMap<String, Arc<EngineMirror>>>,
    /// per-engine 统计簿（engine 标签指标保留 crawlrs 埋点）
    ledgers: Arc<DashMap<String, RwLock<EngineLedger>>>,
    /// 配置映射
    configs: Arc<RwLock<HashMap<String, CircuitConfig>>>,
    /// 默认配置
    default_config: CircuitConfig,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

impl CircuitBreaker {
    /// 创建新的熔断器实例
    pub fn new() -> Self {
        Self {
            breakers: Arc::new(DashMap::with_capacity(8)),
            mirrors: Arc::new(DashMap::with_capacity(8)),
            ledgers: Arc::new(DashMap::with_capacity(8)),
            configs: Arc::new(RwLock::new(HashMap::with_capacity(8))),
            default_config: CircuitConfig::default(),
        }
    }

    /// 使用指定默认配置创建熔断器实例
    pub fn with_default_config(config: CircuitConfig) -> Self {
        Self {
            breakers: Arc::new(DashMap::with_capacity(8)),
            mirrors: Arc::new(DashMap::with_capacity(8)),
            ledgers: Arc::new(DashMap::with_capacity(8)),
            configs: Arc::new(RwLock::new(HashMap::with_capacity(8))),
            default_config: config,
        }
    }

    /// 设置引擎配置
    ///
    /// 注意：库 config 构造后不可变，此处只影响之后**新建**的引擎条目
    /// （已存在条目的库熔断器保持原配置）。
    pub fn set_config(&self, engine_name: &str, config: CircuitConfig) {
        let mut configs = self.configs.write();
        configs.insert(engine_name.to_string(), config);
    }

    /// 获取引擎配置
    fn get_config(&self, engine_name: &str) -> CircuitConfig {
        let configs = self.configs.read();
        configs
            .get(engine_name)
            .cloned()
            .unwrap_or(self.default_config.clone())
    }

    /// 获取（或新建）引擎的库熔断器（键控池，entry API 保证同 key 单实例）
    fn get_or_create_breaker(&self, engine_name: &str) -> Arc<ManualCircuitBreaker> {
        let entry = self
            .breakers
            .entry(engine_name.to_string())
            .or_insert_with(|| {
                let config = self.get_config(engine_name);
                Arc::new(ManualCircuitBreaker::new(to_library_config(&config)))
            });
        Arc::clone(entry.value())
    }

    /// 获取（或新建）引擎镜像
    fn get_or_create_mirror(&self, engine_name: &str) -> Arc<EngineMirror> {
        let entry = self.mirrors.entry(engine_name.to_string()).or_default();
        Arc::clone(entry.value())
    }

    /// 检查熔断器是否打开（同步读镜像，禁止 sync 上下文 block_on）
    ///
    /// 镜像 Open 且冷却期满时同步迁移为 HalfOpen（与库 `state()` 惰性
    /// 迁移同规则），返回 false 放行。
    pub fn is_open(&self, engine_name: &str) -> bool {
        // 使用 get() 而非 entry().or_insert_with()，
        // 避免为每个查询的 engine_name 创建条目导致内存泄漏。
        // 未找到镜像时视为新引擎，返回 false（默认关闭）。
        let Some(mirror) = self
            .mirrors
            .get(engine_name)
            .map(|entry| Arc::clone(entry.value()))
        else {
            return false;
        };

        match mirror.state.load(Ordering::Acquire) {
            MIRROR_CLOSED => false,
            MIRROR_HALF_OPEN => false,
            MIRROR_OPEN => {
                let opened_at_ms = mirror.opened_at_ms.load(Ordering::Acquire);
                let recovery_ms = self.get_config(engine_name).recovery_timeout.as_millis() as u64;
                if opened_at_ms > 0 && now_unix_ms().saturating_sub(opened_at_ms) >= recovery_ms {
                    // 冷却期满：镜像转 HalfOpen（放行探测请求）
                    mirror.state.store(MIRROR_HALF_OPEN, Ordering::Release);
                    mirror.opened_at_ms.store(0, Ordering::Release);
                    self.update_status_metric(engine_name, Status::HalfOpen);
                    false
                } else {
                    #[cfg(feature = "metrics")]
                    counter!("circuit_breaker_rejected_total", "engine" => engine_name.to_string())
                        .increment(1);
                    true
                }
            }
            _ => false,
        }
    }

    /// 记录成功（async：库调用 + 镜像刷新）
    pub async fn record_success(&self, engine_name: &str) {
        let breaker = self.get_or_create_breaker(engine_name);
        // 先执行库惰性冷却迁移（与 selector 同步读同规则），再上报成功，
        // 保证冷却期满后的首个成功即计入 HalfOpen 闭合判定
        breaker.state().await;
        breaker.record_success().await;

        self.record_ledger(engine_name, true);
        self.refresh_mirror(engine_name, &breaker).await;
    }

    /// 记录失败（async：库调用 + 镜像刷新）
    pub async fn record_failure(&self, engine_name: &str) {
        let breaker = self.get_or_create_breaker(engine_name);
        breaker.state().await;
        breaker.record_failure().await;

        self.record_ledger(engine_name, false);
        self.refresh_mirror(engine_name, &breaker).await;
    }

    /// crawlrs 侧统计簿累计 + engine 标签指标埋点（库无计数器面）
    fn record_ledger(&self, engine_name: &str, success: bool) {
        let entry = self
            .ledgers
            .entry(engine_name.to_string())
            .or_insert_with(|| RwLock::new(EngineLedger::default()));
        let mut ledger = entry.write();
        ledger.total_requests += 1;
        if success {
            ledger.total_successes += 1;
            #[cfg(feature = "metrics")]
            counter!("circuit_breaker_successes_total", "engine" => engine_name.to_string())
                .increment(1);
        } else {
            ledger.total_failures += 1;
            ledger.failure_timestamps.push_back(Instant::now());
            // 统计窗口淘汰（仅服务 failure_count 口径，不参与库状态机判定）
            let window = self.get_config(engine_name).failure_window;
            let now = Instant::now();
            while let Some(front) = ledger.failure_timestamps.front() {
                if now.duration_since(*front) > window {
                    ledger.failure_timestamps.pop_front();
                } else {
                    break;
                }
            }
            #[cfg(feature = "metrics")]
            counter!("circuit_breaker_failures_total", "engine" => engine_name.to_string())
                .increment(1);
        }
        #[cfg(feature = "metrics")]
        counter!("circuit_breaker_requests_total", "engine" => engine_name.to_string())
            .increment(1);
    }

    /// 库状态 → 同步镜像（record 路径调用；状态迁移时刷新 gauge 指标）
    async fn refresh_mirror(&self, engine_name: &str, breaker: &ManualCircuitBreaker) {
        let state = breaker.state().await;
        let mirror = self.get_or_create_mirror(engine_name);
        let (code, opened_at_ms) = match state {
            CircuitState::Closed => (MIRROR_CLOSED, 0u64),
            CircuitState::Open => (MIRROR_OPEN, now_unix_ms()),
            CircuitState::HalfOpen => (MIRROR_HALF_OPEN, 0u64),
        };
        let previous = mirror.state.swap(code, Ordering::AcqRel);
        mirror.opened_at_ms.store(opened_at_ms, Ordering::Release);

        if previous != code {
            self.update_status_metric(engine_name, mirror_code_to_status(code));
            // HalfOpen→Closed 时清空窗口失败戳（迁移前 record_success 语义）
            if code == MIRROR_CLOSED {
                if let Some(ledger) = self.ledgers.get(engine_name) {
                    ledger.write().failure_timestamps.clear();
                }
            }
        }
    }

    /// 获取引擎的熔断统计信息
    pub async fn get_stats(&self, engine_name: &str) -> CircuitStats {
        let is_open = self
            .mirrors
            .get(engine_name)
            .map(|m| m.value().state.load(Ordering::Acquire) == MIRROR_OPEN)
            .unwrap_or(false);

        if let Some(ledger) = self.ledgers.get(engine_name) {
            let ledger = ledger.read();
            CircuitStats {
                is_open,
                failure_count: ledger.failure_timestamps.len() as u32,
                total_requests: ledger.total_requests,
                total_failures: ledger.total_failures,
                total_successes: ledger.total_successes,
            }
        } else {
            CircuitStats {
                is_open,
                ..Default::default()
            }
        }
    }

    /// 更新状态指标
    #[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
    fn update_status_metric(&self, engine_name: &str, status: Status) {
        let val = match status {
            Status::Closed => 0.0,
            Status::Open => 1.0,
            Status::HalfOpen => 0.5,
        };
        #[cfg(feature = "metrics")]
        gauge!("circuit_breaker_status", "engine" => engine_name.to_string()).set(val);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    // === Config tests ===

    #[test]
    fn test_circuit_config_default() {
        let config = CircuitConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.recovery_timeout, Duration::from_secs(30));
        assert_eq!(config.failure_window, Duration::from_secs(60));
    }

    #[test]
    fn test_circuit_config_clone() {
        let config = CircuitConfig {
            failure_threshold: 10,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(120),
        };
        let cloned = config.clone();
        assert_eq!(config.failure_threshold, cloned.failure_threshold);
        assert_eq!(config.recovery_timeout, cloned.recovery_timeout);
        assert_eq!(config.failure_window, cloned.failure_window);
    }

    // === CircuitBreaker creation tests ===

    #[test]
    fn test_circuit_breaker_new() {
        let cb = CircuitBreaker::new();
        // A freshly-created breaker should not be open for an unknown engine
        assert!(!cb.is_open("unknown_engine"));
    }

    #[test]
    fn test_circuit_breaker_default() {
        let cb = CircuitBreaker::default();
        assert!(!cb.is_open("any_engine"));
    }

    #[test]
    fn test_circuit_breaker_with_default_config() {
        let config = CircuitConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_secs(10),
            failure_window: Duration::from_secs(30),
        };
        let cb = CircuitBreaker::with_default_config(config);
        assert!(!cb.is_open("engine_a"));
    }

    #[tokio::test]
    async fn test_set_config_uses_custom_threshold() {
        let cb = CircuitBreaker::new();
        let config = CircuitConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_secs(30),
            failure_window: Duration::from_secs(60),
        };
        cb.set_config("fast_trip", config);

        // Two failures should trip the breaker (threshold = 2)
        cb.record_failure("fast_trip").await;
        assert!(!cb.is_open("fast_trip")); // 1 failure, still closed
        cb.record_failure("fast_trip").await;
        assert!(cb.is_open("fast_trip")); // 2 failures, now open
    }

    #[tokio::test]
    async fn test_set_config_does_not_affect_other_engines() {
        let cb = CircuitBreaker::new();
        let config = CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(30),
            failure_window: Duration::from_secs(60),
        };
        cb.set_config("strict", config);

        // "other" engine should use default threshold (5)
        cb.record_failure("other").await;
        assert!(!cb.is_open("other")); // 1 < 5, still closed
    }

    // === Closed state tests ===

    #[test]
    fn test_closed_state_not_open() {
        let cb = CircuitBreaker::new();
        assert!(!cb.is_open("engine"));
    }

    #[tokio::test]
    async fn test_closed_state_records_success() {
        let cb = CircuitBreaker::new();
        cb.record_success("engine").await;
        let stats = cb.get_stats("engine").await;
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.total_successes, 1);
        assert_eq!(stats.total_failures, 0);
        assert!(!stats.is_open);
    }

    #[tokio::test]
    async fn test_closed_state_records_failure() {
        let cb = CircuitBreaker::new();
        cb.record_failure("engine").await;
        let stats = cb.get_stats("engine").await;
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.total_failures, 1);
        assert_eq!(stats.total_successes, 0);
        assert!(!stats.is_open);
    }

    // === Open state tests ===

    #[tokio::test]
    async fn test_open_state_after_threshold() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });

        cb.record_failure("engine").await;
        cb.record_failure("engine").await;
        assert!(!cb.is_open("engine")); // 2 < 3

        cb.record_failure("engine").await;
        assert!(cb.is_open("engine")); // 3 >= 3, now open
    }

    #[tokio::test]
    async fn test_open_state_default_threshold() {
        let cb = CircuitBreaker::new(); // default threshold = 5

        for _ in 0..4 {
            cb.record_failure("engine").await;
            assert!(!cb.is_open("engine"));
        }

        cb.record_failure("engine").await;
        assert!(cb.is_open("engine")); // 5 >= 5
    }

    #[tokio::test]
    async fn test_open_state_blocks_requests() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });

        cb.record_failure("engine").await;
        assert!(cb.is_open("engine"));
        // Subsequent calls should still return true (open)
        assert!(cb.is_open("engine"));
        assert!(cb.is_open("engine"));
    }

    // === Half-open state tests ===

    #[tokio::test]
    async fn test_half_open_after_recovery_timeout() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(500),
            failure_window: Duration::from_secs(60),
        });

        cb.record_failure("engine").await;
        assert!(cb.is_open("engine"));

        // Wait for recovery timeout to elapse
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // is_open should transition to HalfOpen and return false
        assert!(!cb.is_open("engine"));
    }

    #[tokio::test]
    async fn test_half_open_success_closes_circuit() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(500),
            failure_window: Duration::from_secs(60),
        });

        // Trip the breaker
        cb.record_failure("engine").await;
        assert!(cb.is_open("engine"));

        // Wait for recovery timeout
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Trigger transition to half-open
        assert!(!cb.is_open("engine"));

        // Success in half-open should close the circuit
        cb.record_success("engine").await;
        assert!(!cb.is_open("engine"));

        let stats = cb.get_stats("engine").await;
        assert!(!stats.is_open);
        assert_eq!(stats.failure_count, 0); // timestamps cleared on close
    }

    #[tokio::test]
    async fn test_half_open_failure_reopens_circuit() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(500),
            failure_window: Duration::from_secs(60),
        });

        // Trip the breaker
        cb.record_failure("engine").await;
        assert!(cb.is_open("engine"));

        // Wait for recovery timeout
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Trigger transition to half-open
        assert!(!cb.is_open("engine"));

        // Failure in half-open should reopen the circuit
        cb.record_failure("engine").await;
        assert!(cb.is_open("engine"));
    }

    #[tokio::test]
    async fn test_half_open_not_triggered_before_timeout() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });

        cb.record_failure("engine").await;
        assert!(cb.is_open("engine"));

        // Without waiting, should still be open
        assert!(cb.is_open("engine"));
    }

    // === Failure window tests ===

    /// 行为变化标注（S6 库化）：统计窗口仍淘汰旧失败戳（failure_count == 1），
    /// 但库状态机为**累计比例触发**（计数不随窗口衰减）——第 3 次失败后
    /// total=3 ≥ min_requests=3 且失败占比 3/3=1.0 ≥ 0.5，熔断器跳闸
    /// （迁移前滑窗绝对计数口径下不会跳闸）。
    #[tokio::test]
    async fn test_cumulative_ratio_trips_after_stats_window_eviction() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_millis(50),
        });

        // Record 2 failures (total < min_requests=3, no trip)
        cb.record_failure("engine").await;
        cb.record_failure("engine").await;
        assert!(!cb.is_open("engine"));

        // Wait for failures to age out of the stats window
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Record 1 more failure — library cumulative ratio trips the breaker
        cb.record_failure("engine").await;
        assert!(
            cb.is_open("engine"),
            "cumulative ratio trigger: 3/3 failures >= 0.5 with total >= min_requests"
        );

        // Stats window still evicts old failures (count == 1)
        let stats = cb.get_stats("engine").await;
        assert_eq!(stats.failure_count, 1);
    }

    #[tokio::test]
    async fn test_failure_window_keeps_recent_failures() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });

        cb.record_failure("engine").await;
        cb.record_failure("engine").await;
        cb.record_failure("engine").await;
        assert!(cb.is_open("engine")); // 3 failures within window

        let stats = cb.get_stats("engine").await;
        assert_eq!(stats.failure_count, 3);
    }

    // === Statistics tests ===

    #[tokio::test]
    async fn test_get_stats_unknown_engine() {
        let cb = CircuitBreaker::new();
        let stats = cb.get_stats("nonexistent").await;
        assert!(!stats.is_open);
        assert_eq!(stats.failure_count, 0);
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.total_failures, 0);
        assert_eq!(stats.total_successes, 0);
    }

    #[tokio::test]
    async fn test_get_stats_tracks_totals() {
        let cb = CircuitBreaker::new();

        cb.record_success("engine").await;
        cb.record_success("engine").await;
        cb.record_failure("engine").await;

        let stats = cb.get_stats("engine").await;
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.total_successes, 2);
        assert_eq!(stats.total_failures, 1);
    }

    #[tokio::test]
    async fn test_get_stats_is_open_reflects_status() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });

        let stats_before = cb.get_stats("engine").await;
        assert!(!stats_before.is_open);

        cb.record_failure("engine").await;

        let stats_after = cb.get_stats("engine").await;
        assert!(stats_after.is_open);
    }

    // === Multiple engine isolation tests ===

    #[tokio::test]
    async fn test_multiple_engines_are_isolated() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });

        cb.record_failure("engine_a").await;
        assert!(cb.is_open("engine_a"));
        assert!(!cb.is_open("engine_b")); // engine_b is independent
    }

    #[tokio::test]
    async fn test_multiple_engines_stats_isolated() {
        let cb = CircuitBreaker::new();

        cb.record_success("engine_a").await;
        cb.record_failure("engine_b").await;

        let stats_a = cb.get_stats("engine_a").await;
        let stats_b = cb.get_stats("engine_b").await;

        assert_eq!(stats_a.total_successes, 1);
        assert_eq!(stats_a.total_failures, 0);
        assert_eq!(stats_b.total_successes, 0);
        assert_eq!(stats_b.total_failures, 1);
    }

    // === Concurrent access tests ===

    #[tokio::test]
    async fn test_concurrent_record_failures() {
        let cb = Arc::new(CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 100,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        }));

        let mut handles = Vec::new();
        for _ in 0..10 {
            let cb_clone = Arc::clone(&cb);
            handles.push(tokio::spawn(async move {
                for _ in 0..10 {
                    cb_clone.record_failure("concurrent_engine").await;
                }
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let stats = cb.get_stats("concurrent_engine").await;
        assert_eq!(stats.total_failures, 100);
        assert!(stats.is_open); // 100 >= 100 threshold
    }

    #[tokio::test]
    async fn test_concurrent_record_successes() {
        let cb = Arc::new(CircuitBreaker::new());

        let mut handles = Vec::new();
        for _ in 0..10 {
            let cb_clone = Arc::clone(&cb);
            handles.push(tokio::spawn(async move {
                for _ in 0..10 {
                    cb_clone.record_success("concurrent_engine").await;
                }
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let stats = cb.get_stats("concurrent_engine").await;
        assert_eq!(stats.total_successes, 100);
        assert_eq!(stats.total_requests, 100);
    }

    #[tokio::test]
    async fn test_concurrent_mixed_operations() {
        let cb = Arc::new(CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1000,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        }));

        let mut handles = Vec::new();

        // 5 tasks recording failures
        for _ in 0..5 {
            let cb_clone = Arc::clone(&cb);
            handles.push(tokio::spawn(async move {
                for _ in 0..20 {
                    cb_clone.record_failure("mixed_engine").await;
                }
            }));
        }

        // 5 tasks recording successes
        for _ in 0..5 {
            let cb_clone = Arc::clone(&cb);
            handles.push(tokio::spawn(async move {
                for _ in 0..20 {
                    cb_clone.record_success("mixed_engine").await;
                }
            }));
        }

        // 5 tasks checking is_open
        for _ in 0..5 {
            let cb_clone = Arc::clone(&cb);
            handles.push(tokio::spawn(async move {
                for _ in 0..20 {
                    let _ = cb_clone.is_open("mixed_engine");
                }
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let stats = cb.get_stats("mixed_engine").await;
        // 5*20 failures + 5*20 successes = 200 total requests
        assert_eq!(stats.total_requests, 200);
        assert_eq!(stats.total_failures, 100);
        assert_eq!(stats.total_successes, 100);
    }

    // === Full lifecycle test ===

    #[tokio::test]
    async fn test_full_lifecycle_closed_open_half_open_closed() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_millis(500),
            failure_window: Duration::from_secs(60),
        });

        // 1. Start closed
        assert!(!cb.is_open("lifecycle"));

        // 2. Record failures to trip the breaker -> Open
        cb.record_failure("lifecycle").await;
        cb.record_failure("lifecycle").await;
        assert!(cb.is_open("lifecycle"));

        // 3. Wait for recovery timeout -> HalfOpen on next is_open call
        tokio::time::sleep(Duration::from_millis(1000)).await;
        assert!(!cb.is_open("lifecycle")); // transitions to half-open

        // 4. Record success -> Closed
        cb.record_success("lifecycle").await;
        assert!(!cb.is_open("lifecycle"));

        // 5. Verify stats
        let stats = cb.get_stats("lifecycle").await;
        assert!(!stats.is_open);
        assert_eq!(stats.total_requests, 3); // 2 failures + 1 success
        assert_eq!(stats.total_failures, 2);
        assert_eq!(stats.total_successes, 1);
        assert_eq!(stats.failure_count, 0); // cleared on close
    }

    #[tokio::test]
    async fn test_full_lifecycle_closed_open_half_open_open() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_millis(500),
            failure_window: Duration::from_secs(60),
        });

        // 1. Trip the breaker -> Open
        cb.record_failure("lifecycle").await;
        cb.record_failure("lifecycle").await;
        assert!(cb.is_open("lifecycle"));

        // 2. Wait for recovery timeout -> HalfOpen
        tokio::time::sleep(Duration::from_millis(1000)).await;
        assert!(!cb.is_open("lifecycle"));

        // 3. Record failure -> Open again
        cb.record_failure("lifecycle").await;
        assert!(cb.is_open("lifecycle"));

        // 4. Verify stats
        let stats = cb.get_stats("lifecycle").await;
        assert!(stats.is_open);
        assert_eq!(stats.total_failures, 3);
    }

    // === Status enum tests ===

    /// 镜像原子值与库 `state()` 在 record 完成后一致（S6 验收：一致性断言）
    #[tokio::test]
    async fn test_mirror_consistent_with_library_state_after_records() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });

        // 关闭态：镜像与库一致
        cb.record_success("engine").await;
        let breaker = cb.get_or_create_breaker("engine");
        assert_eq!(breaker.state().await, CircuitState::Closed);
        assert!(!cb.is_open("engine"));

        // 连续失败达阈值 → Open：镜像与库一致
        cb.record_failure("engine").await;
        cb.record_failure("engine").await;
        assert_eq!(breaker.state().await, CircuitState::Open);
        assert!(
            cb.is_open("engine"),
            "镜像读必须立即生效（selector 同步剔除）"
        );

        // 冷却期满 → 镜像同步迁移 HalfOpen，与库惰性迁移一致
        let cb2 = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(50),
            failure_window: Duration::from_secs(60),
        });
        cb2.record_failure("engine").await;
        assert!(cb2.is_open("engine"));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!cb2.is_open("engine"), "冷却后镜像转 HalfOpen 放行");
        let breaker2 = cb2.get_or_create_breaker("engine");
        assert_eq!(breaker2.state().await, CircuitState::HalfOpen);

        // HalfOpen 一次成功即闭合（half_open_max_successes = 1），镜像同步
        cb2.record_success("engine").await;
        assert_eq!(breaker2.state().await, CircuitState::Closed);
        assert!(!cb2.is_open("engine"));
    }

    #[test]
    fn test_status_equality() {
        assert_eq!(Status::Closed, Status::Closed);
        assert_eq!(Status::Open, Status::Open);
        assert_eq!(Status::HalfOpen, Status::HalfOpen);
        assert_ne!(Status::Closed, Status::Open);
        assert_ne!(Status::Open, Status::HalfOpen);
        assert_ne!(Status::Closed, Status::HalfOpen);
    }

    #[test]
    fn test_status_copy() {
        let s1 = Status::Open;
        let s2 = s1; // Copy
        assert_eq!(s1, s2);
    }

    // === CircuitStats default test ===

    #[test]
    fn test_circuit_stats_default() {
        let stats = CircuitStats::default();
        assert!(!stats.is_open);
        assert_eq!(stats.failure_count, 0);
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.total_failures, 0);
        assert_eq!(stats.total_successes, 0);
    }

    // === record_failure on already-open breaker ===

    #[tokio::test]
    async fn test_record_failure_on_open_breaker_is_noop_status() {
        // When the breaker is already Open, recording another failure
        // should not change the status (Status::Open => {} arm)
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });

        // Trip the breaker
        cb.record_failure("engine").await;
        assert!(cb.is_open("engine"));

        // Record another failure on the already-open breaker
        cb.record_failure("engine").await;
        assert!(cb.is_open("engine")); // Still open

        // Verify stats: total_failures = 2, total_requests = 2
        let stats = cb.get_stats("engine").await;
        assert!(stats.is_open);
        assert_eq!(stats.total_failures, 2);
        assert_eq!(stats.total_requests, 2);
    }

    // === record_success on closed breaker (no state change) ===

    #[tokio::test]
    async fn test_record_success_on_closed_breaker_does_not_change_state() {
        // Success on a Closed breaker should just update stats, not call
        // update_status_metric or change state
        let cb = CircuitBreaker::new();

        cb.record_success("engine").await;
        cb.record_success("engine").await;

        let stats = cb.get_stats("engine").await;
        assert!(!stats.is_open);
        assert_eq!(stats.total_successes, 2);
        assert_eq!(stats.total_requests, 2);
    }

    // === CircuitBreaker Clone ===

    #[tokio::test]
    async fn test_circuit_breaker_clone_shares_state() {
        // CircuitBreaker implements Clone; cloned instances should share state
        // via Arc
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });
        let cb_clone = cb.clone();

        cb.record_failure("engine").await;
        assert!(cb_clone.is_open("engine")); // Clone sees the same state
    }

    #[tokio::test]
    async fn test_is_open_returns_false_when_already_half_open() {
        // Cover line 202: `Status::HalfOpen => false` in is_open().
        // Existing tests only cover the Open -> HalfOpen transition
        // (lines 192-194) which returns false immediately. To hit line 202,
        // is_open() must be called when the state is ALREADY HalfOpen
        // (set by a previous is_open call after recovery timeout).
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(500),
            failure_window: Duration::from_secs(60),
        });

        cb.record_failure("engine").await;
        assert!(cb.is_open("engine")); // Open -> true

        // Wait for recovery timeout to elapse
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // First call transitions Open -> HalfOpen (lines 192-194), returns false
        assert!(!cb.is_open("engine"));

        // Second call: status is already HalfOpen, hits line 202
        assert!(!cb.is_open("engine"));
    }
}
