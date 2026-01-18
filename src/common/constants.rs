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

/// 测试相关常量
#[cfg(test)]
pub mod testing {
    use std::time::Duration;

    /// API 请求超时时间（10秒）
    pub const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
    /// 快速测试超时时间（10秒）
    pub const QUICK_TEST_TIMEOUT: Duration = Duration::from_secs(10);
    /// E2E 测试超时时间（90秒）
    pub const E2E_TEST_TIMEOUT: Duration = Duration::from_secs(90);
    /// 爬虫任务超时时间（90秒）
    pub const CRAWL_TASK_TIMEOUT: Duration = Duration::from_secs(90);
}

/// 环境变量名称常量
/// 使用常量定义环境变量名称，避免拼写错误，提高可维护性
pub mod env_vars {
    /// 应用程序环境
    pub const ENV: &str = "CRAWLRS_ENV";
    /// 应用程序环境（备用名称）
    pub const APP_ENVIRONMENT: &str = "APP_ENVIRONMENT";

    // === 速率限制相关 ===
    /// 禁用速率限制
    pub const RATE_LIMITING_ENABLED: &str = "CRAWLRS_RATE_LIMITING_ENABLED";

    // === SSRF 保护相关 ===
    /// 禁用 SSRF 保护
    pub const DISABLE_SSRF_PROTECTION: &str = "CRAWLRS_DISABLE_SSRF_PROTECTION";
    /// 启用网络测试
    pub const ENABLE_NETWORK_TESTS: &str = "CRAWLRS_ENABLE_NETWORK_TESTS";

    // === 代理相关 ===
    /// 代理 URL
    pub const PROXY_URL: &str = "CRAWLRS_PROXY_URL";

    // === 测试相关 ===
    /// 测试模式：不复用浏览器
    pub const TEST_NO_BROWSER_REUSE: &str = "CRAWLRS_TEST_NO_BROWSER_REUSE";

    // === 调试相关 ===
    /// 调试：保存 HTML
    pub const DEBUG_SAVE_HTML: &str = "DEBUG_SAVE_HTML";

    // === 健康检查相关 ===
    /// 健康检查 URL
    pub const HEALTH_CHECK_URL: &str = "CRAWLRS_HEALTH_CHECK_URL";

    // === 搜索引擎测试结果 ===
    /// 百度测试结果
    pub const BAIDU_TEST_RESULTS: &str = "BAIDU_TEST_RESULTS";
    /// 必应测试结果
    pub const BING_TEST_RESULTS: &str = "BING_TEST_RESULTS";
    /// 谷歌 HTTP 回退测试结果
    pub const GOOGLE_HTTP_FALLBACK_TEST_RESULTS: &str = "GOOGLE_HTTP_FALLBACK_TEST_RESULTS";
    /// 搜狗测试结果
    pub const SOGOU_TEST_RESULTS: &str = "SOGOU_TEST_RESULTS";
    /// 使用测试数据
    pub const USE_TEST_DATA: &str = "USE_TEST_DATA";

    // === 跳过测试 ===
    /// 跳过搜索测试
    pub const SKIP_SEARCH_TESTS: &str = "SKIP_SEARCH_TESTS";
    /// 跳过浏览器测试
    pub const SKIP_BROWSER_TESTS: &str = "SKIP_BROWSER_TESTS";

    // === 浏览器远程调试 ===
    /// Chromium 远程调试 URL
    pub const CHROMIUM_REMOTE_DEBUGGING_URL: &str = "CHROMIUM_REMOTE_DEBUGGING_URL";

    // === Fire 引擎相关 ===
    /// Fire 引擎 CDP URL
    pub const FIRE_ENGINE_CDP_URL: &str = "FIRE_ENGINE_CDP_URL";
    /// Fire 引擎 TLS URL
    pub const FIRE_ENGINE_TLS_URL: &str = "FIRE_ENGINE_TLS_URL";
    /// Fire 引擎基础 URL
    pub const FIRE_ENGINE_URL: &str = "FIRE_ENGINE_URL";

    // === FlareSolverr ===
    /// FlareSolverr URL
    pub const FLARESOLVERR_URL: &str = "FLARESOLVERR_URL";

    // === 测试用环境变量 ===
    /// 测试数据库 URL
    pub const TEST_DATABASE_URL: &str = "TEST_DATABASE_URL";
    /// 测试数据库密码
    pub const TEST_DATABASE_PASSWORD: &str = "TEST_DATABASE_PASSWORD";
    /// 测试 Redis URL
    pub const TEST_REDIS_URL: &str = "TEST_REDIS_URL";
    /// 测试 Redis 端口
    pub const TEST_REDIS_PORT: &str = "TEST_REDIS_PORT";
    /// 测试 Webhook 密钥
    pub const TEST_WEBHOOK_SECRET: &str = "TEST_WEBHOOK_SECRET";
    /// 测试 S3 访问密钥
    pub const TEST_S3_ACCESS_KEY: &str = "TEST_S3_ACCESS_KEY";
    /// 测试 S3 密钥
    pub const TEST_S3_SECRET_KEY: &str = "TEST_S3_SECRET_KEY";
    /// 测试 S3 端点
    pub const TEST_S3_ENDPOINT: &str = "TEST_S3_ENDPOINT";
    /// 跳过 S3 测试
    pub const SKIP_S3_TESTS: &str = "SKIP_S3_TESTS";
    /// 测试 Fire 引擎 CDP URL
    pub const TEST_FIRE_ENGINE_CDP_URL: &str = "TEST_FIRE_ENGINE_CDP_URL";
}

/// 导出测试常量（仅在测试模式下）
#[cfg(test)]
pub use testing::*;

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
