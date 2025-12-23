// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 指标收集模块
///
/// 提供系统指标的收集和导出功能
/// 使用Prometheus格式暴露应用性能指标
use metrics_exporter_prometheus::PrometheusBuilder;
use std::net::SocketAddr;
use tracing::info;

pub fn init_metrics() {
    let builder = PrometheusBuilder::new();
    let addr: SocketAddr = "0.0.0.0:9000".parse().expect("Invalid metrics address");

    // Start the exporter
    // Ignore error if address is already in use (for development/testing)
    if let Err(e) = builder.with_http_listener(addr).install() {
        tracing::warn!("Failed to install Prometheus recorder: {}. This might happen if the port is already in use.", e);
    }

    info!("Metrics exporter listening on {}", addr);
}
