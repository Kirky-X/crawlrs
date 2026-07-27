// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 智能重试基础设施（design.md §4 — crawler-capability-absorption）
//!
//! 三大组件：
//! - [`RetryReason`]：错误归类（Transient / FeatureToggle / AntiBot）
//! - [`RetryTracker`]：各 reason 独立计数与上限（T025）
//! - [`RetryDirective`]：身份升级指令（T026，UA/代理/viewport/stealth/browser）

pub mod directive;
pub mod tracker;

pub use directive::RetryDirective;
pub use tracker::{RetryReason, RetryTracker};
