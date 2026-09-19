// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 智能重试基础设施
//!
//! 三大组件：
//! - [`RetryReason`]：错误归类（Transient / FeatureToggle / AntiBot）
//! - [`RetryTracker`]：各 reason 独立计数与上限
//! - [`RetryDirective`]：身份升级指令（UA/代理/viewport/stealth/browser）

pub mod directive;
pub mod tracker;

pub use directive::RetryDirective;
pub use tracker::{RetryReason, RetryTracker};
