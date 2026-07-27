// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Hedge 请求副本控制器（design.md §17，T070/R-runtime-004）
//!
//! 移植 spider `hedge.rs`：基于 EMA（指数移动平均）+ 方差估算 P84 延迟阈值，
//! 超阈值时建议发送副本请求降尾延迟。
//!
//! 核心算法：
//! - EMA：`EMA_new = α·x + (1-α)·EMA_old`
//! - 方差：`Var_new = (1-α)·Var_old + α·(x-EMA_new)·(x-EMA_old)`
//!   （指数加权移动方差，递推式）
//! - P84 阈值：`P84 = EMA + σ_multiplier · sqrt(Var)`（标准正态分布 P84 ≈ μ+σ）
//!
//! # 并发模型选型（架构审查 C-1/H-1 修复）
//!
//! 选用 `parking_lot::Mutex<HedgeState>` 而非 `AtomicU64` 双 CAS：
//! - **正确性优先**：variance 递推式依赖 `(ema_old, var_old, ema_new)` 三元组，
//!   AtomicU64 双 CAS 仍可能在 variance CAS 失败时丢失更新，导致方差系统性低估。
//!   Mutex 保证三元组原子读写，无丢失更新。
//! - **非热路径**：仅 `route_race_mode` 胜出后调用 1 次 `record_latency`，
//!   race_mode 默认关闭；即使开启，每请求仅 1 次记录，Mutex 开销 ~50-100ns，
//!   相对 race 本身 100ms+ 网络耗时占比 < 0.0001%。
//! - **与 AIMDController 风格差异说明**：AIMD 用 `AtomicUsize` 是因 AIMD 的
//!   `current_limit` / `consecutive_successes` 是独立变量（自校正容忍丢失更新）；
//!   hedge 的 ema/var 是耦合递推，必须跨变量一致性。两模块风格差异是设计驱动，
//!   非随意打破惯例（规则 4 暴露冲突，规则 8 惯例优先但正确性优先）。
//!
//! 接入路径（T070）：`EngineRouter::route_race_mode` 在 race 胜出后调用
//! `record_latency(response_time)`，未来顺序路径可调用 `should_hedge(elapsed)`
//! 决策是否发起副本。
//!
//! # 样本来源限制（架构审查 M-2）
//!
//! 当前 `record_latency` 仅在 race 胜出后调用，记录的是 `min(各引擎延迟)` 分布。
//! **不可直接用于顺序路径 P84 估算**（顺序路径是单引擎全延迟，分布完全不同，
//! 复用会导致阈值系统性偏低，触发过多副本）。顺序路径接入时需独立的
//! HedgeController 实例，或本模块新增按路径分桶的统计。

use parking_lot::Mutex;
use std::time::Duration;

/// 默认 EMA 系数 α（越接近 1 越重视近期）
pub const DEFAULT_EMA_ALPHA: f64 = 0.2;

/// 默认启用 hedge 的最小样本数（避免冷启动期估算失真）
pub const DEFAULT_MIN_SAMPLES: usize = 10;

/// 默认 σ 倍数（P84 = μ + 1·σ）
pub const DEFAULT_SIGMA_MULTIPLIER: f64 = 1.0;

/// 内部状态：受 Mutex 保护的三元组
///
/// 单独结构体便于一次性 `lock()` 读写，保证跨变量一致性。
#[derive(Debug, Clone, Default)]
struct HedgeState {
    /// 当前 EMA 延迟（ms）
    ema_latency_ms: f64,
    /// 当前方差（ms²）
    variance_ms2: f64,
    /// 样本计数
    sample_count: usize,
}

/// Hedge 控制器：基于 EMA + 方差估算 P84 延迟阈值
///
/// - `record_latency`：更新 EMA 和方差（`pub(crate)`，仅 crate 内 router 接入）
/// - `should_hedge`：判断已耗时是否超过 P84 阈值（`pub`，未来顺序路径可调用）
///
/// 线程安全：内部 `parking_lot::Mutex<HedgeState>`，可安全跨线程共享。
#[derive(Debug)]
pub struct HedgeController {
    /// 受锁保护的状态三元组
    state: Mutex<HedgeState>,
    /// EMA 系数 α（不可变）
    ema_alpha: f64,
    /// 启用 hedge 的最小样本数（不可变）
    min_samples: usize,
    /// σ 倍数（不可变）
    sigma_multiplier: f64,
}

