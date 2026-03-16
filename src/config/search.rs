// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 搜索相关配置
//!
//! 包含 Bing Search 和搜索功能配置

use confers::Config;
use serde::{Deserialize, Serialize};

/// Bing Search API 配置设置
///
/// # 安全提示
///
/// `api_key` 字段包含 Bing Search API 密钥，泄露可能导致未经授权的访问。
/// 该字段仅对 crate 可见，外部模块应使用 `api_key()` 方法访问。
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__BING_SEARCH__")]
pub struct BingSearchSettings {
    /// Bing Search API 密钥 (敏感信息)
    #[config(sensitive)]
    pub(crate) api_key: Option<String>,
}

impl BingSearchSettings {
    /// 获取 Bing Search API 密钥
    ///
    /// # 安全提示
    ///
    /// 此方法返回 Bing Search API 密钥，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }
}

/// 搜索配置设置
///
/// 配置搜索相关功能参数
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__SEARCH__")]
pub struct SearchSettings {
    /// 是否启用 A/B 测试
    #[config(default = false)]
    pub ab_test_enabled: bool,

    /// Variant B 的流量权重 (0.0 到 1.0)
    #[config(default = 0.1)]
    pub variant_b_weight: f64,

    /// 搜索超时时间（秒）
    #[config(default = 30)]
    pub timeout_seconds: u64,

    /// 是否启用速率限制
    #[config(default = true)]
    pub rate_limiting_enabled: bool,

    /// 是否启用测试数据
    #[config(default = false)]
    pub test_data_enabled: bool,

    /// 最大重试次数
    #[config(default = 3)]
    pub max_retries: u32,

    /// 重试延迟（毫秒）
    #[config(default = 1000)]
    pub retry_delay_ms: u64,
}
