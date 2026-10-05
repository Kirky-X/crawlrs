// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! LLMService - LLM provider interaction handling
//!
//! 核心调度层：组合 `prompt_builder`（模板 + prompt 构造）与
//! `provider_adapter`（HTTP 请求发送）完成 LLM 数据提取；
//! 长文经 `chunker`（F8）按模型窗口分块并行提取。

pub mod chunker; // token 窗口分块 + 并行提问 + merge（F8）
pub mod prompt_builder;
pub mod provenance; // 提取结果字符级溯源对齐（F9）
pub mod provider_adapter;
pub mod vision_adapter; // 视觉模型适配器（MLLM 引擎依赖）

use chunker::ChunkingConfig;

#[cfg(test)]
use prompt_builder::TemplateLoaderTrait;
use prompt_builder::{
    build_prompt, parse_llm_response, FileTemplateLoader, InMemoryTemplateLoader, TemplateLoader,
};
use provider_adapter::resolve_provider;

use crate::config::settings::Settings;
use anyhow::Result;
use async_trait::async_trait;
#[cfg(feature = "llm")]
use genai::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Token usage tracking for LLM calls
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[async_trait]
pub trait LLMServiceTrait: Send + Sync {
    async fn extract_data(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, TokenUsage), anyhow::Error>;

    /// 分块提取：长文按模型窗口切块 → 并行提问 → merge 综合（F8）
    ///
    /// 默认实现等价 [`LLMServiceTrait::extract_data`]（未接入分块的实现方零改动）；
    /// `LLMService` 覆盖实现，`[llm.chunking].enabled = false` 时同样直走单发路径。
    async fn extract_data_chunked(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, TokenUsage), anyhow::Error> {
        self.extract_data(text, schema, format).await
    }

    /// 提取 + 字符级溯源（F9）：结果字符串字段对齐回原文字符区间。
    ///
    /// 返回 `(value, provenance, usage)`；`provenance` 为
    /// `{field_path: {start, end, coverage}}` 映射（对齐失败字段为 null，调用方
    /// 过滤），`llm.provenance_enabled = false` 时为 `None`（mock 零改动）。
    /// 启用时 `value` 额外并列挂载 `_provenance` 键，RAG 入库链可直接持久该 JSON。
    async fn extract_data_with_provenance(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, Option<Value>, TokenUsage), anyhow::Error> {
        let (value, usage) = self.extract_data(text, schema, format).await?;
        Ok((value, None, usage))
    }
}

/// LLM服务 - 处理与LLM提供商的交互
#[derive(Clone)]
pub struct LLMService {
    http_client: Arc<reqwest::Client>,
    /// LLM 客户端
    #[cfg(feature = "llm")]
    _client: Client,
    /// 使用的模型
    model: String,
    /// 提供商
    provider: String,
    /// API 基础 URL
    api_base_url: Option<String>,
    /// API 密钥
    api_key: Option<String>,
    /// 提示模板（缓存）
    templates: HashMap<String, String>,
    /// token 窗口分块配置（F8；来自 `[llm.chunking]`，经 extract_data_with_provenance 接线）
    chunking: ChunkingConfig,
    /// 直调单次回答 token 上限（取 `[llm.chunking] max_output_tokens`；封顶生成时长防挂死）
    direct_max_output_tokens: Option<u64>,
    /// 直调单请求超时秒数（`[llm] request_timeout_secs`）
    request_timeout_secs: u64,
    /// 溯源对齐配置（F9；来自 `[llm]` 的 `provenance_*` 两键，同上入口接线）
    provenance: provenance::ProvenanceConfig,
}

impl LLMService {
    /// Create LLMService with settings and custom template loader
    pub fn new_with_template_loader(
        settings: &Settings,
        http_client: Arc<reqwest::Client>,
        template_loader: TemplateLoader,
    ) -> Self {
        let templates = template_loader.load_templates().unwrap_or_default();
        let provider = resolve_provider(settings);
        let model = settings
            .llm
            .model
            .clone()
            .unwrap_or_else(|| "gpt-3.5-turbo".to_string());
        let api_base_url = settings.llm.api_base_url.clone();
        let api_key = settings.llm.api_key.clone();
        let chunking = ChunkingConfig::from_settings(&settings.llm.chunking);
        let provenance = provenance::ProvenanceConfig::from_settings(&settings.llm);
        let direct_max_output_tokens = Some(u64::from(chunking.max_output_tokens));
        let request_timeout_secs = settings.llm.request_timeout_secs;

        Self {
            http_client,
            #[cfg(feature = "llm")]
            _client: Client::default(),
            model,
            provider,
            api_base_url,
            api_key,
            templates,
            chunking,
            direct_max_output_tokens,
            request_timeout_secs,
            provenance,
        }
    }

