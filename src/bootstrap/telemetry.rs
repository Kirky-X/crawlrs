// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Telemetry and metrics initialization.

use tracing::info;

/// Initialize telemetry and tracing systems.
///
/// This function sets up the global tracing subscriber for structured logging
/// across the application.
pub fn init_telemetry() {
    crate::utils::telemetry::init_telemetry();
    info!("Telemetry initialized");
}

/// Initialize Prometheus metrics collection.
///
/// This function sets up the metrics exporter for collecting and exposing
/// application metrics via the `/metrics` endpoint.
pub fn init_metrics() {
    crate::infrastructure::metrics::init_metrics();
    info!("Metrics initialized");
}

/// Initialize both telemetry and metrics.
///
/// This is a convenience function that calls both [`init_telemetry`] and
/// [`init_metrics`] in sequence.
pub fn init_all() {
    init_telemetry();
    init_metrics();
}
