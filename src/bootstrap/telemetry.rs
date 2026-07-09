// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Telemetry and metrics initialization.

use crate::config::LoggingSettings;
use inklog::LoggerManager;
use log::info;

/// Initialize telemetry (inklog logging infrastructure).
///
/// This function sets up the inklog LoggerManager which installs both
/// a tracing subscriber and a log::Log adapter globally.
///
/// The returned LoggerManager must be held for the application lifetime
/// to keep log worker threads alive.
///
/// # Parameters
///
/// * `settings` - 日志配置
pub async fn init_telemetry(
    settings: &LoggingSettings,
) -> Result<LoggerManager, inklog::InklogError> {
    let manager = crate::utils::telemetry::init_telemetry(settings).await?;
    info!("Telemetry initialized");
    Ok(manager)
}

/// Initialize Prometheus metrics collection.
///
/// This function sets up the metrics exporter for collecting and exposing
/// application metrics via the `/metrics` endpoint.
#[cfg(feature = "metrics")]
pub fn init_metrics() {
    crate::infrastructure::metrics::init_metrics();
    info!("Metrics initialized");
}

/// Initialize both telemetry and metrics.
///
/// This is a convenience function that calls both [`init_telemetry`] and
/// [`init_metrics`] in sequence.
///
/// # Parameters
///
/// * `settings` - 日志配置
pub async fn init_all(
    settings: &LoggingSettings,
) -> Result<LoggerManager, inklog::InklogError> {
    let manager = init_telemetry(settings).await?;
    #[cfg(feature = "metrics")]
    init_metrics();
    Ok(manager)
}
