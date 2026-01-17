// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::config::{FileLoggingSettings, LoggingSettings};
use std::path::Path;
use tracing_appender::{non_blocking, rolling};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// 初始化遥测系统
///
/// 根据配置初始化日志系统，支持控制台和文件输出
///
/// # 参数
///
/// * `settings` - 日志配置
pub fn init_telemetry(settings: &LoggingSettings) {
    // 创建环境过滤器
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,crawlrs=debug".into());

    // 根据配置构建 subscriber
    match (settings.console.enabled, settings.file.enabled) {
        (true, false) => {
            // 仅控制台输出
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
                .init();
        }
        (false, true) => {
            // 仅文件输出
            let _guard = init_file_logging(&settings.file);
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(create_file_writer(&settings.file)),
                )
                .init();
        }
        (true, true) => {
            // 同时输出到控制台和文件
            let _guard = init_file_logging(&settings.file);
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(create_file_writer(&settings.file)),
                )
                .init();
        }
        (false, false) => {
            // 都不启用，仅初始化基础注册器
            tracing_subscriber::registry().with(env_filter).init();
        }
    }
}

/// 初始化文件日志
///
/// # 参数
///
/// * `file_settings` - 文件日志配置
///
/// # 返回值
///
/// * guard - 守护者（用于刷新缓冲区）
fn init_file_logging(file_settings: &FileLoggingSettings) -> non_blocking::WorkerGuard {
    // 创建日志目录（如果不存在）
    if let Some(parent) = Path::new(&file_settings.path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("Failed to create log directory: {}", e);
            });
        }
    }

    // 创建滚动文件 appender
    let file_appender = rolling::daily(
        Path::new(&file_settings.path)
            .parent()
            .unwrap_or_else(|| Path::new(".")),
        Path::new(&file_settings.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("crawlrs.log"),
    );

    // 使用非阻塞写入器
    let (non_blocking, guard) = non_blocking(file_appender);
    guard
}

/// 创建文件写入器
///
/// # 参数
///
/// * `file_settings` - 文件日志配置
///
/// # 返回值
///
/// * non_blocking - 非阻塞写入器
fn create_file_writer(file_settings: &FileLoggingSettings) -> non_blocking::NonBlocking {
    // 创建日志目录（如果不存在）
    if let Some(parent) = Path::new(&file_settings.path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("Failed to create log directory: {}", e);
            });
        }
    }

    // 创建滚动文件 appender
    let file_appender = rolling::daily(
        Path::new(&file_settings.path)
            .parent()
            .unwrap_or_else(|| Path::new(".")),
        Path::new(&file_settings.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("crawlrs.log"),
    );

    // 使用非阻塞写入器
    let (non_blocking, _guard) = non_blocking(file_appender);
    non_blocking
}

/// 兼容旧版本的初始化函数（仅控制台输出）
///
/// # Deprecated
///
/// 请使用 `init_telemetry(settings)` 代替
#[deprecated(note = "Please use init_telemetry(settings) instead")]
pub fn init_telemetry_legacy() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,crawlrs=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}
