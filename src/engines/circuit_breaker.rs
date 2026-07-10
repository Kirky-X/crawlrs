// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(feature = "metrics")]
use metrics::{counter, gauge};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use std::collections::VecDeque;

/// 熔断器配置
#[derive(Clone, Debug)]
pub struct CircuitConfig {
    /// 失败阈值
    pub failure_threshold: u32,
    /// 恢复超时时间
    pub recovery_timeout: Duration,
    /// 失败时间窗口
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

/// 熔断器状态
#[derive(Clone, Debug)]
struct CircuitState {
    /// 当前状态
    status: Status,
    /// 失败时间戳
    failure_timestamps: VecDeque<Instant>,
    /// 上次失败时间
    last_failure: Option<Instant>,
    // Statistics
    /// 总请求数
    total_requests: u64,
    /// 总失败数
    total_failures: u64,
    /// 总成功数
    total_successes: u64,
}

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

/// 熔断器
///
/// 实现熔断器模式，防止系统因故障而崩溃
#[derive(Clone)]
pub struct CircuitBreaker {
    /// 状态映射
    states: Arc<RwLock<HashMap<String, CircuitState>>>,
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
    ///
    /// # 返回值
    ///
    /// 返回新的熔断器实例
    pub fn new() -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::with_capacity(8))),
            configs: Arc::new(RwLock::new(HashMap::with_capacity(8))),
            default_config: CircuitConfig::default(),
        }
    }

    /// 使用指定默认配置创建熔断器实例
    ///
    /// # 参数
    ///
    /// * `config` - 默认配置
    ///
    /// # 返回值
    ///
    /// 返回新的熔断器实例
    pub fn with_default_config(config: CircuitConfig) -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::with_capacity(8))),
            configs: Arc::new(RwLock::new(HashMap::with_capacity(8))),
            default_config: config,
        }
    }

    /// 设置引擎配置
    ///
    /// # 参数
    ///
    /// * `engine_name` - 引擎名称
    /// * `config` - 配置
    pub fn set_config(&self, engine_name: &str, config: CircuitConfig) {
        let mut configs = self.configs.write();
        configs.insert(engine_name.to_string(), config);
    }

    /// 创建默认的熔断器状态
    fn create_default_state() -> CircuitState {
        CircuitState {
            status: Status::Closed,
            failure_timestamps: VecDeque::new(),
            last_failure: None,
            total_requests: 0,
            total_failures: 0,
            total_successes: 0,
        }
    }

    /// 获取引擎配置
    ///
    /// # 参数
    ///
    /// * `engine_name` - 引擎名称
    ///
    /// # 返回值
    ///
    /// 引擎配置
    fn get_config(&self, engine_name: &str) -> CircuitConfig {
        let configs = self.configs.read();
        configs
            .get(engine_name)
            .cloned()
            .unwrap_or(self.default_config.clone())
    }

    /// 检查熔断器是否打开
    ///
    /// # 参数
    ///
    /// * `engine_name` - 引擎名称
    ///
    /// # 返回值
    ///
    /// 如果熔断器打开则返回true，否则返回false
    pub fn is_open(&self, engine_name: &str) -> bool {
        let config = self.get_config(engine_name);

        let mut states = self.states.write();
        let state = states
            .entry(engine_name.to_string())
            .or_insert(Self::create_default_state());

        match state.status {
            Status::Closed => false,
            Status::Open => {
                if let Some(last_failure) = state.last_failure {
                    if last_failure.elapsed() > config.recovery_timeout {
                        state.status = Status::HalfOpen;
                        self.update_status_metric(engine_name, Status::HalfOpen);
                        return false;
                    }
                }
                #[cfg(feature = "metrics")]
                counter!("circuit_breaker_rejected_total", "engine" => engine_name.to_string())
                    .increment(1);
                true
            }
            Status::HalfOpen => false,
        }
    }

    /// 记录成功
    ///
    /// # 参数
    ///
    /// * `engine_name` - 引擎名称
    pub fn record_success(&self, engine_name: &str) {
        let mut states = self.states.write();
        let state = states
            .entry(engine_name.to_string())
            .or_insert(Self::create_default_state());
        state.total_requests += 1;
        state.total_successes += 1;

        #[cfg(feature = "metrics")]
        counter!("circuit_breaker_requests_total", "engine" => engine_name.to_string())
            .increment(1);
        #[cfg(feature = "metrics")]
        counter!("circuit_breaker_successes_total", "engine" => engine_name.to_string())
            .increment(1);

        if state.status == Status::HalfOpen {
            state.status = Status::Closed;
            state.failure_timestamps.clear();
            self.update_status_metric(engine_name, Status::Closed);
        }
    }

    /// 记录失败
    ///
    /// # 参数
    ///
    /// * `engine_name` - 引擎名称
    pub fn record_failure(&self, engine_name: &str) {
        let config = self.get_config(engine_name);

        let mut states = self.states.write();
        let state = states
            .entry(engine_name.to_string())
            .or_insert(Self::create_default_state());

        let now = Instant::now();
        state.total_requests += 1;
        state.total_failures += 1;
        state.last_failure = Some(now);
        state.failure_timestamps.push_back(now);

        // 移除超出时间窗口的失败记录
        while let Some(front) = state.failure_timestamps.front() {
            if now.duration_since(*front) > config.failure_window {
                state.failure_timestamps.pop_front();
            } else {
                break;
            }
        }

        #[cfg(feature = "metrics")]
        counter!("circuit_breaker_requests_total", "engine" => engine_name.to_string())
            .increment(1);
        #[cfg(feature = "metrics")]
        counter!("circuit_breaker_failures_total", "engine" => engine_name.to_string())
            .increment(1);

        match state.status {
            Status::Closed => {
                if state.failure_timestamps.len() >= config.failure_threshold as usize {
                    state.status = Status::Open;
                    self.update_status_metric(engine_name, Status::Open);
                }
            }
            Status::HalfOpen => {
                state.status = Status::Open;
                self.update_status_metric(engine_name, Status::Open);
            }
            Status::Open => {}
        }
    }

    /// 获取引擎的熔断统计信息
    ///
    /// # 参数
    ///
    /// * `engine_name` - 引擎名称
    ///
    /// # 返回值
    ///
    /// 统计信息
    pub async fn get_stats(&self, engine_name: &str) -> CircuitStats {
        let states = self.states.read();
        if let Some(state) = states.get(engine_name) {
            CircuitStats {
                is_open: state.status == Status::Open,
                failure_count: state.failure_timestamps.len() as u32,
                total_requests: state.total_requests,
                total_failures: state.total_failures,
                total_successes: state.total_successes,
            }
        } else {
            CircuitStats::default()
        }
    }

    /// 更新状态指标
    ///
    /// # 参数
    ///
    /// * `engine_name` - 引擎名称
    /// * `status` - 状态
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

    #[test]
    fn test_set_config_uses_custom_threshold() {
        let cb = CircuitBreaker::new();
        let config = CircuitConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_secs(30),
            failure_window: Duration::from_secs(60),
        };
        cb.set_config("fast_trip", config);

        // Two failures should trip the breaker (threshold = 2)
        cb.record_failure("fast_trip");
        assert!(!cb.is_open("fast_trip")); // 1 failure, still closed
        cb.record_failure("fast_trip");
        assert!(cb.is_open("fast_trip")); // 2 failures, now open
    }

    #[test]
    fn test_set_config_does_not_affect_other_engines() {
        let cb = CircuitBreaker::new();
        let config = CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(30),
            failure_window: Duration::from_secs(60),
        };
        cb.set_config("strict", config);

        // "other" engine should use default threshold (5)
        cb.record_failure("other");
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
        cb.record_success("engine");
        let stats = cb.get_stats("engine").await;
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.total_successes, 1);
        assert_eq!(stats.total_failures, 0);
        assert!(!stats.is_open);
    }

    #[tokio::test]
    async fn test_closed_state_records_failure() {
        let cb = CircuitBreaker::new();
        cb.record_failure("engine");
        let stats = cb.get_stats("engine").await;
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.total_failures, 1);
        assert_eq!(stats.total_successes, 0);
        assert!(!stats.is_open);
    }

    // === Open state tests ===

    #[test]
    fn test_open_state_after_threshold() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });

        cb.record_failure("engine");
        cb.record_failure("engine");
        assert!(!cb.is_open("engine")); // 2 < 3

        cb.record_failure("engine");
        assert!(cb.is_open("engine")); // 3 >= 3, now open
    }

    #[test]
    fn test_open_state_default_threshold() {
        let cb = CircuitBreaker::new(); // default threshold = 5

        for _ in 0..4 {
            cb.record_failure("engine");
            assert!(!cb.is_open("engine"));
        }

        cb.record_failure("engine");
        assert!(cb.is_open("engine")); // 5 >= 5
    }

    #[test]
    fn test_open_state_blocks_requests() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });

        cb.record_failure("engine");
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
            recovery_timeout: Duration::from_millis(50),
            failure_window: Duration::from_secs(60),
        });

        cb.record_failure("engine");
        assert!(cb.is_open("engine"));

        // Wait for recovery timeout to elapse
        tokio::time::sleep(Duration::from_millis(80)).await;

        // is_open should transition to HalfOpen and return false
        assert!(!cb.is_open("engine"));
    }

    #[tokio::test]
    async fn test_half_open_success_closes_circuit() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(50),
            failure_window: Duration::from_secs(60),
        });

        // Trip the breaker
        cb.record_failure("engine");
        assert!(cb.is_open("engine"));

        // Wait for recovery timeout
        tokio::time::sleep(Duration::from_millis(80)).await;

        // Trigger transition to half-open
        assert!(!cb.is_open("engine"));

        // Success in half-open should close the circuit
        cb.record_success("engine");
        assert!(!cb.is_open("engine"));

        let stats = cb.get_stats("engine").await;
        assert!(!stats.is_open);
        assert_eq!(stats.failure_count, 0); // timestamps cleared on close
    }

    #[tokio::test]
    async fn test_half_open_failure_reopens_circuit() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(50),
            failure_window: Duration::from_secs(60),
        });

        // Trip the breaker
        cb.record_failure("engine");
        assert!(cb.is_open("engine"));

        // Wait for recovery timeout
        tokio::time::sleep(Duration::from_millis(80)).await;

        // Trigger transition to half-open
        assert!(!cb.is_open("engine"));

        // Failure in half-open should reopen the circuit
        cb.record_failure("engine");
        assert!(cb.is_open("engine"));
    }

    #[tokio::test]
    async fn test_half_open_not_triggered_before_timeout() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });

        cb.record_failure("engine");
        assert!(cb.is_open("engine"));

        // Without waiting, should still be open
        assert!(cb.is_open("engine"));
    }

    // === Failure window tests ===

    #[tokio::test]
    async fn test_failure_window_evicts_old_failures() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_millis(50),
        });

        // Record 2 failures (below threshold)
        cb.record_failure("engine");
        cb.record_failure("engine");
        assert!(!cb.is_open("engine"));

        // Wait for failures to age out of the window
        tokio::time::sleep(Duration::from_millis(80)).await;

        // Record 1 more failure — old ones should be evicted, so count is 1
        cb.record_failure("engine");
        assert!(!cb.is_open("engine")); // Only 1 failure in window, below threshold

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

        cb.record_failure("engine");
        cb.record_failure("engine");
        cb.record_failure("engine");
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

        cb.record_success("engine");
        cb.record_success("engine");
        cb.record_failure("engine");

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

        cb.record_failure("engine");

        let stats_after = cb.get_stats("engine").await;
        assert!(stats_after.is_open);
    }

    // === Multiple engine isolation tests ===

    #[test]
    fn test_multiple_engines_are_isolated() {
        let cb = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(60),
            failure_window: Duration::from_secs(60),
        });

        cb.record_failure("engine_a");
        assert!(cb.is_open("engine_a"));
        assert!(!cb.is_open("engine_b")); // engine_b is independent
    }

    #[tokio::test]
    async fn test_multiple_engines_stats_isolated() {
        let cb = CircuitBreaker::new();

        cb.record_success("engine_a");
        cb.record_failure("engine_b");

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
                    cb_clone.record_failure("concurrent_engine");
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
                    cb_clone.record_success("concurrent_engine");
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
                    cb_clone.record_failure("mixed_engine");
                }
            }));
        }

        // 5 tasks recording successes
        for _ in 0..5 {
            let cb_clone = Arc::clone(&cb);
            handles.push(tokio::spawn(async move {
                for _ in 0..20 {
                    cb_clone.record_success("mixed_engine");
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
            recovery_timeout: Duration::from_millis(50),
            failure_window: Duration::from_secs(60),
        });

        // 1. Start closed
        assert!(!cb.is_open("lifecycle"));

        // 2. Record failures to trip the breaker -> Open
        cb.record_failure("lifecycle");
        cb.record_failure("lifecycle");
        assert!(cb.is_open("lifecycle"));

        // 3. Wait for recovery timeout -> HalfOpen on next is_open call
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(!cb.is_open("lifecycle")); // transitions to half-open

        // 4. Record success -> Closed
        cb.record_success("lifecycle");
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
            recovery_timeout: Duration::from_millis(50),
            failure_window: Duration::from_secs(60),
        });

        // 1. Trip the breaker -> Open
        cb.record_failure("lifecycle");
        cb.record_failure("lifecycle");
        assert!(cb.is_open("lifecycle"));

        // 2. Wait for recovery timeout -> HalfOpen
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(!cb.is_open("lifecycle"));

        // 3. Record failure -> Open again
        cb.record_failure("lifecycle");
        assert!(cb.is_open("lifecycle"));

        // 4. Verify stats
        let stats = cb.get_stats("lifecycle").await;
        assert!(stats.is_open);
        assert_eq!(stats.total_failures, 3);
    }

    // === Status enum tests ===

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
}
