// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Backoff delay — 基于 backon 的指数退避 + jitter
//!
//! 使用 `backon::ExponentialBuilder` 替代手写 xorshift32 + full-jitter 实现：
//! - 指数退避：`delay = min * factor^n`
//! - jitter：启用时在 `(1-jitter)..=(1+jitter)` 范围随机化（默认 full jitter = 1.0）
//! - 上限 cap：`min(delay, max)`
//!
//! 相比旧实现（full-jitter: `[0, cap]` 均匀采样），backon 使用
//! "jitter around exponential" 策略，在工程实践中同样有效避免 thundering-herd。

use std::time::Duration;

use backon::{BackoffBuilder, ExponentialBuilder};

/// 指数退避延迟（基于 backon）。
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

    let backoff = ExponentialBuilder::default()
        .with_min_delay(Duration::from_millis(base_ms))
        .with_max_delay(Duration::from_millis(max_ms))
        .with_jitter()
        .with_max_times((attempt + 1) as usize);

    let max = Duration::from_millis(max_ms);
    backoff
        .build()
        .nth(attempt as usize)
        .unwrap_or(max)
        .min(max)
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
