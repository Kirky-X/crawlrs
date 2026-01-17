// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 搜索相关配置
//!
//! 包含 Google Search、Bing Search 和搜索功能配置

use serde::Deserialize;

/// Google Custom Search API 配置设置
///
/// 配置 Google Custom Search API 的参数
///
/// # 字段说明
///
/// * `api_key` - Google Search API 密钥（敏感信息，仅 crate 可见）
/// * `cx` - Google Custom Search Engine ID
///
/// # 安全提示
///
/// `api_key` 字段包含 Google Search API 密钥，泄露可能导致未经授权的访问。
/// 该字段仅对 crate 可见，外部模块应使用 `api_key()` 方法访问。
#[derive(Debug, Clone, Deserialize)]
pub struct GoogleSearchSettings {
    /// Google Search API 密钥 (敏感信息)
    pub(crate) api_key: Option<String>,
    /// Google Custom Search Engine ID
    pub cx: Option<String>,
}

impl GoogleSearchSettings {
    /// 获取 Google Search API 密钥
    ///
    /// # 安全提示
    ///
    /// 此方法返回 Google Search API 密钥，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }
}

/// Bing Search API 配置设置
///
/// # 安全提示
///
/// `api_key` 字段包含 Bing Search API 密钥，泄露可能导致未经授权的访问。
/// 该字段仅对 crate 可见，外部模块应使用 `api_key()` 方法访问。
#[derive(Debug, Clone, Deserialize)]
pub struct BingSearchSettings {
    /// Bing Search API 密钥 (敏感信息)
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
#[derive(Debug, Clone, Deserialize)]
pub struct SearchSettings {
    /// 是否启用 A/B 测试
    pub ab_test_enabled: bool,
    /// Variant B 的流量权重 (0.0 到 1.0)
    pub variant_b_weight: f64,
}