impl HedgeController {
    /// 创建指定参数的 HedgeController
    ///
    /// # Panics
    ///
    /// 参数不变式违反时 panic（fail-fast，规则 12 显性化失败）：
    /// - `0.0 < ema_alpha <= 1.0`：EMA 系数必须开区间下侧
    /// - `min_samples >= 1`：否则永不启用 hedge
    /// - `sigma_multiplier > 0.0`：否则阈值恒等于 EMA
    pub fn new(ema_alpha: f64, min_samples: usize, sigma_multiplier: f64) -> Self {
        assert!(
            ema_alpha > 0.0 && ema_alpha <= 1.0,
            "ema_alpha must be in (0, 1], got {ema_alpha}"
        );
        assert!(
            min_samples >= 1,
            "min_samples must be >= 1, got {min_samples}"
        );
        assert!(
            sigma_multiplier > 0.0,
            "sigma_multiplier must be > 0.0, got {sigma_multiplier}"
        );
        Self {
            state: Mutex::new(HedgeState::default()),
            ema_alpha,
            min_samples,
            sigma_multiplier,
        }
    }

    /// 使用默认参数创建（α=0.2，min_samples=10，σ_mult=1.0）
    pub fn with_defaults() -> Self {
        Self::new(
            DEFAULT_EMA_ALPHA,
            DEFAULT_MIN_SAMPLES,
            DEFAULT_SIGMA_MULTIPLIER,
        )
    }

    /// 记录一次延迟，更新 EMA 和方差
    ///
    /// 算法（指数加权移动方差递推式）：
    /// - `EMA_new = α·x + (1-α)·EMA_old`
    /// - `Var_new = (1-α)·Var_old + α·(x-EMA_new)·(x-EMA_old)`
    ///
    /// 第一个样本：`EMA=x`，`Var=0`。
    ///
    /// # 并发安全
    ///
    /// 使用 `parking_lot::Mutex` 保证三元组 `(ema, var, count)` 原子更新，
    /// 无丢失更新（架构审查 H-1 修复）。
    pub(crate) fn record_latency(&self, latency: Duration) {
        let x_ms = latency.as_secs_f64() * 1000.0;
        let alpha = self.ema_alpha;

        let mut state = self.state.lock();
        if state.sample_count == 0 {
            // 首样本：EMA=x，Var=0
            state.ema_latency_ms = x_ms;
            state.variance_ms2 = 0.0;
            state.sample_count = 1;
        } else {
            let ema_old = state.ema_latency_ms;
            let var_old = state.variance_ms2;
            let ema_new = alpha * x_ms + (1.0 - alpha) * ema_old;
            // 指数加权移动方差递推式
            let var_new = (1.0 - alpha) * var_old + alpha * (x_ms - ema_new) * (x_ms - ema_old);
            state.ema_latency_ms = ema_new;
            state.variance_ms2 = var_new;
            state.sample_count += 1;
        }
    }

    /// 获取 P84 阈值（EMA + σ_multiplier · sqrt(Var)）
    ///
    /// 样本不足（< `min_samples`）或数值非法时返回 `None`。
    pub fn p84_threshold(&self) -> Option<Duration> {
        let state = self.state.lock();
        if state.sample_count < self.min_samples {
            return None;
        }
        let ema_ms = state.ema_latency_ms;
        let var_ms2 = state.variance_ms2;
        if !ema_ms.is_finite() || !var_ms2.is_finite() || var_ms2 < 0.0 {
            return None;
        }
        let sigma_ms = var_ms2.sqrt();
        let threshold_ms = ema_ms + self.sigma_multiplier * sigma_ms;
        if !threshold_ms.is_finite() || threshold_ms < 0.0 {
            return None;
        }
        Some(Duration::from_secs_f64(threshold_ms / 1000.0))
    }

    /// 判断当前已耗时是否超过 P84 阈值（触发 hedge 副本）
    ///
    /// 样本不足时返回 `false`（无法估算）。
    pub fn should_hedge(&self, elapsed: Duration) -> bool {
        match self.p84_threshold() {
            Some(threshold) => elapsed > threshold,
            None => false,
        }
    }

