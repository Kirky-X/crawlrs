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

/// LLM 提取注入净化配置（间接提示注入源头防御）
///
/// 键值语义与 `config/default.toml` `[llm.sanitization]` 节一一对应；
/// 由 [`crate::domain::services::llm::sanitizer::SanitizationConfig`] 消费。
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__LLM__SANITIZATION__")]
pub struct LlmSanitizationSettings {
    /// 总开关：false 时净化层完全旁路（回退旧行为）
    #[serde(default = "default_sanitization_enabled")]
    #[config(default = true)]
    pub enabled: bool,

    /// DOM 层：跳过隐藏子树（内联 display:none/visibility:hidden/opacity:0、hidden 属性、aria-hidden）
    #[serde(default = "default_sanitization_strip_hidden_nodes")]
    #[config(default = true)]
    pub strip_hidden_nodes: bool,

    /// 文本层：移除零宽字符（U+200B/U+200C/U+200D/U+FEFF）
    #[serde(default = "default_sanitization_strip_zero_width")]
    #[config(default = true)]
    pub strip_zero_width: bool,

    /// 文本层：剥除 `<!-- ... -->` HTML 注释载荷
    #[serde(default = "default_sanitization_strip_html_comments")]
    #[config(default = true)]
    pub strip_html_comments: bool,

    /// 文本长度上限（char 边界截断；0 = 不截断）
    #[serde(default = "default_sanitization_max_text_length")]
    #[config(default = 500_000)]
    pub max_text_length: usize,

    /// 可疑指令行的处置：strip_and_flag（移除并计数，默认）/ flag_only（仅计数观察）
    ///
    /// confers ConfigValue 不支持任意枚举（同 `id.algorithm` 先例）：配置面收
    /// 字符串，[`From`] 转换时解析，未知值回退 strip_and_flag（更严格方向）并告警。
    #[serde(default = "default_sanitization_suspicious_pattern_action")]
    #[config(default = "strip_and_flag".to_string())]
    pub suspicious_pattern_action: String,
}

impl From<&LlmSanitizationSettings>
    for crate::domain::services::llm::sanitizer::SanitizationConfig
{
    fn from(s: &LlmSanitizationSettings) -> Self {
        use crate::domain::services::llm::sanitizer::SuspiciousAction;
        let action = match s.suspicious_pattern_action.as_str() {
            "flag_only" | "flagOnly" => SuspiciousAction::FlagOnly,
            "strip_and_flag" | "stripAndFlag" => SuspiciousAction::StripAndFlag,
            other => {
                log::warn!(
                    "llm.sanitization.suspicious_pattern_action = '{other}' is unknown \
                     (options: strip_and_flag | flag_only); falling back to strip_and_flag"
                );
                SuspiciousAction::StripAndFlag
            }
        };
        Self {
            enabled: s.enabled,
            strip_hidden_nodes: s.strip_hidden_nodes,
            strip_zero_width: s.strip_zero_width,
            strip_html_comments: s.strip_html_comments,
            max_text_length: s.max_text_length,
            suspicious_pattern_action: action,
        }
    }
}

/// serde 默认值（与 `#[config(default = ...)]` 保持一致）
fn default_sanitization_enabled() -> bool {
    true
}

fn default_sanitization_strip_hidden_nodes() -> bool {
    true
}

fn default_sanitization_strip_zero_width() -> bool {
    true
}

fn default_sanitization_strip_html_comments() -> bool {
    true
}

fn default_sanitization_max_text_length() -> usize {
    500_000
}

