// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 内存感知调度器
//!
//! 基于 `SystemMonitorTrait` 提供的内存使用率，将系统划分为三态
//! （Normal / Pressure / Critical），并对入队任务给出三种准入决策
//! （Proceed / Defer / Reschedule）。Critical 状态持续超过阈值时
//! 通过 watch channel 发出优雅关闭信号。
//!
//! 设计参考：crawl4ai `async_dispatcher.py::MemoryAdaptiveDispatcher`。
//! 数据源复用 `infrastructure::observability::metrics::SystemMonitorTrait`
//! （规则 5/7：不重复造 sysinfo 采集）。

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::watch;

use crate::infrastructure::observability::metrics::SystemMonitorTrait;

/// 内存状态机三态
///
/// - [`MemoryState::Normal`]：内存使用率低于 `pressure_threshold`
/// - [`MemoryState::Pressure`]：介于 `pressure_threshold` 与 `critical_threshold` 之间
/// - [`MemoryState::Critical`]：达到或超过 `critical_threshold`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryState {
    /// 正常：可继续接纳新任务
    Normal,
    /// 压力：暂缓接纳新任务
    Pressure,
    /// 临界：拒绝新任务并触发重排
    Critical,
}

/// 准入决策
///
/// 由 [`MemoryScheduler::admit`] 根据当前 [`MemoryState`] 给出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// 放行：进入并发获取流程
    Proceed,
    /// 暂缓：将任务改经优先级队列延后
    Defer,
    /// 重排：拒绝当前任务并触发调度器重新排序
    Reschedule,
}

/// 调度器内部可变状态
///
/// 字段通过 `std::sync::RwLock` 保护：
/// - `state`：最近一次采样映射出的 [`MemoryState`]
/// - `critical_since`：首次进入 Critical 的时刻；离开 Critical 时清空
#[derive(Debug)]
struct MemorySchedulerInner {
    state: MemoryState,
    critical_since: Option<Instant>,
}

impl Default for MemorySchedulerInner {
    fn default() -> Self {
        Self {
            state: MemoryState::Normal,
            critical_since: None,
        }
    }
}

/// 内存感知调度器
///
/// 通过注入的 [`SystemMonitorTrait`] 周期性采样内存使用率并更新内部状态。
/// 调用方在 `acquire_concurrency_permit` 之前调用 [`MemoryScheduler::admit`]
/// 决定是否继续推进任务。
pub struct MemoryScheduler {
    /// 内存数据源（复用现有监控组件，规则 5/7）
    monitor: Arc<dyn SystemMonitorTrait>,
    /// 进入 Pressure 状态的内存使用率阈值（0.0 - 1.0）
    pressure_threshold: f64,
    /// 进入 Critical 状态的内存使用率阈值（0.0 - 1.0）
    critical_threshold: f64,
    /// Critical 状态持续多久后触发优雅关闭信号
    critical_timeout: Duration,
    /// 内部状态（state + critical_since）， RwLock 同步保护
    ///
    /// 选用 `std::sync::RwLock` 而非 `tokio::sync::RwLock`：
    /// [`MemoryScheduler::state`] 在设计中为同步函数，不能 `.await`；
    /// 状态读写临界区极短（仅赋值/拷贝），不会阻塞 async runtime。
    inner: Arc<RwLock<MemorySchedulerInner>>,
    /// 优雅关闭信号发送端：Critical 持续超时后置为 true
    shutdown_tx: watch::Sender<bool>,
}

impl MemoryScheduler {
    /// 创建调度器
    ///
    /// `pressure_threshold` 必须严格小于 `critical_threshold`，否则采样永远
    /// 不会映射到 Pressure（直接跳到 Critical）。调用方应通过配置保证
    /// （[`crate::config::ConcurrencySettings`] 在 `Default` 中已满足此约束）。
    pub fn new(
        monitor: Arc<dyn SystemMonitorTrait>,
        pressure_threshold: f64,
        critical_threshold: f64,
        critical_timeout: Duration,
    ) -> Self {
        let (shutdown_tx, _) = watch::channel(false);
        Self {
            monitor,
            pressure_threshold,
            critical_threshold,
            critical_timeout,
            inner: Arc::new(RwLock::new(MemorySchedulerInner::default())),
            shutdown_tx,
        }
    }