    /// 获取样本数
    pub fn sample_count(&self) -> usize {
        self.state.lock().sample_count
    }

    /// 获取当前 EMA 延迟（样本数为 0 或数值非法时返回 None）
    pub fn ema_latency(&self) -> Option<Duration> {
        let state = self.state.lock();
        if state.sample_count == 0 {
            return None;
        }
        let ema_ms = state.ema_latency_ms;
        if !ema_ms.is_finite() || ema_ms < 0.0 {
            return None;
        }
        Some(Duration::from_secs_f64(ema_ms / 1000.0))
    }

    /// 获取当前标准差（单位：ms，样本不足或数值非法时返回 None）
    ///
    /// 返回 `f64` 而非 `Duration`：标准差是统计量而非时间值，
    /// 调用方多用于 P84 计算/日志展示，ms 单位的 f64 更直观。
    pub fn std_dev_ms(&self) -> Option<f64> {
        let state = self.state.lock();
        if state.sample_count < self.min_samples {
            return None;
        }
        let var_ms2 = state.variance_ms2;
        if !var_ms2.is_finite() || var_ms2 < 0.0 {
            return None;
        }
        Some(var_ms2.sqrt())
    }

    /// 重置状态（仅用于测试或已知无并发的冷启动）
    ///
    /// # 并发警告
    ///
    /// 虽然 `Mutex` 保证本方法不引发 UB，但若与并发的 `record_latency` 交错执行，
    /// 可能产生中间状态（部分字段已清零、部分未清零）被读路径观测到。
    /// **生产路径禁止并发调用**，仅在已知无 race 路径调用时使用。
    ///
    /// # 为何 `#[allow(dead_code)]`
    ///
    /// 当前仅在测试中调用，但保留 `pub(crate)` 接口为未来冷启动/配置热重载
    /// 场景预留（架构审查 M-1：接口隔离，不暴露为 `pub`）。
    #[allow(dead_code)]
    pub(crate) fn reset(&self) {
        let mut state = self.state.lock();
        state.ema_latency_ms = 0.0;
        state.variance_ms2 = 0.0;
        state.sample_count = 0;
    }
}

impl Default for HedgeController {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    // ========== new / with_defaults ==========

    #[test]
    fn with_defaults_matches_constants() {
        let c = HedgeController::with_defaults();
        // 不可变参数无 getter（架构审查 M-4：删除投机性 getter），
        // 通过行为验证：min_samples=10 时阈值在样本数 9/10 之间切换
        for _ in 0..9 {
            c.record_latency(Duration::from_millis(100));
        }
        assert_eq!(c.sample_count(), 9);
        assert!(c.p84_threshold().is_none(), "below min_samples=10");

        c.record_latency(Duration::from_millis(100));
        assert_eq!(c.sample_count(), 10);
        assert!(c.p84_threshold().is_some(), "at min_samples=10");
    }

    #[test]
    fn default_equals_with_defaults() {
        let a = HedgeController::default();
        let b = HedgeController::with_defaults();
        // 行为等价：相同参数下首样本行为一致
        a.record_latency(Duration::from_millis(100));
        b.record_latency(Duration::from_millis(100));
        assert_eq!(a.sample_count(), b.sample_count());
        assert_eq!(a.ema_latency(), b.ema_latency());
    }

    #[test]
    fn new_custom_params() {
        // min_samples=5 验证
        let c = HedgeController::new(0.5, 5, 2.0);
        for _ in 0..4 {
            c.record_latency(Duration::from_millis(100));
        }
        assert_eq!(c.sample_count(), 4);
        assert!(c.p84_threshold().is_none(), "below custom min_samples=5");

        c.record_latency(Duration::from_millis(100));
        assert!(c.p84_threshold().is_some(), "at custom min_samples=5");
    }

    #[test]
    #[should_panic(expected = "ema_alpha must be in (0, 1]")]
    fn new_alpha_zero_panics() {
        let _ = HedgeController::new(0.0, 10, 1.0);
    }

    #[test]
    #[should_panic(expected = "ema_alpha must be in (0, 1]")]
    fn new_alpha_above_one_panics() {
        let _ = HedgeController::new(1.5, 10, 1.0);
    }

