// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 重试原因分类与计数器（design.md §4 — crawler-capability-absorption Stage 1/2）
//!
//! `RetryReason` 将 `EngineError` 归类为三类重试策略语义，供 `EngineRouter` /
//! `scrape_worker` 决策如何重试（同引擎 vs 换引擎 vs 切身份）。
//!
//! `RetryTracker`（T025 补齐）为各 reason 维护独立计数与上限：
//! - `Transient` 上限较高（瞬时故障通常可同引擎恢复）
//! - `FeatureToggle` 上限中等（特性切换需换引擎）
//! - `AntiBot` 上限较低（反爬命中后切身份空间有限，避免无谓重试）

/// 重试原因分类 — 驱动重试策略选择。
///
/// - [`RetryReason::Transient`]：瞬时故障（网络抖动、超时、浏览器崩溃），
///   可同引擎重试（design.md §4：RequestFailed/Timeout/BrowserError）。
/// - [`RetryReason::FeatureToggle`]：引擎特性切换（如 Chrome 降级到 HTTP），
///   需换引擎重试（T027 补 `EngineError::FeatureToggle` 变体后启用）。
/// - [`RetryReason::AntiBot`]：反爬虫检测命中，需切换身份（UA/代理/stealth）
///   + 强制浏览器引擎重试（design.md §1.6/1.7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RetryReason {
    /// 瞬时故障 — 同引擎可重试
    Transient,
    /// 引擎特性切换 — 需换引擎重试
    FeatureToggle,
    /// 反爬虫命中 — 需切身份 + 浏览器引擎重试
    AntiBot,
}

/// 重试计数器 — 各 reason 独立计数与上限（design.md §4，T025）
///
/// 默认上限（[`RetryTracker::new_default`]）：
/// - `max_total = 5`：总重试次数硬上限
/// - `max_feature_toggle = 3`：特性切换重试上限
/// - `max_anti_bot = 2`：反爬重试上限（避免无谓身份轮换）
///
/// `Transient` 没有独立计数器字段：它受 `max_total` 间接约束，
/// 因为瞬时故障通常可同引擎恢复，无需独立 cap。
#[derive(Debug, Clone)]
pub struct RetryTracker {
    /// 总重试次数（所有 reason 累计）
    total: u32,
    /// `FeatureToggle` 累计次数
    feature_toggle: u32,
    /// `AntiBot` 累计次数
    anti_bot: u32,
    /// 总重试次数硬上限
    max_total: u32,
    /// `FeatureToggle` 重试上限
    max_feature_toggle: u32,
    /// `AntiBot` 重试上限
    max_anti_bot: u32,
}

impl Default for RetryTracker {
    fn default() -> Self {
        Self::new_default()
    }
}

impl RetryTracker {
    /// 构造默认上限的 tracker：max_total=5, max_feature_toggle=3, max_anti_bot=2
    #[must_use]
    pub fn new_default() -> Self {
        Self {
            total: 0,
            feature_toggle: 0,
            anti_bot: 0,
            max_total: 5,
            max_feature_toggle: 3,
            max_anti_bot: 2,
        }
    }

    /// 构造自定义上限的 tracker。
    #[must_use]
    pub fn new(max_total: u32, max_feature_toggle: u32, max_anti_bot: u32) -> Self {
        Self {
            total: 0,
            feature_toggle: 0,
            anti_bot: 0,
            max_total,
            max_feature_toggle,
            max_anti_bot,
        }
    }

    /// 记录一次重试（计数自增）。
    ///
    /// 调用方应在每次失败后、调用 `should_retry` 前调用本方法。
    pub fn record(&mut self, r: RetryReason) {
        self.total = self.total.saturating_add(1);
        match r {
            RetryReason::Transient => {}
            RetryReason::FeatureToggle => {
                self.feature_toggle = self.feature_toggle.saturating_add(1);
            }
            RetryReason::AntiBot => {
                self.anti_bot = self.anti_bot.saturating_add(1);
            }
        }
    }