    /// 当前内存状态
    ///
    /// 读取最近一次 [`MemoryScheduler::update_state`] 写入的值。
    /// 启动后未采样前返回 [`MemoryState::Normal`]。
    pub fn state(&self) -> MemoryState {
        let inner = self
            .inner
            .read()
            .expect("MemoryScheduler inner RwLock poisoned");
        inner.state
    }

    /// 采样内存并更新内部状态
    ///
    /// - 首次进入 Critical 时记录 `critical_since`
    /// - 离开 Critical 时清空 `critical_since`
    /// - 持续处于 Critical 时不重置 `critical_since`（保证超时判定单调）
    pub fn update_state(&self) {
        let usage = self.monitor.memory_usage();
        let new_state = if usage >= self.critical_threshold {
            MemoryState::Critical
        } else if usage >= self.pressure_threshold {
            MemoryState::Pressure
        } else {
            MemoryState::Normal
        };

        let mut inner = self
            .inner
            .write()
            .expect("MemoryScheduler inner RwLock poisoned");
        match new_state {
            MemoryState::Critical => {
                if inner.critical_since.is_none() {
                    inner.critical_since = Some(Instant::now());
                }
            }
            _ => inner.critical_since = None,
        }
        inner.state = new_state;
    }

    /// 准入决策
    ///
    /// - [`MemoryState::Normal`] → [`Admission::Proceed`]
    /// - [`MemoryState::Pressure`] → [`Admission::Defer`]
    /// - [`MemoryState::Critical`] → [`Admission::Reschedule`]
    pub async fn admit(&self) -> Admission {
        match self.state() {
            MemoryState::Normal => Admission::Proceed,
            MemoryState::Pressure => Admission::Defer,
            MemoryState::Critical => Admission::Reschedule,
        }
    }

    /// 检查 Critical 持续时间是否超过阈值
    ///
    /// 用于 [`MemoryScheduler::spawn_monitor`] 决定是否发送优雅关闭信号。
    /// 非 Critical 状态永远返回 `false`。
    pub fn check_critical_timeout(&self) -> bool {
        let inner = self
            .inner
            .read()
            .expect("MemoryScheduler inner RwLock poisoned");
        match inner.critical_since {
            Some(since) => since.elapsed() >= self.critical_timeout,
            None => false,
        }
    }

    /// 获取优雅关闭信号接收端
    ///
    /// 当 `spawn_monitor` 检测到 Critical 持续超时时会发送 `true`。
    /// 调用方可以 `await` `receiver.changed()` 等待信号触发。
    pub fn shutdown_signal(&self) -> watch::Receiver<bool> {
        self.shutdown_tx.subscribe()
    }