    #[test]
    #[should_panic(expected = "min_samples must be >= 1")]
    fn new_min_samples_zero_panics() {
        let _ = HedgeController::new(0.2, 0, 1.0);
    }

    #[test]
    #[should_panic(expected = "sigma_multiplier must be > 0.0")]
    fn new_sigma_multiplier_zero_panics() {
        let _ = HedgeController::new(0.2, 10, 0.0);
    }

    #[test]
    fn new_alpha_one_is_valid() {
        let c = HedgeController::new(1.0, 1, 1.0);
        c.record_latency(Duration::from_millis(100));
        // α=1.0：EMA=x
        let ema = c.ema_latency().unwrap();
        let ema_ms = ema.as_secs_f64() * 1000.0;
        assert!((ema_ms - 100.0).abs() < 0.01);
    }

    // ========== record_latency: 首样本 ==========

    #[test]
    fn record_first_sample_sets_ema_to_latency() {
        let c = HedgeController::with_defaults();
        c.record_latency(Duration::from_millis(100));
        assert_eq!(c.sample_count(), 1);
        assert_eq!(c.ema_latency(), Some(Duration::from_millis(100)));
    }

    #[test]
    fn record_first_sample_variance_is_zero() {
        let c = HedgeController::with_defaults();
        c.record_latency(Duration::from_millis(100));
        // 首样本：EMA=x，Var=0；但 sample_count < min_samples，std_dev 返回 None
        assert_eq!(c.ema_latency(), Some(Duration::from_millis(100)));
    }

    // ========== record_latency: EMA 递推 ==========

    #[test]
    fn record_two_samples_ema_converges() {
        let c = HedgeController::with_defaults();
        // α=0.2: EMA_new = 0.2*x + 0.8*EMA_old
        c.record_latency(Duration::from_millis(100)); // EMA=100
        c.record_latency(Duration::from_millis(200)); // EMA=0.2*200+0.8*100=120
        let ema = c.ema_latency().unwrap();
        let ema_ms = ema.as_secs_f64() * 1000.0;
        assert!(
            (ema_ms - 120.0).abs() < 0.01,
            "EMA should be 120, got {ema_ms}"
        );
    }

    #[test]
    fn record_multiple_samples_ema_weighted_average() {
        let c = HedgeController::with_defaults();
        for _ in 0..20 {
            c.record_latency(Duration::from_millis(100));
        }
        // 持续输入相同值，EMA 应收敛到 100
        let ema = c.ema_latency().unwrap();
        let ema_ms = ema.as_secs_f64() * 1000.0;
        assert!(
            (ema_ms - 100.0).abs() < 0.5,
            "EMA should converge to 100, got {ema_ms}"
        );
    }

    #[test]
    fn record_constant_samples_variance_zero() {
        let c = HedgeController::with_defaults();
        for _ in 0..20 {
            c.record_latency(Duration::from_millis(100));
        }
        // 持续输入相同值，方差应收敛到 0
        let sigma = c.std_dev_ms().unwrap();
        assert!(sigma < 0.1, "std_dev should be near 0, got {sigma}");
    }

    #[test]
    fn record_high_variance_samples_increases_stddev() {
        let c = HedgeController::with_defaults();
        // 交替输入 50ms 和 150ms（均值 100ms，方差约 2500ms²）
        for i in 0..20 {
            let v = if i % 2 == 0 { 50 } else { 150 };
            c.record_latency(Duration::from_millis(v));
        }
        let sigma = c.std_dev_ms().unwrap();
        // 理论 σ ≈ 50ms（σ_mult=1，P84≈EMA+50）
        assert!(
            sigma > 30.0 && sigma < 70.0,
            "std_dev should be near 50, got {sigma}"
        );
    }

    // ========== p84_threshold ==========

    #[test]
    fn p84_threshold_none_below_min_samples() {
        let c = HedgeController::with_defaults();
        // min_samples=10，记录 9 个样本
        for _ in 0..9 {
            c.record_latency(Duration::from_millis(100));
        }
        assert_eq!(c.p84_threshold(), None);
    }

