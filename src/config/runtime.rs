// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 运行时配置
//!
//! 提供应用运行时配置，包括worker数量、时间间隔等可调参数

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 运行时配置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__RUNTIME__")]
pub struct RuntimeConfig {
    /// Worker相关配置
    pub worker: WorkerConfig,

    /// 后台任务配置
    pub background_tasks: BackgroundTaskConfig,
}

/// Worker配置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__RUNTIME__WORKER__")]
pub struct WorkerConfig {
    /// 初始worker数量
    #[config(default = 5)]
    pub initial_count: usize,

    /// 最大worker数量
    #[config(default = 20)]
    pub max_count: usize,

    /// 每个worker的队列大小
    #[config(default = 100)]
    pub queue_size: usize,
}

/// 后台任务配置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__RUNTIME__BACKGROUND_TASKS__")]
pub struct BackgroundTaskConfig {
    /// Webhook处理间隔（秒）
    #[config(default = 5)]
    pub webhook_interval_secs: u64,

    /// 积压任务处理间隔（秒）
    #[config(default = 30)]
    pub backlog_interval_secs: u64,

    /// 缓存清理间隔（秒）
    #[config(default = 60)]
    pub cache_cleanup_interval_secs: u64,

    /// DNS缓存TTL（秒）
    #[config(default = 300)]
    pub dns_cache_ttl_secs: u64,
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
        assert_eq!(config.cache_cleanup_interval(), Duration::from_secs(60));
        assert_eq!(config.dns_cache_ttl(), Duration::from_secs(300));
    }

    #[test]
    fn test_worker_config_defaults() {
        let config = WorkerConfig::default();
        assert_eq!(config.initial_count, 5);
        assert_eq!(config.max_count, 20);
        assert_eq!(config.queue_size, 100);
    }

    #[test]
    fn test_runtime_config_worker_fields() {
        let config = RuntimeConfig::default();
        assert_eq!(config.worker.max_count, 20);
        assert_eq!(config.worker.queue_size, 100);
        assert_eq!(config.background_tasks.cache_cleanup_interval_secs, 60);
        assert_eq!(config.background_tasks.dns_cache_ttl_secs, 300);
    }
}
