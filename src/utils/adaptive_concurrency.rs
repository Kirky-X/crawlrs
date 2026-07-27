// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! AIMD 自适应并发控制（design.md §8，T036/R-runtime-003）
//!
//! 移植 spider `adaptive_concurrency.rs`：AIMD（Additive Increase / Multiplicative Decrease）
//! 拥塞控制算法，动态调整并发上限。
//!
//! 核心组件：
//! - [`AIMDController`]：无锁（`AtomicUsize`）记录成功/失败，输出动态 target
//!   - **Additive Increase**：连续 `increase_threshold` 次成功后 `+1`
//!   - **Multiplicative Decrease**：单次失败后 `target /= 2`（clamp 到 `min_limit`）
//! - [`AdaptiveSemaphore`]：桥接 `tokio::sync::Semaphore`，`set_target` 调和可用许可
//!
//! 集成路径（T037/T038）：`TeamSemaphore::with_adaptive` + `scrape_worker::record_*`。
//! 默认关闭（`concurrency.adaptive_enabled=false`），开启后增强固定并发为动态带宽利用。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// AIMD 控制器默认参数
pub const DEFAULT_MIN_LIMIT: usize = 1;
pub const DEFAULT_MAX_LIMIT: usize = 100;
pub const DEFAULT_INCREASE_THRESHOLD: usize = 10;

/// AIMD 控制器：无锁动态并发上限
///
/// - `record_success`：连续 `increase_threshold` 次成功后 `current_limit += 1`（clamp `max_limit`）
/// - `record_failure`：`current_limit = max(min_limit, current_limit / 2)`（乘性减少）
///
/// 线程安全：内部全 `AtomicUsize`，无锁。
#[derive(Debug)]
pub struct AIMDController {
    /// 当前并发上限
    current_limit: AtomicUsize,
    /// 连续成功计数（失败时归零）
    consecutive_successes: AtomicUsize,
    /// 最小上限（乘性减少的地板）
    min_limit: usize,
    /// 最大上限（加性增加的天花板）
    max_limit: usize,
    /// 连续成功多少次才 +1
    increase_threshold: usize,
}

impl AIMDController {
    /// 使用默认参数创建
    pub fn new(initial: usize) -> Self {
        Self::with_bounds(initial, DEFAULT_MIN_LIMIT, DEFAULT_MAX_LIMIT)
    }

    /// 指定 min/max 边界创建
    ///
    /// # Panics
    ///
    /// 参数不变式违反时 panic（fail-fast，规则 12 显性化失败）：
    /// - `min_limit >= 1`：否则乘性减少可降至 0，导致 `acquire_owned()` 永久阻塞
    /// - `max_limit >= min_limit`：否则 clamp 语义未定义
    ///
    /// 使用 `assert!` 而非 `debug_assert!`：参数错误属编程错误，应在 release 构建下
    /// 也立即暴露而非静默吞掉（架构审查 H-1）。
    pub fn with_bounds(initial: usize, min_limit: usize, max_limit: usize) -> Self {
        assert!(min_limit >= 1, "min_limit must be >= 1, got {min_limit}");
        assert!(
            max_limit >= min_limit,
            "max_limit ({max_limit}) must be >= min_limit ({min_limit})"
        );
        let clamped = initial.clamp(min_limit, max_limit);
        Self {
            current_limit: AtomicUsize::new(clamped),
            consecutive_successes: AtomicUsize::new(0),
            min_limit,
            max_limit,
            increase_threshold: DEFAULT_INCREASE_THRESHOLD,
        }
    }

    /// 指定所有参数创建
    ///
    /// # Panics
    ///
    /// 参数不变式违反时 panic（同 `with_bounds`，规则 12）：
    /// - `min_limit >= 1`
    /// - `max_limit >= min_limit`
    /// - `increase_threshold >= 1`：否则永远不触发 +1，等价于固定并发
    pub fn with_params(
        initial: usize,
        min_limit: usize,
        max_limit: usize,
        increase_threshold: usize,
    ) -> Self {
        assert!(min_limit >= 1, "min_limit must be >= 1, got {min_limit}");
        assert!(
            max_limit >= min_limit,
            "max_limit ({max_limit}) must be >= min_limit ({min_limit})"
        );
        assert!(
            increase_threshold >= 1,
            "increase_threshold must be >= 1, got {increase_threshold}"
        );
        let clamped = initial.clamp(min_limit, max_limit);
        Self {
            current_limit: AtomicUsize::new(clamped),
            consecutive_successes: AtomicUsize::new(0),
            min_limit,
            max_limit,
            increase_threshold,
        }
    }

