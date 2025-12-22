// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use metrics::{counter, gauge};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
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
            states: Arc::new(RwLock::new(HashMap::new())),
            configs: Arc::new(RwLock::new(HashMap::new())),
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
            states: Arc::new(RwLock::new(HashMap::new())),
            configs: Arc::new(RwLock::new(HashMap::new())),
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
        let mut configs = self.configs.write().unwrap();
        configs.insert(engine_name.to_string(), config);
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
        let configs = self.configs.read().unwrap();
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

        let mut states = self.states.write().unwrap();
        let state = states
            .entry(engine_name.to_string())
            .or_insert(CircuitState {
                status: Status::Closed,
                failure_timestamps: VecDeque::new(),
                last_failure: None,
                total_requests: 0,
                total_failures: 0,
                total_successes: 0,
            });

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
        let mut states = self.states.write().unwrap();
        if let Some(state) = states.get_mut(engine_name) {
            state.total_requests += 1;
            state.total_successes += 1;

            counter!("circuit_breaker_requests_total", "engine" => engine_name.to_string())
                .increment(1);
            counter!("circuit_breaker_successes_total", "engine" => engine_name.to_string())
                .increment(1);

            if state.status == Status::HalfOpen {
                state.status = Status::Closed;
                state.failure_timestamps.clear();
                self.update_status_metric(engine_name, Status::Closed);
            }
        }
    }

    /// 记录失败
    ///
    /// # 参数
    ///
    /// * `engine_name` - 引擎名称
    pub fn record_failure(&self, engine_name: &str) {
        let config = self.get_config(engine_name);

        let mut states = self.states.write().unwrap();
        let state = states
            .entry(engine_name.to_string())
            .or_insert(CircuitState {
                status: Status::Closed,
                failure_timestamps: VecDeque::new(),
                last_failure: None,
                total_requests: 0,
                total_failures: 0,
                total_successes: 0,
            });

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

        counter!("circuit_breaker_requests_total", "engine" => engine_name.to_string())
            .increment(1);
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
        let states = self.states.read().unwrap();
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
        gauge!("circuit_breaker_status", "engine" => engine_name.to_string()).set(val);
    }
}
