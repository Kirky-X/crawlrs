// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
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
/// * `api_key` - Google Search API 密钥
/// * `cx` - Google Custom Search Engine ID
#[derive(Debug, Clone, Deserialize)]
pub struct GoogleSearchSettings {
    /// Google Search API 密钥
    pub api_key: Option<String>,
    /// Google Custom Search Engine ID
    pub cx: Option<String>,
}

/// Bing Search API 配置设置
#[derive(Debug, Clone, Deserialize)]
pub struct BingSearchSettings {
    /// Bing Search API 密钥
    pub api_key: Option<String>,
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