    /// Create LLMService with settings (uses FileTemplateLoader)
    pub fn new(settings: &Settings, http_client: Arc<reqwest::Client>) -> Self {
        Self::new_with_template_loader(
            settings,
            http_client,
            Arc::new(FileTemplateLoader::default()),
        )
    }

    /// Create LLMService with explicit config and custom template loader
    pub fn new_with_config_and_loader(
        _api_key: String,
        model: String,
        api_base_url: String,
        http_client: Arc<reqwest::Client>,
        template_loader: TemplateLoader,
    ) -> Self {
        let templates = template_loader.load_templates().unwrap_or_default();
        let chunking = ChunkingConfig::default();

        Self {
            http_client: http_client.clone(),
            #[cfg(feature = "llm")]
            _client: Client::default(),
            model,
            provider: "openai".to_string(),
            api_base_url: Some(api_base_url),
            api_key: Some(_api_key),
            templates,
            direct_max_output_tokens: Some(u64::from(chunking.max_output_tokens)),
            request_timeout_secs: 120,
            chunking,
            provenance: provenance::ProvenanceConfig::default(),
        }
    }

    /// Create LLMService with explicit config (uses InMemoryTemplateLoader with defaults)
    pub fn new_with_config(
        _api_key: String,
        model: String,
        api_base_url: String,
        http_client: Arc<reqwest::Client>,
    ) -> Self {
        let template_loader: TemplateLoader =
            Arc::new(InMemoryTemplateLoader::new().with_default_templates());
        let templates = template_loader.load_templates().unwrap_or_default();
        let chunking = ChunkingConfig::default();

        Self {
            http_client: http_client.clone(),
            #[cfg(feature = "llm")]
            _client: Client::default(),
            model,
            provider: "openai".to_string(),
            api_base_url: Some(api_base_url),
            api_key: Some(_api_key),
            templates,
            direct_max_output_tokens: Some(u64::from(chunking.max_output_tokens)),
            request_timeout_secs: 120,
            chunking,
            provenance: provenance::ProvenanceConfig::default(),
        }
    }
}

#[async_trait]
impl LLMServiceTrait for LLMService {
    async fn extract_data(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, TokenUsage)> {
        self.extract_data_internal(text, schema, format).await
    }

    /// 分块提取（F8）：`[llm.chunking].enabled = false` 时直走单发路径（现状语义）；
    /// 启用后由 [`chunker::extract_chunked`] 编排（预检 → 切块 → 并行 → merge）。
    async fn extract_data_chunked(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, TokenUsage)> {
        if !self.chunking.enabled {
            return self.extract_data(text, schema, format).await;
        }

        let extract_one = {
            let service = self.clone();
            let schema = schema.clone();
            let format = format.to_string();
            move |chunk: String| {
                let service = service.clone();
                let schema = schema.clone();
                let format = format.clone();
                async move { service.extract_data(&chunk, &schema, &format).await }
            }
        };
        let merge_call = {
            let service = self.clone();
            let schema = schema.clone();
            let format = format.to_string();
            move |payload: String| {
                let service = service.clone();
                let schema = schema.clone();
                let format = format.clone();
                // merge payload 内嵌指令与块结果 JSON 数组，复用同一模板/解析链路
                async move {
                    service
                        .extract_data_internal(&payload, &schema, &format)
                        .await
                }
            }
        };

        chunker::extract_chunked(text, schema, &self.chunking, extract_one, merge_call).await
    }