    #[test]
    fn p84_threshold_some_at_min_samples() {
        let c = HedgeController::with_defaults();
        for _ in 0..10 {
            c.record_latency(Duration::from_millis(100));
        }
        let threshold = c.p84_threshold().expect("threshold should be available");
        // 持续 100ms：EMA=100，Var=0，P84=100+1*0=100
        let threshold_ms = threshold.as_secs_f64() * 1000.0;
        assert!(
            (threshold_ms - 100.0).abs() < 0.5,
            "P84 should be 100, got {threshold_ms}"
        );
    }

    #[test]
    fn p84_threshold_equals_ema_plus_sigma_for_p84() {
        // min_samples=2，快速达到阈值
        let c = HedgeController::new(0.5, 2, 1.0);
        c.record_latency(Duration::from_millis(100));
        c.record_latency(Duration::from_millis(200));
        // EMA=0.5*200+0.5*100=150
        // Var=0.5*0+0.5*(200-150)*(200-100)=0.5*50*100=2500
        // σ=50, P84=150+50=200
        let threshold = c.p84_threshold().expect("threshold should be available");
        let threshold_ms = threshold.as_secs_f64() * 1000.0;
        assert!(
            (threshold_ms - 200.0).abs() < 1.0,
            "P84 should be 200, got {threshold_ms}"
        );
    }

    #[test]
    fn p84_threshold_respects_sigma_multiplier() {
        let c = HedgeController::new(0.5, 2, 2.0);
        c.record_latency(Duration::from_millis(100));
        c.record_latency(Duration::from_millis(200));
        // EMA=150, σ=50, P84=150+2*50=250
        let threshold = c.p84_threshold().expect("threshold should be available");
        let threshold_ms = threshold.as_secs_f64() * 1000.0;
        assert!(
            (threshold_ms - 250.0).abs() < 1.0,
            "P84 with mult=2 should be 250, got {threshold_ms}"
        );
    }

    // ========== should_hedge ==========

    #[test]
    fn should_hedge_false_below_min_samples() {
        let c = HedgeController::with_defaults();
        for _ in 0..5 {
            c.record_latency(Duration::from_millis(100));
        }
        // 样本不足：返回 false
        assert!(!c.should_hedge(Duration::from_secs(10)));
    }

    #[test]
    fn should_hedge_false_when_elapsed_below_threshold() {
        let c = HedgeController::with_defaults();
        for _ in 0..15 {
            c.record_latency(Duration::from_millis(100));
        }
        // P84≈100ms，elapsed=50ms 低于阈值
        assert!(!c.should_hedge(Duration::from_millis(50)));
    }

    #[test]
    fn should_hedge_false_when_elapsed_equals_threshold() {
        let c = HedgeController::with_defaults();
        for _ in 0..15 {
            c.record_latency(Duration::from_millis(100));
        }
        // elapsed == threshold：> 严格大于，等于返回 false
        let threshold = c.p84_threshold().unwrap();
        assert!(!c.should_hedge(threshold));
    }

    #[test]
    fn should_hedge_true_when_elapsed_above_threshold() {
        let c = HedgeController::with_defaults();
        for _ in 0..15 {
            c.record_latency(Duration::from_millis(100));
        }
        // P84≈100ms，elapsed=200ms 超阈值
        assert!(c.should_hedge(Duration::from_millis(200)));
    }

    // ========== reset ==========

    #[test]
    fn reset_clears_state() {
        let c = HedgeController::with_defaults();
        for _ in 0..15 {
            c.record_latency(Duration::from_millis(100));
        }
        assert_eq!(c.sample_count(), 15);
        assert!(c.p84_threshold().is_some());

        c.reset();
        assert_eq!(c.sample_count(), 0);
        assert_eq!(c.p84_threshold(), None);
        assert_eq!(c.ema_latency(), None);
    }

    #[test]
    fn record_after_reset_treats_as_first_sample() {
        let c = HedgeController::with_defaults();
        for _ in 0..15 {
            c.record_latency(Duration::from_millis(100));
        }
        c.reset();
        c.record_latency(Duration::from_millis(200));
        // 首样本：EMA=200
        assert_eq!(c.ema_latency(), Some(Duration::from_millis(200)));
        assert_eq!(c.sample_count(), 1);
    }

