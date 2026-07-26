// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 重试原因分类（design.md §4 — crawler-capability-absorption Stage 1/2）
//!
//! `RetryReason` 将 `EngineError` 归类为三类重试策略语义，供 `EngineRouter` /
//! `scrape_worker` 决策如何重试（同引擎 vs 换引擎 vs 切身份）。
//!
//! 完整的 `RetryTracker`（各 reason 独立计数与上限）在 T025（Stage 2）补齐；
//! 本文件当前仅提供枚举，作为 `EngineError::retry_reason()` 的返回类型。

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
}
