// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 应用程序常量定义
//!
//! 将魔法数字定义为有意义的常量，提高代码可读性和可维护性

/// 应用程序相关常量
pub mod app {
    /// 应用程序名称
    pub const NAME: &str = "crawlrs";
    /// 应用程序版本
    pub const VERSION: &str = env!("CARGO_PKG_VERSION");
}

/// 网络相关常量
pub mod network {
    /// 默认超时时间（秒）
    pub const DEFAULT_TIMEOUT_SECS: u64 = 30;
    /// 最大重试次数
    pub const MAX_RETRIES: u32 = 3;
    /// 连接池大小
    pub const CONNECTION_POOL_SIZE: usize = 10;
    /// 重试延迟（毫秒）
    pub const RETRY_DELAY_MS: u64 = 1000;
    /// 最大并发连接数
    pub const MAX_CONCURRENT_CONNECTIONS: usize = 100;
}

/// 缓存相关常量
pub mod cache {
    /// 默认 TTL（秒）
    pub const DEFAULT_TTL_SECS: u64 = 300;
    /// 最大缓存条目数
    pub const MAX_CACHE_ENTRIES: usize = 10000;
    /// 缓存预热批次大小
    pub const PREHEAT_BATCH_SIZE: usize = 100;
    /// LRU 缓存大小
    pub const LRU_CACHE_SIZE: usize = 256;
}

/// 任务相关常量
pub mod task {
    /// 最大并发任务数
    pub const MAX_CONCURRENT_TASKS: usize = 100;
    /// 任务超时时间（秒）
    pub const TASK_TIMEOUT_SECS: u64 = 300;
    /// 任务锁持续时间（秒）
    pub const TASK_LOCK_DURATION_SECS: i64 = 300;
    /// 最大重试次数
    pub const MAX_RETRIES: u32 = 3;
    /// 默认优先级
    pub const DEFAULT_PRIORITY: i32 = 5;
}

/// 数据库相关常量
pub mod database {
    /// 最大连接数
    pub const MAX_CONNECTIONS: u32 = 100;
    /// 最小连接数
    pub const MIN_CONNECTIONS: u32 = 5;
    /// 连接超时时间（秒）
    pub const CONNECT_TIMEOUT_SECS: u64 = 10;
    /// 空闲超时时间（秒）
    pub const IDLE_TIMEOUT_SECS: u64 = 600;
    /// 连接最大生命周期（秒）
    pub const MAX_LIFETIME_SECS: u64 = 3600;
}

/// 速率限制相关常量
pub mod rate_limit {
    /// 默认每分钟请求数
    pub const DEFAULT_RPM: u32 = 60;
    /// 令牌桶容量
    pub const BUCKET_CAPACITY: u32 = 100;
    /// 每秒请求数
    pub const REQUESTS_PER_SECOND: u32 = 1;
    /// 每小时请求数
    pub const REQUESTS_PER_HOUR: u32 = 3600;
}

/// 并发控制相关常量
pub mod concurrency {
    /// 默认团队限制
    pub const DEFAULT_TEAM_LIMIT: usize = 10;
    /// 最大团队限制
    pub const MAX_TEAM_LIMIT: usize = 100;
}

/// 搜索引擎相关常量
pub mod search {
    /// 搜索超时时间（毫秒）
    pub const SEARCH_TIMEOUT_MS: u64 = 5000;
    /// 最大结果数
    pub const MAX_RESULTS: usize = 10;
    /// 引擎数量
    pub const ENGINE_COUNT: usize = 8;
    /// 平滑因子
    pub const SMOOTHING_FACTOR: f64 = 0.1;
}

/// 监控相关常量
pub mod metrics {
    /// 指标收集间隔（秒）
    pub const COLLECTION_INTERVAL_SECS: u64 = 60;
    /// 性能历史最大条目数
    pub const MAX_PERFORMANCE_HISTORY: usize = 1000;
    /// 性能历史清理数量
    pub const PERFORMANCE_HISTORY_CLEANUP_COUNT: usize = 100;
}

/// 日志相关常量
pub mod logging {
    /// 日志文件最大大小（MB）
    pub const MAX_LOG_FILE_SIZE_MB: u64 = 100;
    /// 日志文件保留数量
    pub const LOG_FILE_COUNT: usize = 10;
}

/// Webhook 相关常量
pub mod webhook {
    /// Webhook 超时时间（秒）
    pub const WEBHOOK_TIMEOUT_SECS: u64 = 30;
    /// 最大重试次数
    pub const MAX_RETRIES: u32 = 3;
    /// 重试延迟（秒）
    pub const RETRY_DELAY_SECS: u64 = 5;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_constants() {
        assert_eq!(app::NAME, "crawlrs");
        assert!(!app::VERSION.is_empty());
    }

    #[test]
    fn test_network_constants() {
        assert_eq!(network::DEFAULT_TIMEOUT_SECS, 30);
        assert_eq!(network::MAX_RETRIES, 3);
    }

    #[test]
    fn test_cache_constants() {
        assert_eq!(cache::DEFAULT_TTL_SECS, 300);
        assert_eq!(cache::MAX_CACHE_ENTRIES, 10000);
    }

    #[test]
    fn test_task_constants() {
        assert_eq!(task::MAX_CONCURRENT_TASKS, 100);
        assert_eq!(task::TASK_TIMEOUT_SECS, 300);
    }

    #[test]
    fn test_database_constants() {
        assert_eq!(database::MAX_CONNECTIONS, 100);
        assert_eq!(database::MIN_CONNECTIONS, 5);
    }

    #[test]
    fn test_rate_limit_constants() {
        assert_eq!(rate_limit::DEFAULT_RPM, 60);
        assert_eq!(rate_limit::BUCKET_CAPACITY, 100);
    }
}