    /// 判断是否还可对该 reason 重试。
    ///
    /// 规则：对应 reason 计数 < 对应上限 **且** 总计数 < `max_total`。
    /// - `Transient` 仅受 `max_total` 约束（无独立计数器）
    /// - `FeatureToggle` 受 `max_feature_toggle` 与 `max_total` 双重约束
    /// - `AntiBot` 受 `max_anti_bot` 与 `max_total` 双重约束
    ///
    /// 注：`record` 在 `should_retry` 之前调用，因此 `should_retry` 检查的是
    /// "已记录"次数是否到达上限（达上限即停止）。
    pub fn should_retry(&self, r: RetryReason) -> bool {
        if self.total >= self.max_total {
            return false;
        }
        match r {
            RetryReason::Transient => true,
            RetryReason::FeatureToggle => self.feature_toggle < self.max_feature_toggle,
            RetryReason::AntiBot => self.anti_bot < self.max_anti_bot,
        }
    }

    /// 当前总重试次数
    #[must_use]
    pub fn total(&self) -> u32 {
        self.total
    }

    /// `FeatureToggle` 累计次数
    #[must_use]
    pub fn feature_toggle(&self) -> u32 {
        self.feature_toggle
    }

    /// `AntiBot` 累计次数
    #[must_use]
    pub fn anti_bot(&self) -> u32 {
        self.anti_bot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_reason_equality() {
        assert_eq!(RetryReason::Transient, RetryReason::Transient);
        assert_eq!(RetryReason::FeatureToggle, RetryReason::FeatureToggle);
        assert_eq!(RetryReason::AntiBot, RetryReason::AntiBot);
        assert_ne!(RetryReason::Transient, RetryReason::AntiBot);
        assert_ne!(RetryReason::FeatureToggle, RetryReason::Transient);
        assert_ne!(RetryReason::AntiBot, RetryReason::FeatureToggle);
    }

    #[test]
    fn retry_reason_copy_clone() {
        let reason = RetryReason::AntiBot;
        let cloned = reason;
        assert_eq!(reason, cloned);
    }

    #[test]
    fn retry_reason_debug_format() {
        assert_eq!(format!("{:?}", RetryReason::Transient), "Transient");
        assert_eq!(format!("{:?}", RetryReason::FeatureToggle), "FeatureToggle");
        assert_eq!(format!("{:?}", RetryReason::AntiBot), "AntiBot");
    }

    #[test]
    fn retry_reason_hash_consistency() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(RetryReason::Transient);
        set.insert(RetryReason::Transient);
        assert_eq!(set.len(), 1);
        set.insert(RetryReason::AntiBot);
        assert_eq!(set.len(), 2);
    }

    // === RetryTracker tests (T025, R-identity-002) ===

    #[test]
    fn tracker_default_initial_counts_zero() {
        let t = RetryTracker::new_default();
        assert_eq!(t.total(), 0);
        assert_eq!(t.feature_toggle(), 0);
        assert_eq!(t.anti_bot(), 0);
    }

    #[test]
    fn tracker_default_allows_initial_retry() {
        let t = RetryTracker::new_default();
        assert!(t.should_retry(RetryReason::Transient));
        assert!(t.should_retry(RetryReason::FeatureToggle));
        assert!(t.should_retry(RetryReason::AntiBot));
    }

    /// R-identity-002: AntiBot 达上限后仅 AntiBot 停止，Transient 仍可重试
    #[test]
    fn tracker_antibot_cap_stops_only_antibot() {
        let mut t = RetryTracker::new_default();
        // 默认 max_anti_bot = 2
        t.record(RetryReason::AntiBot);
        assert!(
            t.should_retry(RetryReason::AntiBot),
            "after 1 antibot: still allowed"
        );
        assert!(t.should_retry(RetryReason::Transient), "transient still ok");

        t.record(RetryReason::AntiBot);
        // 已记录 2 次 anti_bot → should_retry(AntiBot) 现在应 false
        assert!(
            !t.should_retry(RetryReason::AntiBot),
            "after 2 antibot records: should be blocked"
        );
        // Transient 仍可重试（仅受 max_total 约束，total=2 < 5）
        assert!(
            t.should_retry(RetryReason::Transient),
            "transient must still retry when antibot cap reached"
        );
    }

