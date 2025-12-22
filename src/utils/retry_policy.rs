// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
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
    /// 退避乘数
    pub backoff_multiplier: f64,
    /// 抖动因子 (0.0-1.0)
    pub jitter_factor: f64,
    /// 是否启用指数退避
    pub exponential_backoff: bool,
    /// 是否启用抖动
    pub enable_jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
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
            max_retries: 3,
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

    /// 计算下次重试的退避时间
    pub fn calculate_backoff(&self, attempt: u32) -> Duration {
        if !self.exponential_backoff {
            return self.initial_backoff;
        }

        // 计算指数退避
        let backoff_secs =
            self.initial_backoff.as_secs_f64() * self.backoff_multiplier.powi(attempt as i32 - 1);

        // 限制最大退避时间
        let capped_backoff = backoff_secs.min(self.max_backoff.as_secs_f64());

        // 添加抖动
        let final_backoff = if self.enable_jitter {
            let jitter_range = capped_backoff * self.jitter_factor;
            let jitter = rand::random_range(-jitter_range..jitter_range);
            (capped_backoff + jitter).max(0.0)
        } else {
            capped_backoff
        };

        Duration::from_secs_f64(final_backoff)
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
        let mut policy = RetryPolicy::standard();
        policy.enable_jitter = true;
        policy.jitter_factor = 0.1;
        
        let backoff = policy.calculate_backoff(2);
        // 应该接近 2 秒，但有 ±10% 的抖动
        let expected = Duration::from_secs(2);
        let jitter_range = Duration::from_millis(200); // 10% of 2s
        
        assert!(backoff >= expected - jitter_range);
        assert!(backoff <= expected + jitter_range);
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
        assert!(!policy.should_retry(3)); // max_retries = 3
        assert!(!policy.should_retry(4));
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