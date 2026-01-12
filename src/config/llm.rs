// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! LLM 配置
//!
//! 包含大语言模型服务配置

use serde::Deserialize;

/// LLM 配置设置
///
/// 配置 LLM（大语言模型）服务的参数
///
/// # 字段说明
///
/// * `api_key` - LLM API 密钥
/// * `model` - 使用的模型名称，默认 "gpt-3.5-turbo"
/// * `api_base_url` - LLM API 基础 URL，默认 "https://api.openai.com/v1"
#[derive(Debug, Clone, Deserialize)]
pub struct LLMSettings {
    /// LLM API 密钥
    pub api_key: Option<String>,
    /// 使用的模型名称
    pub model: Option<String>,
    /// LLM API 基础 URL
    pub api_base_url: Option<String>,
}
