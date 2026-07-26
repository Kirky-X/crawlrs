// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 智能重试基础设施（design.md §4 — crawler-capability-absorption）
//!
//! 当前（Stage 1）仅提供 [`RetryReason`] 枚举，作为
//! `EngineError::retry_reason()` 的返回类型，驱动 `EngineRouter` /
//! `scrape_worker` 的重试策略选择。
//!
//! Stage 2 将补充 [`tracker::RetryTracker`]（T025，各 reason 独立计数与上限）
//! 与 `directive::RetryDirective`（T026，身份升级指令：UA/代理/viewport/stealth/browser）。

pub mod tracker;

pub use tracker::RetryReason;