/// serde 默认值：可疑行处置动作（与 `#[config(default = "strip_and_flag")]` 一致）
fn default_sanitization_suspicious_pattern_action() -> String {
    "strip_and_flag".to_string()
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

    /// 注入净化配置（间接提示注入源头防御；默认安全全开）
    ///
    /// `serde(default)` 兼容旧 config 文件缺 `[llm.sanitization]` 节。
    #[serde(default)]
    pub sanitization: LlmSanitizationSettings,

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
            sanitization: LlmSanitizationSettings::default(),
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

    /// 净化配置默认值与 config/default.toml `[llm.sanitization]` 节一致（安全全开）
    #[test]
    fn test_llm_sanitization_settings_defaults_match_spec() {
        let s = LlmSanitizationSettings::default();
        assert!(s.enabled, "默认开：隐藏内容进 LLM 本身就是缺陷");
        assert!(s.strip_hidden_nodes);
        assert!(s.strip_zero_width);
        assert!(s.strip_html_comments);
        assert_eq!(s.max_text_length, 500_000);
        assert_eq!(s.suspicious_pattern_action, "strip_and_flag");
    }

    /// 旧配置缺 `[llm.sanitization]` 节时 serde 默认兜底（向后兼容）
    #[test]
    fn test_llm_settings_without_sanitization_section_defaults() {
        let toml = r#"
            model = "qwen3:1.7b"
        "#;
        let s: LLMSettings = toml::from_str(toml).expect("deserialize");
        assert!(s.sanitization.enabled);
        assert_eq!(s.sanitization.max_text_length, 500_000);
    }

    /// 显式 `enabled = false` 可完整关闭净化（回退旧行为）
    #[test]
    fn test_llm_sanitization_settings_toml_parses_all_keys() {
        let toml = r#"
            enabled = false
            strip_hidden_nodes = false
            strip_zero_width = false
            strip_html_comments = false
            max_text_length = 1000
            suspicious_pattern_action = "flag_only"
        "#;
        let s: LlmSanitizationSettings = toml::from_str(toml).expect("deserialize");
        assert!(!s.enabled);
        assert!(!s.strip_hidden_nodes);
        assert!(!s.strip_zero_width);
        assert!(!s.strip_html_comments);
        assert_eq!(s.max_text_length, 1000);
        assert_eq!(s.suspicious_pattern_action, "flag_only");
    }

    /// `From<&LlmSanitizationSettings> for SanitizationConfig` 一一映射
    #[test]
    fn test_llm_sanitization_settings_converts_to_config() {
        let s = LlmSanitizationSettings::default();
        let cfg = crate::domain::services::llm::sanitizer::SanitizationConfig::from(&s);
        assert!(cfg.enabled);
        assert!(cfg.strip_hidden_nodes);
        assert_eq!(cfg.max_text_length, 500_000);
        assert_eq!(
            cfg.suspicious_pattern_action,
            crate::domain::services::llm::sanitizer::SuspiciousAction::StripAndFlag
        );

        let off = LlmSanitizationSettings {
            enabled: false,
            ..LlmSanitizationSettings::default()
        };
        let cfg = crate::domain::services::llm::sanitizer::SanitizationConfig::from(&off);
        assert!(!cfg.enabled);
    }

    /// 未知 action 字符串回退 strip_and_flag（更严格方向）
    #[test]
    fn test_llm_sanitization_unknown_action_falls_back_to_strip() {
        let s = LlmSanitizationSettings {
            suspicious_pattern_action: "nonsense".to_string(),
            ..LlmSanitizationSettings::default()
        };
        let cfg = crate::domain::services::llm::sanitizer::SanitizationConfig::from(&s);
        assert_eq!(
            cfg.suspicious_pattern_action,
            crate::domain::services::llm::sanitizer::SuspiciousAction::StripAndFlag
        );

        let s = LlmSanitizationSettings {
            suspicious_pattern_action: "flag_only".to_string(),
            ..LlmSanitizationSettings::default()
        };
        let cfg = crate::domain::services::llm::sanitizer::SanitizationConfig::from(&s);
        assert_eq!(
            cfg.suspicious_pattern_action,
            crate::domain::services::llm::sanitizer::SuspiciousAction::FlagOnly
        );
    }
}
