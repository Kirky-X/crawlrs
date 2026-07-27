// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 重试指令（design.md §4 — crawler-capability-absorption Stage 2，T026）
//!
//! `RetryDirective` 描述"本次重试"应执行的身份升级动作集：
//! - `rotate_ua`：轮换 User-Agent（`UaPool::pick_seeded(attempt)`）
//! - `rotate_proxy`：轮换代理
//! - `change_viewport`：改变视口尺寸
//! - `enable_stealth`：开启 stealth 模式（隐藏 webdriver 等指纹）
//! - `force_browser`：强制使用浏览器引擎（Playwright/FlareSolverr）
//!
//! `for_attempt(reason, attempt)` 按 attempt 递增依次升级身份：
//! - attempt=0：所有字段 false（首次尝试，无身份切换）
//! - attempt=1：rotate_ua=true（仅换 UA）
//! - attempt=2：rotate_ua + rotate_proxy + change_viewport
//! - attempt≥3：全部 true（含 stealth + force_browser）
//!
//! 特殊处理：`FeatureToggle` reason 时 attempt=0 即 rotate_ua=true
//! （特性切换需立即换身份避免重复触发）。

use super::tracker::RetryReason;

/// 重试指令 — 本次重试应执行的身份升级动作集
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RetryDirective {
    /// 轮换 User-Agent
    pub rotate_ua: bool,
    /// 轮换代理
    pub rotate_proxy: bool,
    /// 改变视口尺寸
    pub change_viewport: bool,
    /// 开启 stealth 模式
    pub enable_stealth: bool,
    /// 强制浏览器引擎
    pub force_browser: bool,
}

impl RetryDirective {
    /// 按 reason + attempt 计算升级指令。
    ///
    /// - `reason`：本次失败的 [`RetryReason`]
    /// - `attempt`：当前 attempt 编号（0 = 首次尝试前的退避）
    ///
    /// 返回值：下次 attempt 应执行的身份升级动作集。
    #[must_use]
    pub fn for_attempt(reason: RetryReason, attempt: u32) -> Self {
        // FeatureToggle 特例：attempt=0 即 rotate_ua=true
        // （特性切换需立即换身份避免重复触发）
        if reason == RetryReason::FeatureToggle && attempt == 0 {
            return Self {
                rotate_ua: true,
                ..Self::default()
            };
        }

        // 通用升级策略（按 attempt 递增依次开启）
        match attempt {
            0 => Self::default(), // 首次尝试无身份切换
            1 => Self {
                rotate_ua: true,
                ..Self::default()
            },
            2 => Self {
                rotate_ua: true,
                rotate_proxy: true,
                change_viewport: true,
                ..Self::default()
            },
            _ => Self {
                // attempt >= 3：全部开启
                rotate_ua: true,
                rotate_proxy: true,
                change_viewport: true,
                enable_stealth: true,
                force_browser: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R-identity-002: attempt=0（首次尝试）所有字段 false
    #[test]
    fn attempt_0_all_false() {
        let d = RetryDirective::for_attempt(RetryReason::Transient, 0);
        assert!(!d.rotate_ua);
        assert!(!d.rotate_proxy);
        assert!(!d.change_viewport);
        assert!(!d.enable_stealth);
        assert!(!d.force_browser);
    }

    /// R-identity-002: attempt=1 仅 rotate_ua=true
    #[test]
    fn attempt_1_only_ua() {
        let d = RetryDirective::for_attempt(RetryReason::Transient, 1);
        assert!(d.rotate_ua);
        assert!(!d.rotate_proxy);
        assert!(!d.change_viewport);
        assert!(!d.enable_stealth);
        assert!(!d.force_browser);
    }

    /// R-identity-002: attempt=2 开启 ua + proxy + viewport
    #[test]
    fn attempt_2_ua_proxy_viewport() {
        let d = RetryDirective::for_attempt(RetryReason::Transient, 2);
        assert!(d.rotate_ua);
        assert!(d.rotate_proxy);
        assert!(d.change_viewport);
        assert!(!d.enable_stealth);
        assert!(!d.force_browser);
    }

    /// R-identity-002: attempt≥3 全部开启
    #[test]
    fn attempt_3_all_enabled() {
        let d = RetryDirective::for_attempt(RetryReason::Transient, 3);
        assert!(d.rotate_ua);
        assert!(d.rotate_proxy);
        assert!(d.change_viewport);
        assert!(d.enable_stealth);
        assert!(d.force_browser);
    }

    /// attempt=4+ 仍全部开启（边界）
    #[test]
    fn attempt_high_all_enabled() {
        let d = RetryDirective::for_attempt(RetryReason::AntiBot, 10);
        assert!(d.rotate_ua);
        assert!(d.rotate_proxy);
        assert!(d.change_viewport);
        assert!(d.enable_stealth);
        assert!(d.force_browser);
    }

    /// R-identity-002: FeatureToggle + attempt=0 → rotate_ua=true（特例）
    #[test]
    fn feature_toggle_attempt_0_rotates_ua() {
        let d = RetryDirective::for_attempt(RetryReason::FeatureToggle, 0);
        assert!(d.rotate_ua, "FeatureToggle attempt=0 must rotate UA");
        assert!(!d.rotate_proxy);
        assert!(!d.change_viewport);
        assert!(!d.enable_stealth);
        assert!(!d.force_browser);
    }

    /// FeatureToggle + attempt=1 仍按通用策略（仅 rotate_ua）
    #[test]
    fn feature_toggle_attempt_1_follows_general() {
        let d = RetryDirective::for_attempt(RetryReason::FeatureToggle, 1);
        assert!(d.rotate_ua);
        assert!(!d.rotate_proxy);
    }

    /// AntiBot reason 也按通用升级策略（attempt 递增）
    #[test]
    fn antibot_follows_general_strategy() {
        let d0 = RetryDirective::for_attempt(RetryReason::AntiBot, 0);
        assert!(!d0.rotate_ua);

        let d1 = RetryDirective::for_attempt(RetryReason::AntiBot, 1);
        assert!(d1.rotate_ua);

        let d3 = RetryDirective::for_attempt(RetryReason::AntiBot, 3);
        assert!(d3.force_browser, "AntiBot attempt=3 must force browser");
        assert!(d3.enable_stealth, "AntiBot attempt=3 must enable stealth");
    }

    /// 三种 reason 在 attempt=2 应有相同指令（仅 FeatureToggle attempt=0 特例）
    #[test]
    fn all_reasons_same_directive_at_attempt_2() {
        let d_trans = RetryDirective::for_attempt(RetryReason::Transient, 2);
        let d_ft = RetryDirective::for_attempt(RetryReason::FeatureToggle, 2);
        let d_ab = RetryDirective::for_attempt(RetryReason::AntiBot, 2);
        assert_eq!(d_trans, d_ft);
        assert_eq!(d_trans, d_ab);
    }

    /// Default impl 所有字段 false（与 attempt=0 一致）
    #[test]
    fn default_all_false() {
        let d = RetryDirective::default();
        assert!(!d.rotate_ua);
        assert!(!d.rotate_proxy);
        assert!(!d.change_viewport);
        assert!(!d.enable_stealth);
        assert!(!d.force_browser);
    }
}
