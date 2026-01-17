// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use serde::Deserialize;

/// 日志配置
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingSettings {
    /// 控制台输出配置
    pub console: ConsoleLoggingSettings,
    /// 文件输出配置
    pub file: FileLoggingSettings,
}

/// 控制台日志配置
#[derive(Debug, Clone, Deserialize)]
pub struct ConsoleLoggingSettings {
    /// 是否启用控制台输出
    #[serde(default = "default_console_enabled")]
    pub enabled: bool,
}

/// 文件日志配置
#[derive(Debug, Clone, Deserialize)]
pub struct FileLoggingSettings {
    /// 是否启用文件输出
    #[serde(default = "default_file_enabled")]
    pub enabled: bool,
    /// 日志文件路径
    #[serde(default = "default_log_path")]
    pub path: String,
    /// 单个日志文件最大大小（MB）
    #[serde(default = "default_max_file_size")]
    pub max_file_size_mb: u64,
    /// 保留的日志文件数量
    #[serde(default = "default_file_count")]
    pub file_count: usize,
}

// 默认值函数
fn default_console_enabled() -> bool {
    true
}

fn default_file_enabled() -> bool {
    false
}

fn default_log_path() -> String {
    "logs/crawlrs.log".to_string()
}

fn default_max_file_size() -> u64 {
    crate::common::constants::logging::MAX_LOG_FILE_SIZE_MB
}

fn default_file_count() -> usize {
    crate::common::constants::logging::LOG_FILE_COUNT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_console_enabled() {
        assert_eq!(default_console_enabled(), true);
    }

    #[test]
    fn test_default_file_enabled() {
        assert_eq!(default_file_enabled(), false);
    }

    #[test]
    fn test_default_log_path() {
        assert_eq!(default_log_path(), "logs/crawlrs.log");
    }

    #[test]
    fn test_default_max_file_size() {
        assert_eq!(default_max_file_size(), 100);
    }

    #[test]
    fn test_default_file_count() {
        assert_eq!(default_file_count(), 10);
    }
}
