// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 指标收集模块
///
/// 提供系统指标的收集和导出功能
/// 使用Prometheus格式暴露应用性能指标
///
/// 该模块已统一到 observability/metrics,此处为向后兼容提供的重导出
pub use crate::infrastructure::observability::metrics::init_metrics;
