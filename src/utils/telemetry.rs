// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Telemetry initialization - logging setup via inklog
//!
//! inklog 同时安装 tracing subscriber 和 log::Log adapter，
//! 项目代码使用 log facade（log::info! 等）记录日志。

use crate::config::{FileLoggingSettings, LoggingSettings};
use inklog::{
    ConsoleSinkConfig, FileSinkConfig, GlobalConfig, InklogConfig, LoggerManager,
};
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
