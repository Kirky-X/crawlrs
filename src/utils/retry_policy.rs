// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use chrono::{DateTime, Utc};
use std::time::Duration;

/// 重试策略配置
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// 最大重试次数
    pub max_retries: u32,
    /// 初始退避时间
    pub initial_backoff: Duration,
    /// 最大退避时间
    pub max_backoff: Duration,
    /// 退避乘数（仅在 `exponential_backoff=true` 时生效；目前固定 2.0，
    /// 因 `backoff::backoff_delay` 用 `2^attempt` 标准 full-jitter 公式，
    /// 此字段保留用于未来自定义乘数扩展）
    pub backoff_multiplier: f64,
    /// 抖动因子 (0.0-1.0)；保留用于向后兼容字段读取，
    /// 实际 jitter 行为由 `enable_jitter` 开关决定（full-jitter 全开/全关）
    pub jitter_factor: f64,
    /// 是否启用指数退避
    pub exponential_backoff: bool,
    /// 是否启用抖动（true=full-jitter，false=deterministic cap）
    pub enable_jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            jitter_factor: 0.1,
            exponential_backoff: true,
            enable_jitter: true,
        }
    }
}

impl RetryPolicy {
    /// 创建标准重试策略
    pub fn standard() -> Self {
        Self::default()
    }

    /// 创建快速重试策略（更短的退避时间）
    pub fn fast() -> Self {
        Self {
            max_retries: 5,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(10),
            backoff_multiplier: 1.5,
            jitter_factor: 0.1,
            exponential_backoff: true,
            enable_jitter: true,
        }
    }

    /// 创建慢速重试策略（更长的退避时间，适合网络请求）
    pub fn slow() -> Self {
        Self {
            max_retries: 5,
            initial_backoff: Duration::from_secs(2),
            max_backoff: Duration::from_secs(300), // 5分钟
            backoff_multiplier: 2.0,
            jitter_factor: 0.2,
            exponential_backoff: true,
            enable_jitter: true,
        }
    }

    /// 计算下次重试的退避时间（T024：改用 full-jitter）
    ///
    /// - `attempt=0` 退化为 `initial_backoff`（保护边界，spider 公式 attempt-1 不合法）
    /// - 启用 jitter 时调用 [`backoff::backoff_delay`]（full-jitter：`[0, cap]` 均匀采样）
    /// - 禁用 jitter 时返回 deterministic cap（指数 + max cap，无随机）
    ///
    /// 调用方约定：`attempt=1` 表示第一次重试，对应 spider 公式 `attempt=0`。
    pub fn calculate_backoff(&self, attempt: u32) -> Duration {
        if !self.exponential_backoff {
            return self.initial_backoff;
        }
        if attempt == 0 {
            // 边界保护：spider 公式 attempt-1 不合法，退化为 initial_backoff
            return self.initial_backoff;
        }

        let base_ms = self.initial_backoff.as_millis() as u64;
        let max_ms = self.max_backoff.as_millis() as u64;
        // spider 的 attempt=0 → 第一次重试；现有 API attempt=1 → 第一次重试
        let spider_attempt = attempt - 1;

        if self.enable_jitter {
            // full-jitter：[0, cap] 均匀采样（R-identity-002）
            crate::utils::backoff::backoff_delay(spider_attempt, base_ms, max_ms)
        } else {
            // 禁用 jitter：deterministic cap（指数 + max cap）
            let exp = 2u64.saturating_pow(spider_attempt);
            let cap_ms = base_ms.saturating_mul(exp).min(max_ms);
            Duration::from_millis(cap_ms)
        }
    }

    /// 计算下次重试时间
    pub fn next_retry_time(&self, attempt: u32, base_time: DateTime<Utc>) -> DateTime<Utc> {
        let backoff = self.calculate_backoff(attempt);
        base_time + chrono::Duration::milliseconds(backoff.as_millis() as i64)
    }

    /// 是否应该重试
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_retries
    }

    /// 根据错误类型判断是否应该重试
    pub fn should_retry_with_error(&self, attempt: u32, error: &anyhow::Error) -> bool {
        attempt < self.max_retries && is_retryable_error(error)
    }
}

