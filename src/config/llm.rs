// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! LLM 配置
//!
//! 包含大语言模型服务配置

use serde::{Deserialize, Serialize};

/// LLM 提取 token 窗口分块配置（吸收点 F8；消费方 `llm::chunker::ChunkingConfig`）
///
/// 键值语义与 `config/default.toml` `[llm.chunking]` 节一一对应。
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__LLM__CHUNKING__")]
pub struct LlmChunkingSettings {
    /// 总开关：false 时长文仍单 prompt 直发（现状，向后兼容）
    #[serde(default)]
    #[config(default = false)]
    pub enabled: bool,

    /// 模型上下文窗口（token）；查不到专用窗口时用此值兜底
    #[serde(default = "default_chunking_model_context_tokens")]
    #[config(default = 8192)]
    pub model_context_tokens: u32,

    /// 单次回答预留输出 token
    #[serde(default = "default_chunking_max_output_tokens")]
    #[config(default = 2048)]
    pub max_output_tokens: u32,

    /// prompt 模板/边界标记等注入开销预留 token
    #[serde(default = "default_chunking_prompt_overhead_tokens")]
    #[config(default = 1024)]
    pub prompt_overhead_tokens: u32,

    /// token→字符保守换算（1 token ≈ N 字符）；÷2 余量在分块预算内固定
    #[serde(default = "default_chunking_chars_per_token")]
    #[config(default = 4.0)]
    pub chars_per_token: f64,

    /// 分块提取并发度（tokio JoinSet 并行上限）
    #[serde(default = "default_chunking_max_parallel")]
    #[config(default = 4)]
    pub max_parallel: usize,

    /// 分块结果单次 merge prompt 综合（false 时逐块结果直接按字段拼接）
    #[serde(default = "default_chunking_merge_enabled")]
    #[config(default = true)]
    pub merge_enabled: bool,

    /// 术语预检：schema 字段名拆词（停用词过滤后）均不在文本中时告警并短路（省 token）
    #[serde(default = "default_chunking_term_precheck_enabled")]
    #[config(default = true)]
    pub term_precheck_enabled: bool,
}

/// serde 默认值：模型窗口兜底（与 `#[config(default = 8192)]` 保持一致）
fn default_chunking_model_context_tokens() -> u32 {
    8192
}

/// serde 默认值：回答预留（与 `#[config(default = 2048)]` 保持一致）
fn default_chunking_max_output_tokens() -> u32 {
    2048
}

/// serde 默认值：模板注入开销预留（与 `#[config(default = 1024)]` 保持一致）
fn default_chunking_prompt_overhead_tokens() -> u32 {
    1024
}

/// serde 默认值：token→字符换算（与 `#[config(default = 4.0)]` 保持一致）
fn default_chunking_chars_per_token() -> f64 {
    4.0
}

/// serde 默认值：并行上限（与 `#[config(default = 4)]` 保持一致）
fn default_chunking_max_parallel() -> usize {
    4
}

/// serde 默认值：merge 开关（与 `#[config(default = true)]` 保持一致）
fn default_chunking_merge_enabled() -> bool {
    true
}

/// serde 默认值：术语预检开关（与 `#[config(default = true)]` 保持一致）
fn default_chunking_term_precheck_enabled() -> bool {
    true
}

/// serde 默认值：溯源覆盖率门下限（与 `#[config(default = 0.75)]` 保持一致）
fn default_provenance_min_coverage() -> f64 {
    0.75
}

fn default_request_timeout_secs() -> u64 {
    120
}

/// LLM 配置设置
///
/// 配置 LLM 提取编排参数。供应商访问语义（provider / api_key / api_base_url）
/// 已随手写供应商栈退役——承载实现由宿主注入，供应商凭据不进入 crawlrs 配置。
///
/// # 字段说明
///
/// * `model` - 使用的模型名称，默认 "gpt-3.5-turbo"（宿主 carrier 可读取）
/// * `chunking` / `provenance_*` - 提取编排参数（对注入的 carrier 同样生效）
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__LLM__")]
pub struct LLMSettings {
    /// 使用的模型名称
    #[config(default = Some("gpt-3.5-turbo".to_string()))]
    pub model: Option<String>,

    /// LLM 提取 token 窗口分块配置（F8）
    ///
    /// `serde(default)` 兼容旧 config 文件缺 `[llm.chunking]` 节
    /// （同 `sitemap` / `crawl` 节先例）。
    #[serde(default)]
    pub chunking: LlmChunkingSettings,

    /// 直调单请求超时秒数
    ///
    /// 覆盖本地/自托管端点的冷加载与长生成（实测 Ollama 模型重载可达 ~50s，
    /// 30s 硬编码会导致全部直调超时）。`serde(default)` 兼容旧配置缺键。
    #[serde(default = "default_request_timeout_secs")]
    #[config(default = 120)]
    pub request_timeout_secs: u64,

    /// 提取结果字符级溯源对齐开关（F9）
    ///
    /// true 时 `extract_data_with_provenance` 对结果字符串字段尝试对齐回
    /// 原文字符区间，产出 `_provenance` 映射（`serde(default)` 兼容旧配置缺键）。
    #[serde(default)]
    #[config(default = false)]
    pub provenance_enabled: bool,

