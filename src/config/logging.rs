// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 日志配置

use confers::Config;
use serde::{Deserialize, Serialize};

/// 日志配置
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__LOGGING__")]
pub struct LoggingSettings {
    /// 控制台输出配置
    pub console: ConsoleLoggingSettings,

    /// 文件输出配置
    pub file: FileLoggingSettings,
}

/// 控制台日志配置
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__LOGGING__CONSOLE__")]
pub struct ConsoleLoggingSettings {
    /// 是否启用控制台输出
    #[config(default = true)]
    pub enabled: bool,
}

/// 文件日志配置
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__LOGGING__FILE__")]
pub struct FileLoggingSettings {
    /// 是否启用文件输出
    #[config(default = false)]
    pub enabled: bool,

    /// 日志文件路径
    #[config(default = "logs/crawlrs.log".to_string())]
    pub path: String,

    /// 单个日志文件最大大小（MB）
    #[config(default = 100)]
    pub max_file_size_mb: u64,

    /// 保留的日志文件数量
    #[config(default = 10)]
    pub file_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_console_enabled() {
        let settings = ConsoleLoggingSettings::default();
        assert!(settings.enabled);
    }

    #[test]
    fn test_default_file_enabled() {
        let settings = FileLoggingSettings::default();
        assert!(!settings.enabled);
    }

    #[test]
    fn test_default_log_path() {
        let settings = FileLoggingSettings::default();
        assert_eq!(settings.path, "logs/crawlrs.log");
    }

    #[test]
    fn test_default_max_file_size() {
        let settings = FileLoggingSettings::default();
        assert_eq!(settings.max_file_size_mb, 100);
    }

    #[test]
    fn test_default_file_count() {
        let settings = FileLoggingSettings::default();
        assert_eq!(settings.file_count, 10);
    }
}