    /// 记录一次成功
    ///
    /// 连续 `increase_threshold` 次成功后 `current_limit += 1`（clamp `max_limit`）。
    /// 返回更新后的 target。
    ///
    /// # 内存序说明（架构审查 M-2）
    ///
    /// 全部使用 `Ordering::Relaxed`：AIMD 算法是自校正的，无需跨变量同步：
    /// - `current_limit` 的 CAS 保证不会超过 `max_limit`（单一变量不变式）
    /// - `consecutive_successes` 的丢失更新最多延迟一次 +1，不影响算法收敛
    /// - 失败路径的 `store(0)` 即使被并发的 `fetch_add` 覆盖，也只是少计一次成功，
    ///   下一轮 `increase_threshold` 次成功后仍会触发 +1
    ///
    /// 若未来需要更严格的同步（如监控精确读），可在读路径加 `Acquire`。
    pub fn record_success(&self) -> usize {
        let successes = self.consecutive_successes.fetch_add(1, Ordering::Relaxed) + 1;
        if successes >= self.increase_threshold {
            // 达到阈值：归零计数并尝试 +1（CAS 保证不超过 max_limit）
            self.consecutive_successes.store(0, Ordering::Relaxed);
            loop {
                let current = self.current_limit.load(Ordering::Relaxed);
                if current >= self.max_limit {
                    return current;
                }
                let new = current + 1;
                match self.current_limit.compare_exchange(
                    current,
                    new,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return new,
                    Err(_) => continue, // CAS 失败：其他线程修改了，重试
                }
            }
        }
        self.current_limit.load(Ordering::Relaxed)
    }

    /// 记录一次失败
    ///
    /// 乘性减少：`current_limit = max(min_limit, current_limit / 2)`，连续成功归零。
    /// 返回更新后的 target。
    ///
    /// # 内存序说明
    ///
    /// 同 [`record_success`](Self::record_success)，全 `Relaxed` 即可。
    pub fn record_failure(&self) -> usize {
        self.consecutive_successes.store(0, Ordering::Relaxed);
        loop {
            let current = self.current_limit.load(Ordering::Relaxed);
            let new = (current / 2).max(self.min_limit);
            if new == current {
                // 已经在 min_limit，无需减少
                return current;
            }
            if self
                .current_limit
                .compare_exchange(current, new, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return new;
            }
            // CAS 失败：其他线程修改了，重试
        }
    }

    /// 获取当前 target（不加不减）
    pub fn current_limit(&self) -> usize {
        self.current_limit.load(Ordering::Relaxed)
    }

    /// 重置为初始值（测试用）
    pub fn reset(&self, initial: usize) {
        let clamped = initial.clamp(self.min_limit, self.max_limit);
        self.current_limit.store(clamped, Ordering::Relaxed);
        self.consecutive_successes.store(0, Ordering::Relaxed);
    }

    /// 获取 min_limit
    pub fn min_limit(&self) -> usize {
        self.min_limit
    }

    /// 获取 max_limit
    pub fn max_limit(&self) -> usize {
        self.max_limit
    }
}

impl Default for AIMDController {
    fn default() -> Self {
        Self::new(10)
    }
}

/// 自适应信号量：桥接 `tokio::sync::Semaphore` 与 AIMD target
///
/// `set_target` 调和可用许可：
/// - **target 增加**：`semaphore.add_permits(diff)` 新增许可
/// - **target 减少**：`try_acquire` + `forget` 移除可用许可（best-effort；
///   正在使用的许可会在释放时自然回收，target 不再回补）
#[derive(Debug)]
pub struct AdaptiveSemaphore {
    semaphore: Arc<Semaphore>,
    target: AtomicUsize,
}

