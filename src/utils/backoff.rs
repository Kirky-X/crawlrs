// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Backoff delay（full-jitter）— design.md §4 / R-identity-002
//!
//! 移植自 spider 的 `backoff_delay(attempt, base_ms, max_ms)`：
//! - 指数上限：`cap = min(base_ms * 2^attempt, max_ms)`
//! - full-jitter：从 `[0, cap]` 均匀采样（AWS "full jitter" 策略，
//!   相比对称 jitter 能更有效避免 thundering-herd）
//! - thread-local xorshift32 RNG：避免全局锁，每线程独立状态

use std::cell::Cell;
use std::time::Duration;

// thread-local xorshift32 状态（非零）
thread_local! {
    static RNG_STATE: Cell<u32> = Cell::new(seed_from_time());
}

/// 从系统时间生成非零种子；失败则用 fallback 常量。
fn seed_from_time() -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0xCAFEBABE);
    if nanos == 0 {
        0xCAFEBABE
    } else {
        nanos
    }
}

/// xorshift32 推进一步，返回下一个 u32 伪随机数。
fn next_u32() -> u32 {
    RNG_STATE.with(|s| {
        let mut x = s.get();
        if x == 0 {
            x = 0xCAFEBABE;
        }
        // Marsaglia xorshift32 三移位
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        s.set(x);
        x
    })
}

/// 从 `[0, max]` 采样一个 u64（均匀分布，full-jitter 用）。
///
/// 拼接两次 xorshift32 得到 64-bit 值后取模。
/// 由于 `cap` 是毫秒级、上界有限（max_ms），偏差在工程可接受范围内。
fn gen_range_u64_inclusive(max: u64) -> u64 {
    if max == 0 {
        return 0;
    }
    let hi = next_u32() as u64;
    let lo = next_u32() as u64;
    let r = (hi << 32) | lo;
    r % (max + 1)
}

/// full-jitter 退避延迟。
///
/// - `attempt`：重试次数（0 = 首次尝试前的退避）；上限保护用饱和乘法。
/// - `base_ms`：基础退避毫秒；为 0 时直接返回零。
/// - `max_ms`：退避上限毫秒。
///
/// 返回值 ∈ `[0, min(base_ms * 2^attempt, max_ms)]`。
///
/// # Panics
/// 本函数不 panic：`attempt` 极大时使用 `saturating_mul` / `saturating_pow` 保护。
#[must_use]
pub fn backoff_delay(attempt: u32, base_ms: u64, max_ms: u64) -> Duration {
    if base_ms == 0 {
        return Duration::ZERO;
    }
    let exp = 2u64.saturating_pow(attempt);
    let cap = base_ms.saturating_mul(exp).min(max_ms);
    let delay_ms = gen_range_u64_inclusive(cap);
    Duration::from_millis(delay_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R-identity-002: 所有采样结果 ∈ [0, min(base*2^attempt, max)]
    #[test]
    fn delay_in_range() {
        let base_ms = 100u64;
        let max_ms = 10_000u64;
        for attempt in 0u32..6 {
            let exp = 2u64.saturating_pow(attempt);
            let expected_cap = base_ms.saturating_mul(exp).min(max_ms);
            for _ in 0..1000 {
                let d = backoff_delay(attempt, base_ms, max_ms);
                let ms = d.as_millis() as u64;
                assert!(
                    ms <= expected_cap,
                    "attempt={}: delay {}ms exceeds cap {}ms",
                    attempt,
                    ms,
                    expected_cap
                );
            }
        }
    }

    /// R-identity-002: 高 attempt 均值更大（指数退避特性）
    #[test]
    fn higher_attempt_higher_mean() {
        let base_ms = 100u64;
        let max_ms = 10_000u64;

        let mean_low = sample_mean(0, base_ms, max_ms, 1000);
        let mean_high = sample_mean(5, base_ms, max_ms, 1000);

        // attempt=5 时 cap = min(100*32, 10000) = 3200 ms，均值约为 cap/2 = 1600ms
        // attempt=0 时 cap = min(100, 10000) = 100 ms，均值约为 50ms
        assert!(
            mean_high > mean_low,
            "expected higher attempt ({}) to have larger mean than attempt=0 (mean={})",
            mean_high,
            mean_low
        );
        // 强校验：attempt=5 的均值至少是 attempt=0 的 5 倍
        assert!(
            mean_high > mean_low * 5.0,
            "expected mean_high (>5x mean_low), got mean_high={}, mean_low={}",
            mean_high,
            mean_low
        );
    }

    /// max_ms 应严格 cap 结果（不超出）
    #[test]
    fn max_ms_caps_delay() {
        let base_ms = 1_000u64;
        let max_ms = 500u64; // 比 base 还小，触发 cap
        for _ in 0..1000 {
            let d = backoff_delay(3, base_ms, max_ms);
            let ms = d.as_millis() as u64;
            assert!(ms <= max_ms, "delay {}ms exceeds max_ms {}", ms, max_ms);
        }
    }

    /// attempt 极大不应 panic（saturating 保护）
    #[test]
    fn overflow_safe() {
        // 2^100 远超 u64 范围，saturating_pow 应 cap 到 u64::MAX
        let d = backoff_delay(100, 100, 60_000);
        let ms = d.as_millis() as u64;
        assert!(ms <= 60_000, "overflow case: delay must be capped at max_ms");
    }

    /// base_ms=0 时返回零延迟（边界）
    #[test]
    fn zero_base_returns_zero() {
        let d = backoff_delay(5, 0, 1000);
        assert_eq!(d, Duration::ZERO);
    }

    /// attempt=0 时 cap=base_ms（边界）
    #[test]
    fn attempt_zero_cap_equals_base() {
        let base_ms = 200u64;
        let max_ms = 10_000u64;
        for _ in 0..500 {
            let d = backoff_delay(0, base_ms, max_ms);
            let ms = d.as_millis() as u64;
            assert!(ms <= base_ms, "attempt=0 delay {} exceeds base {}", ms, base_ms);
        }
    }

    fn sample_mean(attempt: u32, base_ms: u64, max_ms: u64, n: usize) -> f64 {
        let mut sum = 0u64;
        for _ in 0..n {
            sum += backoff_delay(attempt, base_ms, max_ms).as_millis() as u64;
        }
        sum as f64 / n as f64
    }
}
