// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

/// 可观测性模块
///
/// 提供系统监控和可观测性功能
/// 包括指标收集和遥测数据管理
#[cfg(feature = "metrics")]
pub mod metrics;
// pub mod telemetry;

/// 失败现场快照（F4）：错误签名键去重存档（HTML + 响应元数据 + 可选截图）。
///
/// 不挂 metrics 门控：std::fs + sha2 + dashmap 均为核心依赖；
/// Prometheus 计数经 `metrics_shim`（关闭时编译为 noop）。
pub mod failure_snapshot;