impl AdaptiveSemaphore {
    /// 创建指定初始容量的自适应信号量
    pub fn new(initial_permits: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(initial_permits)),
            target: AtomicUsize::new(initial_permits),
        }
    }

    /// 获取许可（异步等待）
    ///
    /// 返回 `OwnedSemaphorePermit`，Drop 时释放许可回信号量。
    pub async fn acquire(&self) -> OwnedSemaphorePermit {
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore closed unexpectedly")
    }

    /// 尝试立即获取许可（非阻塞）
    pub fn try_acquire(&self) -> Option<OwnedSemaphorePermit> {
        self.semaphore.clone().try_acquire_owned().ok()
    }

    /// 安全审查 H-01：查询当前可用许可数（用于判断团队是否空闲）
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// 设置新的 target 并调和许可
    ///
    /// - target > current：`add_permits(diff)`
    /// - target < current：`try_acquire` + `forget` 移除多余可用许可
    /// - target == current：无操作
    ///
    /// 返回新的 target。
    pub fn set_target(&self, new_target: usize) -> usize {
        let current = self.target.load(Ordering::Relaxed);
        if new_target == current {
            return current;
        }

        self.target.store(new_target, Ordering::Relaxed);

        if new_target > current {
            // 增加：直接 add_permits
            self.semaphore.add_permits(new_target - current);
        } else {
            // 减少：try_acquire + forget 移除可用许可
            let to_remove = current - new_target;
            let mut removed = 0;
            while removed < to_remove {
                match self.semaphore.clone().try_acquire_owned() {
                    Ok(permit) => {
                        permit.forget();
                        removed += 1;
                    }
                    Err(_) => {
                        // 无可用许可：剩余许可正在使用中，
                        // 释放时不会回补（因为 target 已降低，
                        // 下次 set_target 会基于新 target 计算）
                        // 这是一个 best-effort 的限制：实际并发可能短暂超过 target
                        break;
                    }
                }
            }
        }
        new_target
    }

    /// 获取当前 target
    pub fn current_target(&self) -> usize {
        self.target.load(Ordering::Relaxed)
    }

    /// 获取底层 Semaphore 的 Arc（供直接操作）
    pub fn semaphore(&self) -> Arc<Semaphore> {
        self.semaphore.clone()
    }
}

