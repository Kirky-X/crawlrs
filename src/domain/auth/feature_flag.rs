// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Implementation of [`FeatureFlag`].

use super::FeatureFlag;
use uuid::Uuid;

impl FeatureFlag {
    /// Check if the feature is currently active
    pub fn is_active(&self) -> bool {
        self.enabled
            && self.started_at.is_none_or(|t| t <= chrono::Utc::now())
            && self.stopped_at.is_none_or(|t| t > chrono::Utc::now())
    }

    /// Check if a specific API Key should have access based on rollout
    pub fn should_enable_for_key(&self, api_key_id: Uuid) -> bool {
        if !self.is_active() {
            return false;
        }

        if self.rollout_percentage == 100 {
            return true;
        }

        if self.rollout_percentage == 0 {
            return false;
        }

        // Deterministic rollout based on API Key ID
        let bytes = api_key_id.as_bytes();
        let mut hash: u64 = 0;
        for &byte in bytes {
            hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
        }
        let bucket = hash % 100;
        bucket < self.rollout_percentage as u64
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::auth::FeatureFlag;
    use uuid::Uuid;

    fn make_flag(
        enabled: bool,
        rollout_percentage: u8,
        started_at: Option<chrono::DateTime<chrono::Utc>>,
        stopped_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> FeatureFlag {
        FeatureFlag {
            id: Uuid::new_v4(),
            name: "test_flag".to_string(),
            description: None,
            enabled,
            rollout_percentage,
            metadata: serde_json::json!({}),
            started_at,
            stopped_at,
        }
    }

    #[test]
    fn test_is_active_disabled_flag_returns_false() {
        let flag = make_flag(false, 100, None, None);
        assert!(!flag.is_active());
    }

    #[test]
    fn test_is_active_enabled_no_time_window_returns_true() {
        let flag = make_flag(true, 100, None, None);
        assert!(flag.is_active());
    }

    #[test]
    fn test_is_active_started_at_in_future_returns_false() {
        let future = chrono::Utc::now() + chrono::Duration::hours(1);
        let flag = make_flag(true, 100, Some(future), None);
        assert!(!flag.is_active());
    }

    #[test]
    fn test_is_active_started_at_in_past_returns_true() {
        let past = chrono::Utc::now() - chrono::Duration::hours(1);
        let flag = make_flag(true, 100, Some(past), None);
        assert!(flag.is_active());
    }

    #[test]
    fn test_is_active_stopped_at_in_past_returns_false() {
        let past = chrono::Utc::now() - chrono::Duration::hours(1);
        let flag = make_flag(true, 100, None, Some(past));
        assert!(!flag.is_active());
    }

    #[test]
    fn test_is_active_stopped_at_in_future_returns_true() {
        let future = chrono::Utc::now() + chrono::Duration::hours(1);
        let flag = make_flag(true, 100, None, Some(future));
        assert!(flag.is_active());
    }

    #[test]
    fn test_is_active_started_at_exact_now_returns_true() {
        // 边界条件：started_at <= now 为 active（使用当前时间）
        let now = chrono::Utc::now();
        let flag = make_flag(true, 100, Some(now), None);
        assert!(flag.is_active());
    }

    #[test]
    fn test_is_active_within_active_time_window_returns_true() {
        let past = chrono::Utc::now() - chrono::Duration::days(1);
        let future = chrono::Utc::now() + chrono::Duration::days(1);
        let flag = make_flag(true, 100, Some(past), Some(future));
        assert!(flag.is_active());
    }

    #[test]
    fn test_is_active_outside_active_time_window_returns_false() {
        // started_at 在未来 + stopped_at 在更远的未来：当前不在窗口内
        let future1 = chrono::Utc::now() + chrono::Duration::days(1);
        let future2 = chrono::Utc::now() + chrono::Duration::days(2);
        let flag = make_flag(true, 100, Some(future1), Some(future2));
        assert!(!flag.is_active());
    }

    #[test]
    fn test_should_enable_for_key_disabled_flag_returns_false_even_at_100() {
        let flag = make_flag(false, 100, None, None);
        let api_key_id = Uuid::new_v4();
        assert!(!flag.should_enable_for_key(api_key_id));
    }

    #[test]
    fn test_should_enable_for_key_100_percent_returns_true_for_any_key() {
        let flag = make_flag(true, 100, None, None);
        // 多个不同的 key 在 100% rollout 下都应启用
        for _ in 0..10 {
            assert!(flag.should_enable_for_key(Uuid::new_v4()));
        }
    }

    #[test]
    fn test_should_enable_for_key_0_percent_returns_false_for_any_key() {
        let flag = make_flag(true, 0, None, None);
        for _ in 0..10 {
            assert!(!flag.should_enable_for_key(Uuid::new_v4()));
        }
    }

    #[test]
    fn test_should_enable_for_key_nil_uuid_returns_deterministic_result() {
        let flag = make_flag(true, 50, None, None);
        let nil = Uuid::nil();
        let first = flag.should_enable_for_key(nil);
        let second = flag.should_enable_for_key(nil);
        // 相同 key 必须返回相同结果
        assert_eq!(first, second);
    }

    #[test]
    fn test_should_enable_for_key_returns_false_when_flag_inactive() {
        // started_at 在未来 -> is_active() = false -> should_enable_for_key 必为 false
        let future = chrono::Utc::now() + chrono::Duration::hours(1);
        let flag = make_flag(true, 100, Some(future), None);
        let api_key_id = Uuid::new_v4();
        assert!(!flag.should_enable_for_key(api_key_id));
    }

    #[test]
    fn test_should_enable_for_key_partial_rollout_distribution_within_bounds() {
        // 30% rollout 在 1000 个 key 中应产生约 30% 启用率，至少应同时有启用和禁用样本
        let flag = make_flag(true, 30, None, None);
        let mut enabled_count = 0;
        let total = 1000;
        for _ in 0..total {
            if flag.should_enable_for_key(Uuid::new_v4()) {
                enabled_count += 1;
            }
        }
        assert!(enabled_count > 0, "expected at least some keys enabled");
        assert!(
            enabled_count < total,
            "expected at least some keys disabled"
        );
        // 启用率应在合理范围内（10% ~ 50%）以排除严重 bug
        let ratio = enabled_count as f64 / total as f64;
        assert!(
            ratio > 0.10 && ratio < 0.50,
            "ratio out of expected range: {}",
            ratio
        );
    }

    #[test]
    fn test_should_enable_for_key_deterministic_for_multiple_keys() {
        // 同一组 api_key_id 在多次调用下必须返回相同结果
        let flag = make_flag(true, 50, None, None);
        let keys: Vec<Uuid> = (0..20).map(|_| Uuid::new_v4()).collect();
        for key in &keys {
            let first = flag.should_enable_for_key(*key);
            let second = flag.should_enable_for_key(*key);
            let third = flag.should_enable_for_key(*key);
            assert_eq!(first, second);
            assert_eq!(second, third);
        }
    }
}