    // ========== 并发安全（Mutex 保证原子性） ==========

    #[test]
    fn concurrent_record_latency_no_lost_updates() {
        let c = Arc::new(HedgeController::new(0.2, 1, 1.0));
        let thread_count = 4;
        let per_thread = 100;
        let mut handles = Vec::new();
        for _ in 0..thread_count {
            let cc = c.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..per_thread {
                    cc.record_latency(Duration::from_millis(100));
                }
            }));
        }
        for h in handles {
            h.join().expect("thread should not panic");
        }
        // Mutex 保证无丢失更新：sample_count 必须精确等于 thread_count * per_thread
        assert_eq!(
            c.sample_count(),
            thread_count * per_thread,
            "Mutex should prevent lost updates"
        );
    }

    #[test]
    fn concurrent_record_then_check_threshold() {
        let c = Arc::new(HedgeController::new(0.2, 5, 1.0));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let cc = c.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..50 {
                    cc.record_latency(Duration::from_millis(100));
                    // 并发读阈值不应 panic
                    let _ = cc.p84_threshold();
                    let _ = cc.should_hedge(Duration::from_millis(200));
                }
            }));
        }
        for h in handles {
            h.join().expect("thread should not panic");
        }
        assert_eq!(c.sample_count(), 200);
        // 全部 100ms：阈值应接近 100
        let threshold = c.p84_threshold().expect("threshold should be available");
        let threshold_ms = threshold.as_secs_f64() * 1000.0;
        assert!(
            (threshold_ms - 100.0).abs() < 5.0,
            "P84 should converge to ~100, got {threshold_ms}"
        );
    }

    // ========== ema_latency / std_dev 边界 ==========

    #[test]
    fn ema_latency_none_on_empty_controller() {
        let c = HedgeController::with_defaults();
        assert_eq!(c.ema_latency(), None);
    }

    #[test]
    fn std_dev_none_below_min_samples() {
        let c = HedgeController::with_defaults();
        c.record_latency(Duration::from_millis(100));
        assert_eq!(c.std_dev_ms(), None);
    }

    #[test]
    fn std_dev_some_at_min_samples() {
        let c = HedgeController::with_defaults();
        for _ in 0..10 {
            c.record_latency(Duration::from_millis(100));
        }
        // 10 个样本：std_dev 应有值（即使方差近 0）
        assert!(c.std_dev_ms().is_some());
    }

    // ========== 收敛性 ==========

    #[test]
    fn ema_reacts_to_recent_changes_faster_with_higher_alpha() {
        // α=0.8：更重视近期
        let c = HedgeController::new(0.8, 1, 1.0);
        c.record_latency(Duration::from_millis(100));
        c.record_latency(Duration::from_millis(200));
        // EMA=0.8*200+0.2*100=180
        let ema = c.ema_latency().unwrap();
        let ema_ms = ema.as_secs_f64() * 1000.0;
        assert!(
            (ema_ms - 180.0).abs() < 0.5,
            "EMA should be 180, got {ema_ms}"
        );
    }

    #[test]
    fn p84_increases_with_high_latency_outlier() {
        let c = HedgeController::with_defaults();
        // 前 9 个 100ms 样本
        for _ in 0..9 {
            c.record_latency(Duration::from_millis(100));
        }
        // 第 10 个突然 1000ms
        c.record_latency(Duration::from_millis(1000));
        let threshold = c.p84_threshold().expect("threshold");
        let threshold_ms = threshold.as_secs_f64() * 1000.0;
        // EMA 应上升，方差也应上升，P84 > 100
        assert!(
            threshold_ms > 150.0,
            "P84 should react to outlier, got {threshold_ms}"
        );
    }

    // ========== 零延迟样本（边界 L-2） ==========

    #[test]
    fn record_zero_latency_is_valid_sample() {
        // 修复 L-2：放宽 ema_ms < 0.0 检查，允许 0 延迟样本
        let c = HedgeController::with_defaults();
        c.record_latency(Duration::ZERO);
        assert_eq!(c.sample_count(), 1);
        // ema_latency 应返回 Some(0ms) 而非 None（L-2 修复）
        assert_eq!(c.ema_latency(), Some(Duration::ZERO));
    }
}