    /// 启动后台监控任务
    ///
    /// 周期（1 秒）采样内存并更新状态；一旦 Critical 持续时长超过
    /// `critical_timeout`，通过 `shutdown_signal` 发送 `true` 并退出循环。
    ///
    /// T025 修复：返回 `JoinHandle` 供调用方在关闭时 await，防止任务泄漏。
    ///
    /// 返回 (`watch::Receiver<bool>`, `JoinHandle<()>`)：
    /// - receiver 供调用方监听关闭信号
    /// - JoinHandle 供关闭时 await 确保监控任务退出
    pub fn spawn_monitor(self: Arc<Self>) -> (watch::Receiver<bool>, tokio::task::JoinHandle<()>) {
        let rx = self.shutdown_signal();
        let me = Arc::clone(&self);
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                me.update_state();
                if me.check_critical_timeout() {
                    let _ = me.shutdown_tx.send(true);
                    break;
                }
            }
        });
        (rx, handle)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 可变 mock 句柄：测试中动态调整 `memory_usage` 返回值
    ///
    /// `MockSystemMonitor` 内部持有同一 `Arc<AtomicU64>`，
    /// 测试通过句柄修改值即可让调度器读到最新数据。
    #[derive(Clone)]
    struct MockHandle {
        memory_bits: Arc<AtomicU64>,
    }

    impl MockHandle {
        fn set_memory(&self, value: f64) {
            self.memory_bits.store(value.to_bits(), Ordering::Relaxed);
        }
    }

    /// 可变 mock：`memory_usage` 从内部 `AtomicU64` 读取（f64 位模式）
    struct MockSystemMonitor {
        memory_bits: Arc<AtomicU64>,
    }

    impl MockSystemMonitor {
        /// 返回 (mock trait 对象, 可变句柄)
        fn pair(memory: f64) -> (Arc<Self>, MockHandle) {
            let bits = Arc::new(AtomicU64::new(memory.to_bits()));
            (
                Arc::new(Self {
                    memory_bits: bits.clone(),
                }),
                MockHandle { memory_bits: bits },
            )
        }
    }

    impl SystemMonitorTrait for MockSystemMonitor {
        fn cpu_usage(&self) -> f64 {
            0.0
        }
        fn memory_usage(&self) -> f64 {
            f64::from_bits(self.memory_bits.load(Ordering::Relaxed))
        }
        fn is_metrics_stale(&self) -> bool {
            false
        }
    }

    /// 构造调度器并返回可变 mock 句柄，便于状态转移类测试动态改值
    fn make_scheduler(
        memory: f64,
        pressure: f64,
        critical: f64,
        timeout: Duration,
    ) -> (MemoryScheduler, MockHandle) {
        let (mock, handle) = MockSystemMonitor::pair(memory);
        (
            MemoryScheduler::new(mock, pressure, critical, timeout),
            handle,
        )
    }

    /// 不需要动态改值的测试用便捷构造
    fn make_static_scheduler(
        memory: f64,
        pressure: f64,
        critical: f64,
        timeout: Duration,
    ) -> MemoryScheduler {
        make_scheduler(memory, pressure, critical, timeout).0
    }

    // ========== MemoryState 映射测试 ==========

    #[test]
    fn test_state_normal_below_pressure_threshold() {
        let scheduler = make_static_scheduler(0.5, 0.8, 0.9, Duration::from_secs(30));
        scheduler.update_state();
        assert_eq!(scheduler.state(), MemoryState::Normal);
    }

    #[test]
    fn test_state_pressure_between_thresholds() {
        let scheduler = make_static_scheduler(0.85, 0.8, 0.9, Duration::from_secs(30));
        scheduler.update_state();
        assert_eq!(scheduler.state(), MemoryState::Pressure);
    }

    #[test]
    fn test_state_pressure_at_pressure_threshold_boundary() {
        // 边界：usage == pressure_threshold 应当落入 Pressure（>= 判定）
        let scheduler = make_static_scheduler(0.8, 0.8, 0.9, Duration::from_secs(30));
        scheduler.update_state();
        assert_eq!(scheduler.state(), MemoryState::Pressure);
    }

    #[test]
    fn test_state_critical_at_critical_threshold_boundary() {
        let scheduler = make_static_scheduler(0.9, 0.8, 0.9, Duration::from_secs(30));
        scheduler.update_state();
        assert_eq!(scheduler.state(), MemoryState::Critical);
    }

    #[test]
    fn test_state_critical_above_critical_threshold() {
        let scheduler = make_static_scheduler(0.95, 0.8, 0.9, Duration::from_secs(30));
        scheduler.update_state();
        assert_eq!(scheduler.state(), MemoryState::Critical);
    }

    #[test]
    fn test_state_defaults_to_normal_before_first_sample() {
        let scheduler = make_static_scheduler(0.95, 0.8, 0.9, Duration::from_secs(30));
        // 未调用 update_state 前应为 Normal（避免启动期误判）
        assert_eq!(scheduler.state(), MemoryState::Normal);
    }

    // ========== 状态转移与 critical_since 单调性 ==========

    #[test]
    fn test_state_transition_critical_to_normal_clears_critical_since() {
        let (scheduler, handle) = make_scheduler(0.95, 0.8, 0.9, Duration::from_secs(30));
        scheduler.update_state();
        assert_eq!(scheduler.state(), MemoryState::Critical);
        // 进入 Critical 后 critical_since 已设置，未超时前 check 返回 false
        assert!(!scheduler.check_critical_timeout());

        // 模拟内存回落
        handle.set_memory(0.3);
        scheduler.update_state();
        assert_eq!(scheduler.state(), MemoryState::Normal);
        // 离开 Critical 后 critical_since 必须清空
        assert!(!scheduler.check_critical_timeout());
    }

    #[test]
    fn test_critical_since_persists_across_consecutive_critical_samples() {
        let (scheduler, _handle) = make_scheduler(0.95, 0.8, 0.9, Duration::from_millis(10));
        scheduler.update_state();
        // 第一次采样设置 critical_since（仍未超时）
        assert!(!scheduler.check_critical_timeout());

        // 短暂 sleep 后再次采样（仍在 Critical），critical_since 不应被重置
        std::thread::sleep(Duration::from_millis(5));
        scheduler.update_state();
        std::thread::sleep(Duration::from_millis(10));
        // 此时若 critical_since 被第二次 update 重置，check 不会返回 true
        assert!(
            scheduler.check_critical_timeout(),
            "critical_since 必须保持首次进入时刻"
        );
    }

    // ========== admit 决策测试 ==========

    #[tokio::test]
    async fn test_admit_proceed_when_normal() {
        let scheduler = make_static_scheduler(0.5, 0.8, 0.9, Duration::from_secs(30));
        scheduler.update_state();
        assert_eq!(scheduler.admit().await, Admission::Proceed);
    }

    #[tokio::test]
    async fn test_admit_defer_when_pressure() {
        let scheduler = make_static_scheduler(0.85, 0.8, 0.9, Duration::from_secs(30));
        scheduler.update_state();
        assert_eq!(scheduler.admit().await, Admission::Defer);
    }

    #[tokio::test]
    async fn test_admit_reschedule_when_critical() {
        let scheduler = make_static_scheduler(0.95, 0.8, 0.9, Duration::from_secs(30));
        scheduler.update_state();
        assert_eq!(scheduler.admit().await, Admission::Reschedule);
    }

    #[tokio::test]
    async fn test_admit_follows_live_state_changes() {
        // 同一调度器：状态变化时 admit 决策同步变化
        let (scheduler, handle) = make_scheduler(0.5, 0.8, 0.9, Duration::from_secs(30));
        scheduler.update_state();
        assert_eq!(scheduler.admit().await, Admission::Proceed);

        handle.set_memory(0.85);
        scheduler.update_state();
        assert_eq!(scheduler.admit().await, Admission::Defer);

        handle.set_memory(0.95);
        scheduler.update_state();
        assert_eq!(scheduler.admit().await, Admission::Reschedule);
    }

    // ========== check_critical_timeout 测试 ==========

    #[test]
    fn test_check_critical_timeout_false_when_not_critical() {
        let scheduler = make_static_scheduler(0.5, 0.8, 0.9, Duration::from_millis(10));
        scheduler.update_state();
        assert!(!scheduler.check_critical_timeout());
    }

    #[test]
    fn test_check_critical_timeout_false_immediately_after_entering_critical() {
        let scheduler = make_static_scheduler(0.95, 0.8, 0.9, Duration::from_secs(60));
        scheduler.update_state();
        assert!(!scheduler.check_critical_timeout());
    }

    #[test]
    fn test_check_critical_timeout_true_after_elapsed_exceeds_timeout() {
        let scheduler = make_static_scheduler(0.95, 0.8, 0.9, Duration::from_millis(20));
        scheduler.update_state();
        std::thread::sleep(Duration::from_millis(30));
        assert!(scheduler.check_critical_timeout());
    }

    // ========== shutdown_signal 测试 ==========

    #[tokio::test]
    async fn test_shutdown_signal_initially_false() {
        let scheduler = make_static_scheduler(0.5, 0.8, 0.9, Duration::from_secs(30));
        let rx = scheduler.shutdown_signal();
        assert!(!(*rx.borrow()));
    }

    #[tokio::test]
    async fn test_spawn_monitor_emits_shutdown_on_critical_timeout() {
        let scheduler = Arc::new(make_static_scheduler(
            0.95,
            0.8,
            0.9,
            Duration::from_millis(50),
        ));
        let (mut rx, _handle) = scheduler.clone().spawn_monitor();

        // 等待后台任务采样并最终触发关闭信号
        let changed = tokio::time::timeout(Duration::from_secs(5), rx.changed()).await;
        assert!(changed.is_ok(), "spawn_monitor 必须在超时后发送关闭信号");
        assert!(*rx.borrow());
    }

    #[tokio::test]
    async fn test_spawn_monitor_no_shutdown_when_memory_recovers() {
        // 短超时，但内存正常 → 不应发送 shutdown
        let scheduler = Arc::new(make_static_scheduler(
            0.5,
            0.8,
            0.9,
            Duration::from_millis(50),
        ));
        let (mut rx, _handle) = scheduler.clone().spawn_monitor();
        let changed = tokio::time::timeout(Duration::from_millis(500), rx.changed()).await;
        assert!(changed.is_err(), "内存正常时不应发送 shutdown 信号");
    }
}
