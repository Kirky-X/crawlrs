// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![deprecated(
    since = "0.1.0",
    note = "Use `crate::common::constants` instead. This module will be removed in a future version."
)]

/// 全局常量定义 - 避免代码中的魔法数字和硬编码值
///
/// ⚠️ 已废弃 - 请使用 `crate::common::constants`
///
/// 这些常量用于在整个项目中保持一致性和可维护性
/// 遵循代码审查报告中发现的优化建议
/// HTTP 内容类型常量
pub mod content_types {
    pub const TEXT_HTML: &str = "text/html";
    pub const APPLICATION_JSON: &str = "application/json";
    pub const TEXT_PLAIN: &str = "text/plain";
}

/// HTTP 头部常量
pub mod headers {
    pub const CONTENT_TYPE: &str = "Content-Type";
    pub const USER_AGENT: &str = "User-Agent";
    pub const ACCEPT: &str = "Accept";
    pub const ACCEPT_LANGUAGE: &str = "Accept-Language";
}

/// 任务状态常量
pub mod task_status {
    pub const QUEUED: &str = "queued";
    pub const ACTIVE: &str = "active";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
    pub const CANCELLED: &str = "cancelled";
}

/// 存储类型常量
pub mod storage_types {
    pub const LOCAL: &str = "local";
    pub const S3: &str = "s3";
}

/// 搜索引擎常量
pub mod search_engines {
    pub const GOOGLE: &str = "google";
    pub const BING: &str = "bing";
    pub const BAIDU: &str = "baidu";
    pub const SOGOU: &str = "sogou";
}

/// 默认值常量
pub mod defaults {
    pub const APP_ENVIRONMENT: &str = "default";
    pub const DEFAULT_LANGUAGE: &str = "default";
    pub const DEFAULT_COUNTRY: &str = "default";
    pub const CHINESE_RATIO_THRESHOLD: f32 = 0.3;
    pub const ASCII_RATIO_THRESHOLD: f32 = 0.8;
}

/// 错误分类常量
pub mod error_types {
    pub const TIMEOUT: &str = "timeout";
    pub const SSRF: &str = "ssrf_protection";
    pub const NETWORK: &str = "network_error";
    pub const CIRCUIT: &str = "circuit_breaker";
    pub const BROWSER: &str = "browser_error";
    pub const OTHER: &str = "other";
}

/// 语言常量
pub mod languages {
    pub const CHINESE: &str = "zh";
    pub const ENGLISH: &str = "en";
    pub const UNKNOWN: &str = "unknown";
}

/// 缓存键前缀常量
pub mod cache_keys {
    pub const RATELIMIT_PREFIX: &str = "crawlrs:ratelimit";
    pub const SEARCH_CACHE_PREFIX: &str = "crawlrs:search";
    pub const ROBOTS_CACHE_PREFIX: &str = "crawlrs:robots";
}

/// 评分权重常量 - 避免relevance_scorer.rs中的魔法数字
pub mod scoring_weights {
    pub const TITLE_EXACT_MATCH: f64 = 2.0;
    pub const TITLE_PARTIAL_MATCH: f64 = 1.5;
    pub const DESCRIPTION_MATCH: f64 = 1.2;
    pub const SECONDARY_MATCH: f64 = 0.8;
    pub const TERTIARY_MATCH: f64 = 0.6;
    pub const BASE_SCORE: f64 = 0.5;
    pub const PENALTY_FACTOR: f64 = 0.8;
    pub const BOOST_FACTOR: f64 = 1.2;
}

/// 系统负载阈值常量 - 避免crawl_service.rs中的魔法数字
pub mod load_thresholds {
    pub const HIGH_LOAD: f64 = 0.8;
    pub const MEDIUM_LOAD: f64 = 0.6;
    pub const MEDIUM_LOAD_DEPTH_FACTOR: f64 = 0.75;
}

/// 处理时间限制常量 - 避免processor.rs中的魔法数字
pub mod processing_limits {
    pub const MAX_TEXT_PROCESSING_TIME_SECS: u64 = 30;
    pub const MAX_EXTRACTION_TIME_SECS: u64 = 60;
    pub const MAX_ROBOTS_FETCH_TIME_SECS: u64 = 5;
    pub const MAX_CONTENT_SIZE_MB: usize = 10;
}

/// 资源使用阈值常量 - 避免metrics.rs中的魔法数字
pub mod resource_thresholds {
    pub const CPU_USAGE_HIGH: f64 = 0.9;
    pub const CPU_USAGE_MEDIUM: f64 = 0.8;
    pub const MEMORY_USAGE_HIGH: f64 = 0.9;
    pub const MEMORY_USAGE_MEDIUM: f64 = 0.8;
}

/// 缓存配置常量 - 避免cache_strategy.rs中的魔法数字
pub mod cache_config {
    pub const DEFAULT_TTL_SECS: u64 = 300; // 5分钟
    pub const ROBOTS_TTL_SECS: u64 = 3600; // 1小时
    pub const REDIS_CACHE_TTL_SECS: u64 = 86400; // 24小时
    pub const MAX_CACHE_ENTRIES: usize = 10000;
    pub const MEMORY_CACHE_MAX_SIZE: usize = 1000;
    pub const EVICTION_BUFFER_PERCENT: usize = 10;
    pub const PERFORMANCE_HISTORY_MAX_SIZE: usize = 1000;
}

/// 重试策略常量 - 避免robots.rs中的魔法数字
pub mod retry_config {
    pub const MAX_RETRIES: u32 = 5;
    pub const INITIAL_BACKOFF_MS: u64 = 2000; // 2秒
    pub const MAX_BACKOFF_MS: u64 = 10000; // 10秒
}

/// API安全常量 - 避免settings.rs中的弱密码检测
pub mod security_limits {
    pub const MIN_WEBHOOK_SECRET_LENGTH: usize = 32;
    pub const MIN_S3_SECRET_LENGTH: usize = 32;
    pub const WEAK_SECRET_LENGTH: usize = 8;
}

/// 数据库连接池常量 - 避免settings.rs中的魔法数字
pub mod database_config {
    pub const DEFAULT_MAX_CONNECTIONS: u32 = 100;
    pub const DEFAULT_MIN_CONNECTIONS: u32 = 10;
    pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
    pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 300;
}

/// 服务器配置常量 - 避免settings.rs中的魔法数字
pub mod server_config {
    pub const DEFAULT_HOST: &str = "0.0.0.0";
    pub const DEFAULT_PORT: u16 = 8899;
    pub const DEFAULT_RATE_LIMIT_RPM: u32 = 100;
    pub const DEFAULT_TEAM_LIMIT: u32 = 10;
    pub const DEFAULT_TASK_LOCK_DURATION_SECS: u64 = 300;
}

/// A/B测试配置常量 - 避免search模块中的魔法数字
pub mod ab_test_config {
    pub const DEFAULT_VARIANT_B_WEIGHT: f64 = 0.1;
    pub const PERFORMANCE_CHECK_PROBABILITY: f64 = 0.01;
}
