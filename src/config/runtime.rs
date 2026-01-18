// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 运行时配置
//!
//! 提供应用运行时配置，包括worker数量、时间间隔等可调参数

use serde::Deserialize;
use std::time::Duration;

/// 运行时配置
#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    /// Worker相关配置
    pub worker: WorkerConfig,
    /// 后台任务配置
    pub background_tasks: BackgroundTaskConfig,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            worker: WorkerConfig::default(),
            background_tasks: BackgroundTaskConfig::default(),
        }
    }
}

/// Worker配置
#[derive(Debug, Clone, Deserialize)]
pub struct WorkerConfig {
    /// 初始worker数量
    pub initial_count: usize,
    /// 最大worker数量
    pub max_count: usize,
    /// 每个worker的队列大小
    pub queue_size: usize,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            initial_count: 5,
            max_count: 20,
            queue_size: 100,
        }
    }
}

/// 后台任务配置
#[derive(Debug, Clone, Deserialize)]
pub struct BackgroundTaskConfig {
    /// Webhook处理间隔（秒）
    pub webhook_interval_secs: u64,
    /// 积压任务处理间隔（秒）
    pub backlog_interval_secs: u64,
    /// 缓存清理间隔（秒）
    pub cache_cleanup_interval_secs: u64,
    /// DNS缓存TTL（秒）
    pub dns_cache_ttl_secs: u64,
}

impl Default for BackgroundTaskConfig {
    fn default() -> Self {
        Self {
            webhook_interval_secs: 5,
            backlog_interval_secs: 30,
            cache_cleanup_interval_secs: 60,
            dns_cache_ttl_secs: 300,
        }
    }
}

impl BackgroundTaskConfig {
    /// 获取Webhook处理间隔
    pub fn webhook_interval(&self) -> Duration {
        Duration::from_secs(self.webhook_interval_secs)
    }

    /// 获取积压任务处理间隔
    pub fn backlog_interval(&self) -> Duration {
        Duration::from_secs(self.backlog_interval_secs)
    }

    /// 获取缓存清理间隔
    pub fn cache_cleanup_interval(&self) -> Duration {
        Duration::from_secs(self.cache_cleanup_interval_secs)
    }

    /// 获取DNS缓存TTL
    pub fn dns_cache_ttl(&self) -> Duration {
        Duration::from_secs(self.dns_cache_ttl_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_config_defaults() {
        let config = RuntimeConfig::default();
        assert_eq!(config.worker.initial_count, 5);
        assert_eq!(config.background_tasks.webhook_interval_secs, 5);
    }

    #[test]
    fn test_background_task_intervals() {
        let config = BackgroundTaskConfig::default();
        assert_eq!(config.webhook_interval(), Duration::from_secs(5));
        assert_eq!(config.backlog_interval(), Duration::from_secs(30));
    }
}
