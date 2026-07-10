// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Telemetry initialization - logging setup via inklog
//!
//! inklog 同时安装 tracing subscriber 和 log::Log adapter，
//! 项目代码使用 log facade（log::info! 等）记录日志。

use crate::config::{FileLoggingSettings, LoggingSettings};
use inklog::{ConsoleSinkConfig, FileSinkConfig, GlobalConfig, InklogConfig, LoggerManager};
use std::path::PathBuf;

/// 初始化遥测系统（inklog 日志基础设施）
///
/// 根据配置构建 InklogConfig 并通过 LoggerManager::with_config 启动。
/// 返回的 LoggerManager 必须在应用生命周期内保持存活，否则日志 worker 线程会被释放。
///
/// # 参数
///
/// * `settings` - 日志配置
///
/// # 返回值
///
/// * `Ok(LoggerManager)` - 日志管理器，调用方必须持有
/// * `Err(InklogError)` - 初始化失败
pub async fn init_telemetry(
    settings: &LoggingSettings,
) -> Result<LoggerManager, inklog::InklogError> {
    let config = build_inklog_config(settings);
    LoggerManager::with_config(config).await
}

/// 从 LoggingSettings 构建 InklogConfig
///
/// 映射 crawlrs 的日志配置到 inklog 的配置结构。
fn build_inklog_config(settings: &LoggingSettings) -> InklogConfig {
    let console_sink = if settings.console.enabled {
        Some(ConsoleSinkConfig::default())
    } else {
        None
    };

    let file_sink = if settings.file.enabled {
        Some(build_file_sink_config(&settings.file))
    } else {
        None
    };

    InklogConfig {
        global: GlobalConfig::default(),
        console_sink,
        file_sink,
        database_sink: None,
        performance: Default::default(),
        http_server: None,
    }
}

/// 从 FileLoggingSettings 构建 FileSinkConfig
fn build_file_sink_config(file_settings: &FileLoggingSettings) -> FileSinkConfig {
    FileSinkConfig {
        enabled: true,
        path: PathBuf::from(&file_settings.path),
        max_size: format!("{}MB", file_settings.max_file_size_mb),
        keep_files: file_settings.file_count as u32,
        ..Default::default()
    }
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
                path: "logs/test.log".to_string(),
                max_file_size_mb: 50,
                file_count: 5,
            },
        }
    }

    fn settings_file_only() -> LoggingSettings {
        LoggingSettings {
            console: ConsoleLoggingSettings { enabled: false },
            file: FileLoggingSettings {
                enabled: true,
                path: "logs/app.log".to_string(),
                max_file_size_mb: 20,
                file_count: 3,
            },
        }
    }

    fn settings_both_disabled() -> LoggingSettings {
        LoggingSettings {
            console: ConsoleLoggingSettings { enabled: false },
            file: FileLoggingSettings {
                enabled: false,
                path: "logs/disabled.log".to_string(),
                max_file_size_mb: 10,
                file_count: 1,
            },
        }
    }

    fn settings_both_enabled() -> LoggingSettings {
        LoggingSettings {
            console: ConsoleLoggingSettings { enabled: true },
            file: FileLoggingSettings {
                enabled: true,
                path: "logs/both.log".to_string(),
                max_file_size_mb: 30,
                file_count: 7,
            },
        }
    }

    // ========== build_inklog_config tests ==========

    #[test]
    fn test_build_inklog_config_console_only() {
        let config = build_inklog_config(&settings_console_only());
        assert!(
            config.console_sink.is_some(),
            "console sink should be present"
        );
        assert!(config.file_sink.is_none(), "file sink should be absent");
    }

    #[test]
    fn test_build_inklog_config_file_only() {
        let config = build_inklog_config(&settings_file_only());
        assert!(
            config.console_sink.is_none(),
            "console sink should be absent"
        );
        assert!(config.file_sink.is_some(), "file sink should be present");
    }

    #[test]
    fn test_build_inklog_config_both_disabled() {
        let config = build_inklog_config(&settings_both_disabled());
        assert!(
            config.console_sink.is_none(),
            "console sink should be absent"
        );
        assert!(config.file_sink.is_none(), "file sink should be absent");
    }

    #[test]
    fn test_build_inklog_config_both_enabled() {
        let config = build_inklog_config(&settings_both_enabled());
        assert!(
            config.console_sink.is_some(),
            "console sink should be present"
        );
        assert!(config.file_sink.is_some(), "file sink should be present");
    }

    #[test]
    fn test_build_inklog_config_database_sink_always_none() {
        let config = build_inklog_config(&settings_both_enabled());
        assert!(
            config.database_sink.is_none(),
            "database sink should always be None"
        );
    }

    #[test]
    fn test_build_inklog_config_http_server_always_none() {
        let config = build_inklog_config(&settings_both_enabled());
        assert!(
            config.http_server.is_none(),
            "http server should always be None"
        );
    }

    // ========== build_file_sink_config tests ==========

    #[test]
    fn test_build_file_sink_config_path() {
        let file_settings = FileLoggingSettings {
            enabled: true,
            path: "logs/custom.log".to_string(),
            max_file_size_mb: 25,
            file_count: 4,
        };
        let config = build_file_sink_config(&file_settings);
        assert_eq!(config.path, PathBuf::from("logs/custom.log"));
    }

    #[test]
    fn test_build_file_sink_config_max_size_format() {
        let file_settings = FileLoggingSettings {
            enabled: true,
            path: "x.log".to_string(),
            max_file_size_mb: 100,
            file_count: 2,
        };
        let config = build_file_sink_config(&file_settings);
        assert_eq!(config.max_size, "100MB");
    }

    #[test]
    fn test_build_file_sink_config_keep_files() {
        let file_settings = FileLoggingSettings {
            enabled: true,
            path: "x.log".to_string(),
            max_file_size_mb: 10,
            file_count: 8,
        };
        let config = build_file_sink_config(&file_settings);
        assert_eq!(config.keep_files, 8);
    }

    #[test]
    fn test_build_file_sink_config_enabled_is_true() {
        let file_settings = FileLoggingSettings {
            enabled: false,
            path: "x.log".to_string(),
            max_file_size_mb: 10,
            file_count: 1,
        };
        let config = build_file_sink_config(&file_settings);
        assert!(
            config.enabled,
            "build_file_sink_config always sets enabled=true"
        );
    }

    // ========== init_telemetry tests ==========

    #[tokio::test]
    async fn test_init_telemetry_succeeds_with_console_only() {
        let settings = settings_console_only();
        let result = init_telemetry(&settings).await;
        assert!(
            result.is_ok(),
            "init_telemetry should succeed with console-only"
        );
    }

    #[tokio::test]
    async fn test_init_telemetry_succeeds_with_both_disabled() {
        let settings = settings_both_disabled();
        let result = init_telemetry(&settings).await;
        assert!(
            result.is_ok(),
            "init_telemetry should succeed with all sinks disabled"
        );
    }

    #[tokio::test]
    async fn test_init_telemetry_succeeds_with_both_enabled() {
        let settings = settings_both_enabled();
        let result = init_telemetry(&settings).await;
        assert!(
            result.is_ok(),
            "init_telemetry should succeed with both sinks enabled"
        );
    }
}