    /// 溯源对齐双门下限：token 覆盖率门（字符密度门 1/3 为代码内常量）（F9）
    ///
    /// 对齐结果须同时满足 coverage ≥ 此值且 density ≥ 1/3，否则该字段区间置
    /// null 由调用方过滤。
    #[serde(default = "default_provenance_min_coverage")]
    #[config(default = 0.75)]
    pub provenance_min_coverage: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LlmChunkingSettings 默认值与 config/default.toml `[llm.chunking]`
    /// 节及蓝图 configKeys 一一对应（F8）
    #[test]
    fn test_llm_chunking_settings_defaults_match_spec() {
        let s = LlmChunkingSettings::default();
        assert!(!s.enabled, "默认关：长文单 prompt 直发（现状）");
        assert_eq!(s.model_context_tokens, 8192, "模型窗口兜底值");
        assert_eq!(s.max_output_tokens, 2048, "回答预留");
        assert_eq!(s.prompt_overhead_tokens, 1024, "模板注入开销预留");
        assert!((s.chars_per_token - 4.0).abs() < f64::EPSILON, "保守换算");
        assert_eq!(s.max_parallel, 4, "JoinSet 并行上限");
        assert!(s.merge_enabled, "单次 merge prompt 综合");
        assert!(s.term_precheck_enabled, "零成本术语预检短路");
    }

    /// LlmChunkingSettings 从 toml 全键反序列化
    #[test]
    fn test_llm_chunking_settings_toml_parses_all_keys() {
        let toml = r#"
            enabled = true
            model_context_tokens = 32768
            max_output_tokens = 4096
            prompt_overhead_tokens = 512
            chars_per_token = 1.5
            max_parallel = 8
            merge_enabled = false
            term_precheck_enabled = false
        "#;
        let s: LlmChunkingSettings = toml::from_str(toml).expect("deserialize");
        assert!(s.enabled);
        assert_eq!(s.model_context_tokens, 32768);
        assert_eq!(s.max_output_tokens, 4096);
        assert_eq!(s.prompt_overhead_tokens, 512);
        assert!((s.chars_per_token - 1.5).abs() < f64::EPSILON);
        assert_eq!(s.max_parallel, 8);
        assert!(!s.merge_enabled);
        assert!(!s.term_precheck_enabled);
    }

    /// 旧配置缺 `[llm.chunking]` 节时 serde 默认兜底（向后兼容）
    #[test]
    fn test_llm_settings_without_chunking_section_defaults() {
        let toml = r#"
            model = "qwen3:1.7b"
        "#;
        let s: LLMSettings = toml::from_str(toml).expect("deserialize");
        assert!(!s.chunking.enabled);
        assert_eq!(s.chunking.model_context_tokens, 8192);
    }

    /// 溯源配置默认值与 config/default.toml `[llm]` 节及蓝图 configKeys 一致（F9）
    #[test]
    fn test_llm_provenance_settings_defaults_match_spec() {
        let s = LLMSettings::default();
        assert!(!s.provenance_enabled, "默认关：结果不挂 _provenance");
        assert!(
            (s.provenance_min_coverage - 0.75).abs() < f64::EPSILON,
            "langextract 同款覆盖率门 0.75"
        );
    }

    /// 溯源配置从 toml 全键反序列化（F9）
    #[test]
    fn test_llm_provenance_settings_toml_parses() {
        let toml = r#"
            provenance_enabled = true
            provenance_min_coverage = 0.8
        "#;
        let s: LLMSettings = toml::from_str(toml).expect("deserialize");
        assert!(s.provenance_enabled);
        assert!((s.provenance_min_coverage - 0.8).abs() < f64::EPSILON);
    }

    /// 旧配置缺溯源键时 serde 默认兜底：开关关、覆盖率门 0.75（向后兼容，F9）
    #[test]
    fn test_llm_settings_without_provenance_keys_defaults() {
        let toml = r#"
            model = "qwen3:1.7b"
        "#;
        let s: LLMSettings = toml::from_str(toml).expect("deserialize");
        assert!(!s.provenance_enabled);
        assert!((s.provenance_min_coverage - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_llm_default_model() {
        let settings = LLMSettings::default();
        assert_eq!(
            settings.model.as_deref(),
            Some("gpt-3.5-turbo"),
            "default model should be gpt-3.5-turbo"
        );
    }

    /// 供应商语义配置已退役：旧配置里的 provider/api_key/api_base_url 键不再解析
    #[test]
    fn test_llm_retired_provider_keys_are_ignored() {
        let toml = r#"
            model = "host-carrier-model"
            provider = "ollama"
            api_key = "legacy-secret"
            api_base_url = "http://localhost:11434/v1"
        "#;
        let settings: LLMSettings = toml::from_str(toml).expect("deserialize");
        // 旧供应商键被 serde 忽略，只有保留的编排键生效
        assert_eq!(settings.model.as_deref(), Some("host-carrier-model"));
        assert!(!settings.provenance_enabled);
    }

    #[test]
    fn test_llm_serde_roundtrip_default() {
        let settings = LLMSettings::default();
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: LLMSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.model.as_deref(), settings.model.as_deref());
        assert_eq!(
            back.chunking.model_context_tokens,
            settings.chunking.model_context_tokens
        );
        assert_eq!(back.provenance_enabled, settings.provenance_enabled);
    }

    #[test]
    fn test_llm_clone_preserves_fields() {
        let settings = LLMSettings {
            model: Some("llama2".to_string()),
            chunking: LlmChunkingSettings::default(),
            provenance_enabled: true,
            request_timeout_secs: 90,
            provenance_min_coverage: 0.8,
        };
        let cloned = settings.clone();
        assert_eq!(cloned.model, settings.model);
        assert_eq!(cloned.provenance_enabled, settings.provenance_enabled);
        assert_eq!(cloned.request_timeout_secs, 90);
    }

    #[test]
    fn test_llm_debug_contains_model() {
        let settings = LLMSettings {
            model: Some("gpt-4".to_string()),
            ..LLMSettings::default()
        };
        let debug = format!("{:?}", settings);
        assert!(debug.contains("LLMSettings"));
        assert!(debug.contains("gpt-4"));
    }
}