    /// 溯源提取（F9）：`provenance_enabled = false` 时仅按 chunking 开关决定是否
    /// 分块；启用时提取（分块开启则走 [`Self::extract_data_chunked`]）后对 merge 后
    /// 结果做全文对齐——merge 后字段值仍出自原文各块切片，全文对齐即可还原
    /// 全局 char 区间（chunker::extract_chunked 不暴露逐块偏移，故不做逐块换算）。
    async fn extract_data_with_provenance(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, Option<Value>, TokenUsage)> {
        let (value, usage) = if self.chunking.enabled {
            self.extract_data_chunked(text, schema, format).await?
        } else {
            self.extract_data(text, schema, format).await?
        };
        if !self.provenance.enabled {
            return Ok((value, None, usage));
        }
        let (value, prov) = provenance::apply_provenance(text, value, &self.provenance);
        Ok((value, prov, usage))
    }
}

impl LLMService {
    pub async fn extract_data_internal(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, TokenUsage)> {
        let template = self
            .templates
            .get(format)
            .ok_or_else(|| anyhow::anyhow!("Template not found for format: {}", format))?;

        // 使用 prompt_builder 构造 prompt
        let prompt = build_prompt(template, text, schema)?;

        // 使用 provider_adapter 发送请求
        let (content, usage) = if let Some(base_url) = &self.api_base_url {
            let url = provider_adapter::build_request_url(base_url);
            let response = provider_adapter::send_direct_request(
                &self.http_client,
                &url,
                &self.model,
                &prompt,
                self.api_key.as_deref(),
                self.direct_max_output_tokens,
                self.request_timeout_secs,
            )
            .await?;
            (response.content, response.usage)
        } else {
            let response =
                provider_adapter::send_genai_request(&self.provider, &self.model, &prompt).await?;
            (response.content, response.usage)
        };

        // 使用 prompt_builder 解析响应
        let data = parse_llm_response(&content, format)?;
        Ok((data, usage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    // ===== TokenUsage tests =====

    #[test]
    fn test_token_usage_default_is_zero() {
        let usage = TokenUsage::default();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    #[test]
    fn test_token_usage_serde_roundtrip() {
        let usage = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let serialized = serde_json::to_string(&usage).unwrap();
        let deserialized: TokenUsage = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.prompt_tokens, 100);
        assert_eq!(deserialized.completion_tokens, 50);
        assert_eq!(deserialized.total_tokens, 150);
    }

    #[test]
    fn test_token_usage_clone_preserves_values() {
        let usage = TokenUsage {
            prompt_tokens: 42,
            completion_tokens: 21,
            total_tokens: 63,
        };
        let cloned = usage.clone();
        assert_eq!(cloned.prompt_tokens, 42);
        assert_eq!(cloned.completion_tokens, 21);
        assert_eq!(cloned.total_tokens, 63);
    }

    // ===== FileTemplateLoader tests =====

    #[test]
    fn test_file_template_loader_new_valid_file_loads_templates() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("crawlrs_test_llm_loader");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("test_prompts.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(
            f,
            r#"
[extraction]
json = 'Extract JSON from {{{{text}}}} using schema {{{{schema}}}}'
markdown = 'Extract Markdown from {{{{text}}}}'
"#
        )
        .unwrap();

        let loader = FileTemplateLoader::new(file_path.to_str().unwrap());
        let templates = loader.load_templates().unwrap();
        assert!(templates.contains_key("json"));
        assert!(templates.contains_key("markdown"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_template_loader_read_templates_valid_toml() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("crawlrs_test_llm_loader2");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("test_prompts2.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(
            f,
            r#"
[extraction]
json = 'json template'
"#
        )
        .unwrap();

        let loader = FileTemplateLoader::new(file_path.to_str().unwrap());
        let templates = loader.load_templates().unwrap();
        assert_eq!(templates.get("json").unwrap(), "json template");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_template_loader_read_templates_invalid_toml_returns_empty() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("crawlrs_test_llm_loader3");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("invalid.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(f, "this is not valid toml {{{{").unwrap();

        let loader = FileTemplateLoader::new(file_path.to_str().unwrap());
        let templates = loader.load_templates().unwrap();
        assert!(templates.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_template_loader_read_templates_partial_toml_only_json() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("crawlrs_test_llm_loader4");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("partial.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(
            f,
            r#"
[extraction]
json = 'json only'
"#
        )
        .unwrap();

        let loader = FileTemplateLoader::new(file_path.to_str().unwrap());
        let templates = loader.load_templates().unwrap();
        assert_eq!(templates.len(), 1);
        assert!(templates.contains_key("json"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_template_loader_read_templates_no_extraction_key() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("crawlrs_test_llm_loader5");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("no_extraction.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(f, "[other]\nkey = \"value\"").unwrap();

        let loader = FileTemplateLoader::new(file_path.to_str().unwrap());
        let templates = loader.load_templates().unwrap();
        assert!(templates.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_template_loader_load_templates_returns_clone() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("crawlrs_test_llm_loader6");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("clone_test.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(f, "[extraction]\njson = 'test'").unwrap();

        let loader = FileTemplateLoader::new(file_path.to_str().unwrap());
        let mut t1 = loader.load_templates().unwrap();
        t1.insert("new".to_string(), "val".to_string());
        let t2 = loader.load_templates().unwrap();
        assert!(!t2.contains_key("new"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ===== extract_data_chunked（F8）=====

    /// trait 默认实现：mock 只实现 extract_data 时，extract_data_chunked 行为等价
    /// extract_data（mock 零改动编译通过）
    #[tokio::test]
    async fn test_extract_data_chunked_default_impl_delegates_to_extract_data() {
        struct ChunklessMock {
            calls: Arc<AtomicU32>,
        }

        #[async_trait]
        impl LLMServiceTrait for ChunklessMock {
            async fn extract_data(
                &self,
                text: &str,
                _schema: &Value,
                _format: &str,
            ) -> Result<(Value, TokenUsage)> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok((serde_json::json!({"echo": text}), TokenUsage::default()))
            }
        }

        let calls = Arc::new(AtomicU32::new(0));
        let mock = ChunklessMock {
            calls: calls.clone(),
        };
        let schema = serde_json::json!({"type": "object"});
        let (value, usage) = mock
            .extract_data_chunked("some text", &schema, "json")
            .await
            .expect("chunked call ok");
        assert_eq!(value, serde_json::json!({"echo": "some text"}));
        assert_eq!(usage, TokenUsage::default());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    // ===== extract_data_with_provenance（F9）=====

    /// trait 默认实现：mock 只实现 extract_data 时，溯源方法返回 None（mock 零破坏）
    #[tokio::test]
    async fn test_extract_data_with_provenance_default_impl_yields_none() {
        struct ProvenancelessMock {
            calls: Arc<AtomicU32>,
        }

        #[async_trait]
        impl LLMServiceTrait for ProvenancelessMock {
            async fn extract_data(
                &self,
                text: &str,
                _schema: &Value,
                _format: &str,
            ) -> Result<(Value, TokenUsage)> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok((serde_json::json!({"echo": text}), TokenUsage::default()))
            }
        }

        let calls = Arc::new(AtomicU32::new(0));
        let mock = ProvenancelessMock {
            calls: calls.clone(),
        };
        let schema = serde_json::json!({"type": "object"});
        let (value, provenance, usage) = mock
            .extract_data_with_provenance("some text", &schema, "json")
            .await
            .expect("provenance call ok");
        assert_eq!(value, serde_json::json!({"echo": "some text"}));
        assert!(provenance.is_none(), "默认实现溯源为 None");
        assert!(value.get("_provenance").is_none(), "不挂载 _provenance");
        assert_eq!(usage, TokenUsage::default());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// LLMService 构造：`[llm]` 的 `provenance_*` 两键映射进服务配置
    #[test]
    fn test_llm_service_provenance_config_from_settings() {
        let mut llm = llm_settings_default();
        llm.provenance_enabled = true;
        llm.provenance_min_coverage = 0.6;
        let settings = make_test_settings(llm);
        let service = LLMService::new_with_template_loader(
            &settings,
            Arc::new(reqwest::Client::new()),
            Arc::new(InMemoryTemplateLoader::new()),
        );
        assert!(service.provenance.enabled);
        assert!((service.provenance.min_coverage - 0.6).abs() < f64::EPSILON);
    }

    /// 显式配置构造（无 Settings）时溯源默认禁用（与 chunking 同惯例）
    #[test]
    fn test_llm_service_new_with_config_provenance_disabled() {
        let service = LLMService::new_with_config(
            "key".to_string(),
            "m".to_string(),
            "https://api.example.com".to_string(),
            Arc::new(reqwest::Client::new()),
        );
        assert!(!service.provenance.enabled);
    }

    // ===== LLMService constructor tests =====

    #[test]
    fn test_llm_service_new_with_config_creates_instance() {
        let _settings = make_test_settings(llm_settings_default());
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "test-key".to_string(),
            "gpt-4".to_string(),
            "https://api.openai.com/v1".to_string(),
            http_client,
        );
        assert_eq!(service.model, "gpt-4");
        assert_eq!(service.provider, "openai");
    }

    #[test]
    fn test_llm_service_new_with_config_and_loader_creates_instance() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config_and_loader(
            "test-key".to_string(),
            "gpt-4".to_string(),
            "https://api.openai.com/v1".to_string(),
            http_client,
            loader,
        );
        assert_eq!(service.model, "gpt-4");
    }

    #[test]
    fn test_llm_service_new_with_config_has_default_templates() {
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "test-key".to_string(),
            "gpt-4".to_string(),
            "https://api.openai.com/v1".to_string(),
            http_client,
        );
        assert!(service.templates.contains_key("json"));
        assert!(service.templates.contains_key("markdown"));
    }

    #[test]
    fn test_llm_service_clone_preserves_fields() {
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "key".to_string(),
            "model-x".to_string(),
            "https://api.example.com".to_string(),
            http_client,
        );
        let cloned = service.clone();
        assert_eq!(cloned.model, "model-x");
        assert_eq!(
            cloned.api_base_url,
            Some("https://api.example.com".to_string())
        );
    }

    // ===== Helper functions =====

    fn llm_settings_default() -> crate::config::settings::LLMSettings {
        crate::config::settings::LLMSettings {
            provider: Some("openai".to_string()),
            api_key: Some("test-key".to_string()),
            model: Some("gpt-3.5-turbo".to_string()),
            api_base_url: None,
            chunking: crate::config::settings::LlmChunkingSettings::default(),
            provenance_enabled: false,
            provenance_min_coverage: 0.75,
            request_timeout_secs: 120,
        }
    }

    fn make_test_settings(llm: crate::config::settings::LLMSettings) -> Settings {
        Settings {
            llm,
            ..Settings::default()
        }
    }

    #[test]
    fn test_new_with_template_loader_default_settings() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let settings = make_test_settings(llm_settings_default());
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        assert_eq!(service.provider, "openai");
        assert_eq!(service.model, "gpt-3.5-turbo");
    }

    #[test]
    fn test_new_with_template_loader_with_provider_and_model() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let mut llm = llm_settings_default();
        llm.provider = Some("anthropic".to_string());
        llm.model = Some("claude-2".to_string());
        let settings = make_test_settings(llm);
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        assert_eq!(service.provider, "anthropic");
        assert_eq!(service.model, "claude-2");
    }

    #[test]
    fn test_new_with_template_loader_ollama_port_forces_openai() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let mut llm = llm_settings_default();
        llm.provider = Some("ollama".to_string());
        llm.api_base_url = Some("http://localhost:11434".to_string());
        let settings = make_test_settings(llm);
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        // OllamaStrategy should resolve to "openai" for localhost:11434
        assert_eq!(service.provider, "openai");
    }

    #[test]
    fn test_new_with_template_loader_172_24_160_1_forces_openai() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let mut llm = llm_settings_default();
        llm.provider = Some("ollama".to_string());
        llm.api_base_url = Some("http://172.24.160.1:8080".to_string());
        let settings = make_test_settings(llm);
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        assert_eq!(service.provider, "openai");
    }

    #[test]
    fn test_new_with_template_loader_no_api_base_url_skips_ollama_check() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let mut llm = llm_settings_default();
        llm.provider = Some("ollama".to_string());
        llm.api_base_url = None;
        let settings = make_test_settings(llm);
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        // Without api_base_url, OllamaStrategy doesn't transform
        assert_eq!(service.provider, "ollama");
    }

    #[test]
    fn test_new_with_template_loader_failing_loader_uses_empty_templates() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let settings = make_test_settings(llm_settings_default());
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        // Loader returns empty templates, service should still construct
        assert!(service.templates.is_empty());
    }

    #[test]
    fn test_new_with_file_template_loader_default() {
        let http_client = Arc::new(reqwest::Client::new());
        let settings = make_test_settings(llm_settings_default());
        let service = LLMService::new(&settings, http_client);
        // FileTemplateLoader::default() tries to load from config/prompts.toml
        // which may or may not exist; service should still construct
        assert_eq!(service.model, "gpt-3.5-turbo");
    }
}
