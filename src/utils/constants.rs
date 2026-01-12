// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 全局常量定义 - 避免代码中的魔法字符串和重复调用
///
/// 这些常量用于在整个项目中保持一致性和可维护性

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
