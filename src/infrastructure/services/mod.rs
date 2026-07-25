// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 基础设施服务模块
///
/// 提供基础设施层的服务实现
/// 包括限流服务等核心功能
pub mod config_service;
/// limiteron 限流服务实现
///
/// R-rl-001 / T017：rate-limit feature 关闭时不编译此模块。
/// rate-limit-off 模式下，`init_rate_limiting_service` 装配
/// `NoopRateLimitingService` 替代（见 T020），不需要 limiteron 依赖。
#[cfg(feature = "rate-limit")]
pub mod limiteron_service;
/// Noop 限流服务实现（rate-limit feature 关闭时使用）
///
/// R-rl-002 / T019：rate-limit feature 关闭时编译此模块，
/// 提供 `NoopRateLimitingService` 替代 `LimiteronService`，
/// 所有方法返回放行/成功。
#[cfg(not(feature = "rate-limit"))]
pub mod noop_rate_limiting_service;
pub mod webhook_sender_impl;