    /// R-identity-002: FeatureToggle 达上限后仅 FeatureToggle 停止
    #[test]
    fn tracker_feature_toggle_cap_stops_only_ft() {
        let mut t = RetryTracker::new_default();
        // max_feature_toggle = 3
        t.record(RetryReason::FeatureToggle);
        t.record(RetryReason::FeatureToggle);
        t.record(RetryReason::FeatureToggle);
        assert!(
            !t.should_retry(RetryReason::FeatureToggle),
            "after 3 feature_toggle: blocked"
        );
        assert!(t.should_retry(RetryReason::Transient), "transient still ok");
        assert!(t.should_retry(RetryReason::AntiBot), "antibot still ok");
    }

    /// R-identity-002: max_total 是硬上限，达上限后所有 reason 都停止
    #[test]
    fn tracker_max_total_blocks_all() {
        let mut t = RetryTracker::new_default();
        // 用 Transient 填满 max_total=5（不受独立 cap 约束）
        for _ in 0..5 {
            t.record(RetryReason::Transient);
        }
        assert_eq!(t.total(), 5);
        assert!(!t.should_retry(RetryReason::Transient));
        assert!(!t.should_retry(RetryReason::FeatureToggle));
        assert!(!t.should_retry(RetryReason::AntiBot));
    }

    #[test]
    fn tracker_record_increments_correct_counter() {
        let mut t = RetryTracker::new_default();
        t.record(RetryReason::Transient);
        assert_eq!(t.total(), 1);
        assert_eq!(t.feature_toggle(), 0);
        assert_eq!(t.anti_bot(), 0);

        t.record(RetryReason::FeatureToggle);
        assert_eq!(t.total(), 2);
        assert_eq!(t.feature_toggle(), 1);
        assert_eq!(t.anti_bot(), 0);

        t.record(RetryReason::AntiBot);
        assert_eq!(t.total(), 3);
        assert_eq!(t.feature_toggle(), 1);
        assert_eq!(t.anti_bot(), 1);
    }

    #[test]
    fn tracker_custom_limits() {
        let mut t = RetryTracker::new(10, 1, 1);
        t.record(RetryReason::AntiBot);
        assert!(
            !t.should_retry(RetryReason::AntiBot),
            "custom max_anti_bot=1"
        );
        assert!(t.should_retry(RetryReason::Transient), "total=1 < 10");
        assert!(t.should_retry(RetryReason::FeatureToggle), "ft=0 < 1");
    }

    /// 混合场景：AntiBot 触顶后 Transient + FeatureToggle 仍可用
    #[test]
    fn tracker_mixed_reasons_partial_block() {
        let mut t = RetryTracker::new_default();
        t.record(RetryReason::AntiBot);
        t.record(RetryReason::AntiBot); // anti_bot cap reached
        t.record(RetryReason::FeatureToggle);
        t.record(RetryReason::Transient);
        assert!(!t.should_retry(RetryReason::AntiBot));
        assert!(t.should_retry(RetryReason::FeatureToggle)); // ft=1 < 3
        assert!(t.should_retry(RetryReason::Transient)); // total=4 < 5
                                                         // 再记一次 transient 触顶
        t.record(RetryReason::Transient);
        assert_eq!(t.total(), 5);
        assert!(!t.should_retry(RetryReason::Transient));
    }

    /// 计数饱和保护：反复 record 不应 panic（saturating_add 编译期保证）
    #[test]
    fn tracker_saturating_record_no_overflow() {
        // 反复记录远超 max_total，应正常计数（不 panic、不回绕），
        // should_retry 受 max_total 限制始终 false
        let mut t = RetryTracker::new(3, 1, 1);
        for _ in 0..1000 {
            t.record(RetryReason::Transient);
        }
        assert_eq!(t.total(), 1000);
        assert!(!t.should_retry(RetryReason::Transient));
        assert!(!t.should_retry(RetryReason::FeatureToggle));
        assert!(!t.should_retry(RetryReason::AntiBot));
    }
}