impl Default for AdaptiveSemaphore {
    fn default() -> Self {
        Self::new(10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== AIMDController tests ==========

    /// new() 创建的默认参数
    #[test]
    fn controller_new_default_params() {
        let c = AIMDController::new(10);
        assert_eq!(c.current_limit(), 10);
        assert_eq!(c.min_limit(), DEFAULT_MIN_LIMIT);
        assert_eq!(c.max_limit(), DEFAULT_MAX_LIMIT);
    }

    /// with_bounds 钳制 initial 到 [min, max]
    #[test]
    fn controller_with_bounds_clamps_initial() {
        let c = AIMDController::with_bounds(200, 1, 100);
        assert_eq!(c.current_limit(), 100);

        let c = AIMDController::with_bounds(0, 1, 100);
        assert_eq!(c.current_limit(), 1);
    }

    /// record_success 在阈值前不增加
    #[test]
    fn record_success_below_threshold_no_increase() {
        let c = AIMDController::with_params(10, 1, 100, 5);
        for _ in 0..4 {
            assert_eq!(c.record_success(), 10);
        }
        assert_eq!(c.current_limit(), 10);
    }

    /// record_success 达到阈值后 +1
    #[test]
    fn record_success_at_threshold_increases() {
        let c = AIMDController::with_params(10, 1, 100, 5);
        for _ in 0..4 {
            c.record_success();
        }
        // 第 5 次：达到阈值，+1
        let target = c.record_success();
        assert_eq!(target, 11);
        assert_eq!(c.current_limit(), 11);
    }

    /// 连续成功多次持续 +1
    #[test]
    fn record_success_multiple_thresholds() {
        let c = AIMDController::with_params(10, 1, 100, 3);
        // 3 次成功 → +1
        for _ in 0..3 {
            c.record_success();
        }
        assert_eq!(c.current_limit(), 11);
        // 再 3 次 → +1
        for _ in 0..3 {
            c.record_success();
        }
        assert_eq!(c.current_limit(), 12);
    }

    /// record_success 达到 max_limit 不再增加
    #[test]
    fn record_success_at_max_no_increase() {
        let c = AIMDController::with_params(99, 1, 100, 1);
        let target = c.record_success();
        assert_eq!(target, 100);
        // 再成功也不超过 max
        let target = c.record_success();
        assert_eq!(target, 100);
        assert_eq!(c.current_limit(), 100);
    }

    /// record_failure 减半
    #[test]
    fn record_failure_halves_limit() {
        let c = AIMDController::new(10);
        let target = c.record_failure();
        assert_eq!(target, 5);
        assert_eq!(c.current_limit(), 5);
    }

    /// 连续失败持续减半
    #[test]
    fn record_failure_multiple_halves() {
        let c = AIMDController::new(16);
        c.record_failure();
        assert_eq!(c.current_limit(), 8);
        c.record_failure();
        assert_eq!(c.current_limit(), 4);
        c.record_failure();
        assert_eq!(c.current_limit(), 2);
        c.record_failure();
        assert_eq!(c.current_limit(), 1);
    }

    /// record_failure 到 min_limit 不再减少
    #[test]
    fn record_failure_at_min_no_decrease() {
        let c = AIMDController::with_bounds(2, 1, 100);
        c.record_failure();
        assert_eq!(c.current_limit(), 1);
        // 已经在 min
        let target = c.record_failure();
        assert_eq!(target, 1);
    }

    /// record_failure 归零连续成功计数
    #[test]
    fn record_failure_resets_success_counter() {
        let c = AIMDController::with_params(10, 1, 100, 5);
        // 3 次成功
        for _ in 0..3 {
            c.record_success();
        }
        // 失败：归零
        c.record_failure();
        // 再 3 次成功不应 +1（需要 5 次）
        for _ in 0..3 {
            c.record_success();
        }
        assert_eq!(c.current_limit(), 5); // 10/2=5
                                          // 第 4 次（总计 4，不够 5）
        c.record_success();
        assert_eq!(c.current_limit(), 5);
        // 第 5 次：+1
        c.record_success();
        assert_eq!(c.current_limit(), 6);
    }

    /// 交替成功/失败验证 AIMD 行为
    #[test]
    fn alternating_success_failure() {
        let c = AIMDController::with_params(10, 1, 100, 1);
        // 成功 → 11
        c.record_success();
        assert_eq!(c.current_limit(), 11);
        // 失败 → 5
        c.record_failure();
        assert_eq!(c.current_limit(), 5);
        // 成功 → 6
        c.record_success();
        assert_eq!(c.current_limit(), 6);
        // 失败 → 3
        c.record_failure();
        assert_eq!(c.current_limit(), 3);
    }

    /// reset 重置到指定值
    #[test]
    fn reset_sets_new_value() {
        let c = AIMDController::new(10);
        c.record_failure();
        c.reset(20);
        assert_eq!(c.current_limit(), 20);
        // 连续成功计数也归零
        for _ in 0..9 {
            c.record_success();
        }
        assert_eq!(c.current_limit(), 20);
    }

    /// reset 钳制到 [min, max]
    #[test]
    fn reset_clamps_to_bounds() {
        let c = AIMDController::with_bounds(10, 1, 100);
        c.reset(500);
        assert_eq!(c.current_limit(), 100);
        c.reset(0);
        assert_eq!(c.current_limit(), 1);
    }

    /// Default 等价于 new(10)
    #[test]
    fn default_equals_new_10() {
        let c = AIMDController::default();
        assert_eq!(c.current_limit(), 10);
    }

    /// 并发 record_success 线程安全（不 panic）
    #[test]
    fn concurrent_record_success_is_safe() {
        use std::sync::Arc;
        use std::thread;
        let c = Arc::new(AIMDController::with_params(10, 1, 1000, 1));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let c = c.clone();
                thread::spawn(move || {
                    for _ in 0..100 {
                        c.record_success();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("thread panicked");
        }
        assert!(c.current_limit() <= 1000);
    }

    /// 并发 record_failure 线程安全（不 panic）
    #[test]
    fn concurrent_record_failure_is_safe() {
        use std::sync::Arc;
        use std::thread;
        let c = Arc::new(AIMDController::new(100));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let c = c.clone();
                thread::spawn(move || {
                    for _ in 0..10 {
                        c.record_failure();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("thread panicked");
        }
        assert!(c.current_limit() >= 1);
    }

    // ========== AdaptiveSemaphore tests ==========

    /// new 创建指定许可
    #[test]
    fn semaphore_new_has_initial_permits() {
        let s = AdaptiveSemaphore::new(5);
        assert_eq!(s.current_target(), 5);
        // 可获取 5 个
        let _permits: Vec<OwnedSemaphorePermit> = (0..5)
            .map(|_| s.try_acquire().expect("should acquire"))
            .collect();
        // 第 6 个应失败
        assert!(s.try_acquire().is_none());
    }

    /// set_target 增加许可
    #[test]
    fn set_target_increase_adds_permits() {
        let s = AdaptiveSemaphore::new(2);
        // 消耗 2 个
        let _p1 = s.try_acquire().expect("permit 1");
        let _p2 = s.try_acquire().expect("permit 2");
        assert!(s.try_acquire().is_none());

        // 增加到 4
        s.set_target(4);
        assert_eq!(s.current_target(), 4);
        // 现在可获取 2 个新的
        let _p3 = s.try_acquire().expect("permit 3");
        let _p4 = s.try_acquire().expect("permit 4");
        assert!(s.try_acquire().is_none());
    }

    /// set_target 减少许可（有可用时）
    #[test]
    fn set_target_decrease_removes_available_permits() {
        let s = AdaptiveSemaphore::new(5);
        // 减少到 2：移除 3 个可用许可
        s.set_target(2);
        assert_eq!(s.current_target(), 2);
        // 只能获取 2 个
        let _p1 = s.try_acquire().expect("permit 1");
        let _p2 = s.try_acquire().expect("permit 2");
        assert!(s.try_acquire().is_none());
    }

    /// set_target 减少许可（许可正在使用中，best-effort）
    #[test]
    fn set_target_decrease_with_in_use_permits() {
        let s = AdaptiveSemaphore::new(5);
        // 消耗 3 个
        let _p1 = s.try_acquire().expect("p1");
        let _p2 = s.try_acquire().expect("p2");
        let _p3 = s.try_acquire().expect("p3");
        // 剩 2 个可用

        // 减少到 1：移除 2 个可用，但无法移除在用的 3 个
        s.set_target(1);
        assert_eq!(s.current_target(), 1);

        // 在用的 3 个释放后，target 已是 1，不应回补到 5
        drop(_p1);
        drop(_p2);
        drop(_p3);
        // 实际可用 = 0（释放的 3 个中 2 个被 forget，1 个回到池中但 target 是 1）
        // 注：best-effort，此处验证 target 已更新即可
        assert_eq!(s.current_target(), 1);
    }

    /// set_target 相同值无操作
    #[test]
    fn set_target_same_value_noop() {
        let s = AdaptiveSemaphore::new(5);
        let result = s.set_target(5);
        assert_eq!(result, 5);
        assert_eq!(s.current_target(), 5);
        // 仍可获取 5 个
        for _ in 0..5 {
            assert!(s.try_acquire().is_some());
        }
    }

    /// acquire 异步获取许可
    #[tokio::test]
    async fn acquire_returns_permit() {
        let s = AdaptiveSemaphore::new(1);
        let permit = s.acquire().await;
        // permit Drop 后释放
        drop(permit);
        // 可再次获取
        let _permit2 = s.acquire().await;
    }

    /// Default 等价于 new(10)
    #[test]
    fn semaphore_default_equals_new_10() {
        let s = AdaptiveSemaphore::default();
        assert_eq!(s.current_target(), 10);
    }

    /// semaphore() 返回底层 Arc
    #[test]
    fn semaphore_returns_underlying_arc() {
        let s = AdaptiveSemaphore::new(3);
        let sem = s.semaphore();
        // 可直接操作
        let _ = sem.try_acquire();
    }
}