/// 判断错误是否可重试
pub fn is_retryable_error(error: &anyhow::Error) -> bool {
    let error_string = error.to_string().to_lowercase();

    // 网络相关错误可重试
    let retryable_patterns = [
        "timeout",
        "connection reset",
        "connection refused",
        "dns error",
        "500 internal server error",
        "502 bad gateway",
        "503 service unavailable",
        "504 gateway timeout",
        "network is unreachable",
        "broken pipe",
        "too many connections",
        "rate limit",
    ];

    retryable_patterns.iter().any(|&p| error_string.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_backoff_exponential() {
        let mut policy = RetryPolicy::standard();
        policy.enable_jitter = false; // 禁用抖动以获得精确值

        // 第一次重试 (attempt = 1)
        let backoff1 = policy.calculate_backoff(1);
        assert_eq!(backoff1, Duration::from_secs(1));

        // 第二次重试 (attempt = 2)
        let backoff2 = policy.calculate_backoff(2);
        assert_eq!(backoff2, Duration::from_secs(2)); // 1 * 2^1

        // 第三次重试 (attempt = 3)
        let backoff3 = policy.calculate_backoff(3);
        assert_eq!(backoff3, Duration::from_secs(4)); // 1 * 2^2
    }

    #[test]
    fn test_calculate_backoff_with_jitter() {
        // T024: 改用 full-jitter（[0, cap] 均匀采样）替代对称 ±10% jitter
        let mut policy = RetryPolicy::standard();
        policy.enable_jitter = true;

        // attempt=2 → spider_attempt=1 → cap = min(1000*2^1, 60000) = 2000ms
        // full-jitter: 均匀采样 [0, 2000ms]
        let expected_cap = Duration::from_millis(2000);

        // 采样多次验证结果 ∈ [0, cap]（Duration 本身非负，只需校验上界）
        for _ in 0..1000 {
            let backoff = policy.calculate_backoff(2);
            assert!(
                backoff <= expected_cap,
                "full-jitter backoff {:?} exceeds cap {:?}",
                backoff,
                expected_cap
            );
        }
    }

    /// T024 额外测试：full-jitter 高 attempt 均值更大（指数退避特性）
    #[test]
    fn test_calculate_backoff_full_jitter_higher_attempt_higher_mean() {
        let mut policy = RetryPolicy::standard();
        policy.enable_jitter = true;

        let mean_low = sample_backoff_mean(&policy, 1, 500); // cap=1000ms
        let mean_high = sample_backoff_mean(&policy, 5, 500); // cap=min(1000*2^4, 60000)=16000ms
        assert!(
            mean_high > mean_low * 5.0,
            "expected mean_high (>5x mean_low), got high={}, low={}",
            mean_high,
            mean_low
        );
    }

    /// T024 额外测试：attempt=0 边界保护
    #[test]
    fn test_calculate_backoff_attempt_zero_returns_initial() {
        let mut policy = RetryPolicy::standard();
        policy.enable_jitter = true;
        policy.initial_backoff = Duration::from_millis(800);

        // attempt=0 退化为 initial_backoff（不调用 backoff_delay）
        let backoff = policy.calculate_backoff(0);
        assert_eq!(backoff, Duration::from_millis(800));
    }

    fn sample_backoff_mean(policy: &RetryPolicy, attempt: u32, n: usize) -> f64 {
        let mut sum = 0u64;
        for _ in 0..n {
            sum += policy.calculate_backoff(attempt).as_millis() as u64;
        }
        sum as f64 / n as f64
    }

    #[test]
    fn test_calculate_backoff_max_limit() {
        let mut policy = RetryPolicy::standard();
        policy.max_backoff = Duration::from_secs(5);
        policy.enable_jitter = false; // 禁用抖动以获得精确值

        // 尝试计算一个会超过最大值的退避时间
        let backoff = policy.calculate_backoff(10);
        assert_eq!(backoff, Duration::from_secs(5)); // 被限制在最大值
    }

    #[test]
    fn test_should_retry() {
        let policy = RetryPolicy::standard();

        assert!(policy.should_retry(0));
        assert!(policy.should_retry(1));
        assert!(policy.should_retry(2));
        assert!(policy.should_retry(3));
        assert!(policy.should_retry(4));
        assert!(!policy.should_retry(5)); // max_retries = 5
        assert!(!policy.should_retry(6));
    }

    #[test]
    fn test_next_retry_time() {
        use chrono::TimeZone;

        let mut policy = RetryPolicy::standard();
        policy.enable_jitter = false; // 禁用抖动以获得精确值

        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();

        let next_retry = policy.next_retry_time(2, base_time);
        let expected = base_time + chrono::Duration::seconds(2);

        assert_eq!(next_retry, expected);
    }

    #[test]
    fn test_calculate_backoff_non_exponential_returns_initial_backoff() {
        // Cover line 77: when exponential_backoff is false, the function
        // returns initial_backoff immediately without exponential calculation.
        let mut policy = RetryPolicy::standard();
        policy.exponential_backoff = false;
        policy.enable_jitter = false;
        policy.initial_backoff = Duration::from_secs(3);

        let backoff1 = policy.calculate_backoff(1);
        let backoff5 = policy.calculate_backoff(5);

        assert_eq!(backoff1, Duration::from_secs(3));
        assert_eq!(backoff5, Duration::from_secs(3));
    }
}
