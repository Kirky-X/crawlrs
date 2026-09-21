// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Backoff delay — 基于 limiteron retry 的指数退避 + jitter
//!
//! 2026-09 自研库收敛：延迟公式由 `limiteron::retry::delay_for_attempt` 承载
//! （与 `RetryPolicy::execute` 的内部退避共用同一公式）：
//! - 指数退避：`delay = base * 2^(attempt-1)`，封顶 `max`
//! - jitter：`[1, 1+jitter]` 比例随机化（jitter=1.0，等效打散重试尖峰）
//!
//! 与旧 backon 的 "jitter around exponential"（±50%）分布不同但同属
//! anti-thundering-herd 抖动；对外契约（≤ max、均值随 attempt 增大）不变。

use std::time::Duration;

use limiteron::retry::delay_for_attempt;

/// 指数退避延迟（基于 limiteron retry）。
///
/// - `attempt`：重试次数（0 = 首次重试前的退避）
/// - `base_ms`：基础退避毫秒（min delay）
/// - `max_ms`：退避上限毫秒（max delay）
///
/// 返回带 jitter 的指数退避延迟。
///
/// # Panics
/// 本函数不 panic。
#[must_use]
pub fn backoff_delay(attempt: u32, base_ms: u64, max_ms: u64) -> Duration {
    if base_ms == 0 {
        return Duration::ZERO;
    }

    if attempt == 0 {
        // 首次重试前按 base 退避（jitter=1.0 与后续 attempt 一致）
        return delay_for_attempt(
            1,
            Duration::from_millis(base_ms),
            2.0,
            Duration::from_millis(max_ms),
            1.0,
        );
    }
    delay_for_attempt(
        attempt,
        Duration::from_millis(base_ms),
        2.0,
        Duration::from_millis(max_ms),
        1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 所有采样结果应 ≤ max_ms
    #[test]
    fn delay_capped_at_max() {
        let base_ms = 100u64;
        let max_ms = 10_000u64;
        for attempt in 0u32..6 {
            for _ in 0..1000 {
                let d = backoff_delay(attempt, base_ms, max_ms);
                assert!(
                    d.as_millis() as u64 <= max_ms,
                    "attempt={}: delay {:?} exceeds max_ms {}",
                    attempt,
                    d,
                    max_ms
                );
            }
        }
    }

    /// 高 attempt 均值更大（指数退避特性）
    #[test]
    fn higher_attempt_higher_mean() {
        let base_ms = 100u64;
        let max_ms = 10_000u64;

        let mean_low = sample_mean(0, base_ms, max_ms, 1000);
        let mean_high = sample_mean(5, base_ms, max_ms, 1000);

        assert!(
            mean_high > mean_low,
            "expected higher attempt to have larger mean: high={}, low={}",
            mean_high,
            mean_low
        );
    }

    /// max_ms 应严格 cap 结果
    #[test]
    fn max_ms_caps_delay() {
        let base_ms = 1_000u64;
        let max_ms = 500u64;
        for _ in 0..1000 {
            let d = backoff_delay(3, base_ms, max_ms);
            assert!(
                d.as_millis() as u64 <= max_ms,
                "delay {:?} exceeds max_ms {}",
                d,
                max_ms
            );
        }
    }

    /// attempt 极大不应 panic
    #[test]
    fn overflow_safe() {
        let d = backoff_delay(100, 100, 60_000);
        assert!(
            d.as_millis() as u64 <= 60_000,
            "overflow case: delay must be capped at max_ms"
        );
    }

    /// base_ms=0 时返回零延迟
    #[test]
    fn zero_base_returns_zero() {
        let d = backoff_delay(5, 0, 1000);
        assert_eq!(d, Duration::ZERO);
    }

    fn sample_mean(attempt: u32, base_ms: u64, max_ms: u64, n: usize) -> f64 {
        let mut sum = 0u64;
        for _ in 0..n {
            sum += backoff_delay(attempt, base_ms, max_ms).as_millis() as u64;
        }
        sum as f64 / n as f64
    }
}
