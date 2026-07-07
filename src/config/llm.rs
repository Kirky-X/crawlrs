// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! LLM 配置
//!
//! 包含大语言模型服务配置

use confers::Config;
use serde::{Deserialize, Serialize};

/// LLM 配置设置
///
/// 配置 LLM（大语言模型）服务的参数
///
/// # 字段说明
///
/// * `api_key` - LLM API 密钥（敏感信息，仅 crate 可见）
/// * `model` - 使用的模型名称，默认 "gpt-3.5-turbo"
/// * `api_base_url` - LLM API 基础 URL，默认 "https://api.openai.com/v1"
///
/// # 安全提示
///
/// `api_key` 字段包含 LLM API 密钥，泄露可能导致未经授权的访问。
/// 该字段仅对 crate 可见，外部模块应使用 `api_key()` 方法访问。
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__LLM__")]
pub struct LLMSettings {
    /// LLM 提供商 (openai, ollama, anthropic, etc)
    pub provider: Option<String>,

    /// LLM API 密钥 (敏感信息)
    pub(crate) api_key: Option<String>,

    /// 使用的模型名称
    #[config(default = "gpt-3.5-turbo".to_string())]
    pub model: Option<String>,

    /// LLM API 基础 URL
    #[config(default = "https://api.openai.com/v1".to_string())]
    pub api_base_url: Option<String>,
}

impl LLMSettings {
    /// 获取 LLM API 密钥
    ///
    /// # 安全提示
    ///
    /// 此方法返回 LLM API 密钥，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }
}
