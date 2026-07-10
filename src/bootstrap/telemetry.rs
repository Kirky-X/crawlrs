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
pub async fn init_all(settings: &LoggingSettings) -> Result<LoggerManager, inklog::InklogError> {
    let manager = init_telemetry(settings).await?;
    #[cfg(feature = "metrics")]
    init_metrics();
    Ok(manager)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConsoleLoggingSettings, FileLoggingSettings, LoggingSettings};

    fn settings_console_only() -> LoggingSettings {
        LoggingSettings {
            console: ConsoleLoggingSettings { enabled: true },
            file: FileLoggingSettings {
                enabled: false,
                path: "logs/test_telemetry.log".to_string(),
                max_file_size_mb: 50,
                file_count: 5,
            },
        }
    }

    fn settings_all_disabled() -> LoggingSettings {
        LoggingSettings {
            console: ConsoleLoggingSettings { enabled: false },
            file: FileLoggingSettings {
                enabled: false,
                path: "logs/test_telemetry_disabled.log".to_string(),
                max_file_size_mb: 10,
                file_count: 1,
            },
        }
    }

    // ========== init_telemetry tests ==========

    #[tokio::test]
    async fn test_init_telemetry_succeeds_with_console_only() {
        let settings = settings_console_only();
        let result = init_telemetry(&settings).await;
        assert!(
            result.is_ok(),
            "init_telemetry should succeed with console-only settings"
        );
    }

    #[tokio::test]
    async fn test_init_telemetry_succeeds_with_all_disabled() {
        let settings = settings_all_disabled();
        let result = init_telemetry(&settings).await;
        assert!(
            result.is_ok(),
            "init_telemetry should succeed with all sinks disabled"
        );
    }

    #[tokio::test]
    async fn test_init_telemetry_returns_logger_manager() {
        let settings = settings_console_only();
        let manager = init_telemetry(&settings)
            .await
            .expect("init_telemetry should succeed");
        // LoggerManager should be successfully created and hold the log worker
        // Dropping it should be safe (shuts down workers)
        drop(manager);
    }

    // ========== init_metrics tests ==========

    #[cfg(feature = "metrics")]
    #[tokio::test]
    async fn test_init_metrics_does_not_panic() {
        // init_metrics sets up the Prometheus metrics exporter.
        // It internally uses tokio::spawn, so it must run within a Tokio runtime.
        init_metrics();
    }

    // ========== init_all tests ==========

    #[tokio::test]
    async fn test_init_all_succeeds_with_console_only() {
        let settings = settings_console_only();
        let result = init_all(&settings).await;
        assert!(
            result.is_ok(),
            "init_all should succeed with console-only settings"
        );
    }

    #[tokio::test]
    async fn test_init_all_succeeds_with_all_disabled() {
        let settings = settings_all_disabled();
        let result = init_all(&settings).await;
        assert!(
            result.is_ok(),
            "init_all should succeed with all sinks disabled"
        );
    }

    #[tokio::test]
    async fn test_init_all_returns_logger_manager() {
        let settings = settings_console_only();
        let manager = init_all(&settings)
            .await
            .expect("init_all should succeed");
        drop(manager);
    }
}
