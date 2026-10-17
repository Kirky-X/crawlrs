// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! LLM 边界与提取编排
//!
//! [`LLMServiceTrait`] 是 crawlrs 唯一的 LLM 边界：供应商访问由宿主注入的
//! carrier 承担（默认 [`ExplicitErrorCarrier`] 在未注入时显式报错）。
//! `prompt_builder`（模板 + prompt 构造）、`chunker`（token 窗口分块并行）、
//! `provenance`（字符级溯源）保留为编排工具，宿主 carrier 可复用。

pub mod chunker; // token 窗口分块 + 并行提问 + merge（F8）
pub mod prompt_builder;
pub mod provenance; // 提取结果字符级溯源对齐（F9）
#[cfg(feature = "engine-mllm")]
pub mod vision_adapter; // 视觉模型适配器（MLLM 引擎依赖，genai 仅经此面保留）

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// 宿主 carrier 可复用 [`chunker::extract_chunked`] 覆盖实现，
    /// `[llm.chunking].enabled = false` 时同样直走单发路径。
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

/// 默认装配的显式失败承载（未注入宿主 carrier 时）
///
/// crawlrs 不再内置 LLM 供应商访问栈；LLM 能力由宿主经装配面注入
/// [`LLMServiceTrait`] 实现（`Services.llm_service` / `init_llm_service` / kit 注册）。
/// 未注入时本承载把「LLM 不可用」变成可观测的结构化错误，而不是静默降级或 panic。
pub struct ExplicitErrorCarrier;

impl ExplicitErrorCarrier {
    /// 按 locale 取译文案；该 locale 未被启动束加载时用规范串，
    /// 避免 Fluent 回退到默认语言导致「双语」退化为单语。
    fn localized(
        bundle: &crate::i18n::I18nBundle,
        locale: &crate::i18n::Locale,
        key: &str,
        fallback: &str,
    ) -> String {
        if bundle.supported_locales().contains(locale) {
            crate::i18n::t(locale, bundle, key)
        } else {
            fallback.to_string()
        }
    }

    /// 构造结构化错误：anyhow 链 + i18n 双语载荷（en | zh）。
    ///
    /// 启动期 i18n 全局束已初始化时经 Fluent 输出双语；未初始化（单测、轻量面）
    /// 时回退同语义的英文/中文规范串（双轨基准，与 `CrawlRsError::user_message`
    /// 同惯例）。
    fn missing_carrier_error() -> anyhow::Error {
        const KEY: &str = "domain-error-llm-carrier-not-injected";
        const EN_FALLBACK: &str = "LLM carrier is not injected: crawlrs no longer bundles \
             LLM provider access; inject a host implementation of LLMServiceTrait";
        const ZH_FALLBACK: &str = "LLM 未接入承载方：crawlrs 不再内置 LLM 供应商访问，\
             请经宿主注入 LLMServiceTrait 实现";

        let (en, zh) = match crate::i18n::startup_i18n() {
            Some((_, bundle)) => {
                let en_locale = "en-US".parse::<crate::i18n::Locale>();
                let zh_locale = "zh-CN".parse::<crate::i18n::Locale>();
                match (en_locale, zh_locale) {
                    (Ok(en_locale), Ok(zh_locale)) => (
                        Self::localized(bundle, &en_locale, KEY, EN_FALLBACK),
                        Self::localized(bundle, &zh_locale, KEY, ZH_FALLBACK),
                    ),
                    _ => (EN_FALLBACK.to_string(), ZH_FALLBACK.to_string()),
                }
            }
            None => (EN_FALLBACK.to_string(), ZH_FALLBACK.to_string()),
        };
        anyhow::anyhow!("{en} | {zh}")
    }
}

#[async_trait]
impl LLMServiceTrait for ExplicitErrorCarrier {
    async fn extract_data(
        &self,
        _text: &str,
        _schema: &Value,
        _format: &str,
    ) -> Result<(Value, TokenUsage)> {
        Err(Self::missing_carrier_error())
    }

    /// 分块入口同样显式失败：未注入承载时不得静默退化为单发/空结果。
    async fn extract_data_chunked(
        &self,
        _text: &str,
        _schema: &Value,
        _format: &str,
    ) -> Result<(Value, TokenUsage)> {
        Err(Self::missing_carrier_error())
    }

    /// 溯源入口同样显式失败：未注入承载时不得返回空成功。
    async fn extract_data_with_provenance(
        &self,
        _text: &str,
        _schema: &Value,
        _format: &str,
    ) -> Result<(Value, Option<Value>, TokenUsage)> {
        Err(Self::missing_carrier_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prompt_builder::{
        FileTemplateLoader, InMemoryTemplateLoader, TemplateLoader, TemplateLoaderTrait,
    };
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    // ===== ExplicitErrorCarrier（默认装配：未注入承载）=====

    /// 默认装配态：`extract_data` 调用返回结构化错误，不 panic、不返回空成功。
    /// 错误载荷为 i18n 双语（en/zh 均含「未接入承载」语义）。
    #[tokio::test]
    async fn test_explicit_error_carrier_extract_data_reports_missing_carrier() {
        let carrier = ExplicitErrorCarrier;
        let schema = serde_json::json!({"type": "object"});
        let err = carrier
            .extract_data("text", &schema, "json")
            .await
            .expect_err("未注入承载时必须显式失败");
        let message = format!("{err:#}");
        assert!(
            message.contains("carrier is not injected"),
            "en payload missing: {message}"
        );
        assert!(
            message.contains("未接入承载方"),
            "zh payload missing: {message}"
        );
    }

    /// 默认装配态：分块入口同样显式失败（不静默降级为单发/空结果）。
    #[tokio::test]
    async fn test_explicit_error_carrier_chunked_reports_missing_carrier() {
        let carrier = ExplicitErrorCarrier;
        let schema = serde_json::json!({"type": "object"});
        let err = carrier
            .extract_data_chunked("text", &schema, "json")
            .await
            .expect_err("未注入承载时分块入口必须显式失败");
        let message = format!("{err:#}");
        assert!(
            message.contains("carrier is not injected"),
            "en payload missing: {message}"
        );
        assert!(
            message.contains("未接入承载方"),
            "zh payload missing: {message}"
        );
    }

    /// 默认装配态：溯源入口同样显式失败（不返回空成功）。
    #[tokio::test]
    async fn test_explicit_error_carrier_provenance_reports_missing_carrier() {
        let carrier = ExplicitErrorCarrier;
        let schema = serde_json::json!({"type": "object"});
        let err = carrier
            .extract_data_with_provenance("text", &schema, "json")
            .await
            .expect_err("未注入承载时溯源入口必须显式失败");
        let message = format!("{err:#}");
        assert!(
            message.contains("carrier is not injected"),
            "en payload missing: {message}"
        );
        assert!(
            message.contains("未接入承载方"),
            "zh payload missing: {message}"
        );
    }

    /// 启动期 i18n 束就绪时双语载荷来自 FTL（en/zh 各自命中语义键）。
    #[test]
    fn test_explicit_error_carrier_bilingual_payload_from_bundle() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/locales");
        let bundle = crate::i18n::I18nBundle::load("en-US", &["en-US", "zh-CN"], dir)
            .expect("locales bundle should load");
        let en_locale = "en-US".parse::<crate::i18n::Locale>().expect("en locale");
        let zh_locale = "zh-CN".parse::<crate::i18n::Locale>().expect("zh locale");
        const KEY: &str = "domain-error-llm-carrier-not-injected";

        let en = ExplicitErrorCarrier::localized(&bundle, &en_locale, KEY, "fallback");
        let zh = ExplicitErrorCarrier::localized(&bundle, &zh_locale, KEY, "fallback");
        assert!(en.contains("carrier is not injected"), "en payload: {en}");
        assert!(zh.contains("未接入承载方"), "zh payload: {zh}");
    }

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

    /// 注入面：宿主 carrier 经 `Arc<dyn LLMServiceTrait>` 装配后，
    /// `ExtractionService::extract_with_schema` 全链路（清洗 → carrier → 溯源挂载）工作。
    ///
    /// 模拟宿主经 `Services.llm_service` pub 字段注入自有实现；编排工具
    /// （[`provenance::apply_provenance`]）由 carrier 复用，证明边界开放且编排保留。
    #[tokio::test]
    async fn test_injected_carrier_full_provenance_chain_works() {
        use crate::domain::services::extraction_service::{
            ExtractionService, ExtractionServiceTrait,
        };

        struct MockCarrier {
            calls: Arc<AtomicU32>,
        }

        #[async_trait]
        impl LLMServiceTrait for MockCarrier {
            async fn extract_data(
                &self,
                _text: &str,
                _schema: &Value,
                _format: &str,
            ) -> Result<(Value, TokenUsage)> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok((
                    serde_json::json!({"name": "crawlrs"}),
                    TokenUsage::default(),
                ))
            }

            /// 宿主 carrier 复用 crawlrs 公开的溯源编排工具（F9）
            async fn extract_data_with_provenance(
                &self,
                text: &str,
                schema: &Value,
                format: &str,
            ) -> Result<(Value, Option<Value>, TokenUsage)> {
                let (value, usage) = self.extract_data(text, schema, format).await?;
                let config = provenance::ProvenanceConfig {
                    enabled: true,
                    min_coverage: 0.0,
                };
                let (value, prov) = provenance::apply_provenance(text, value, &config);
                Ok((value, prov, usage))
            }
        }

        let calls = Arc::new(AtomicU32::new(0));
        // 等价于宿主向 `Services.llm_service`（`Arc<dyn LLMServiceTrait>`）注入 carrier
        let carrier: Arc<dyn LLMServiceTrait> = Arc::new(MockCarrier {
            calls: calls.clone(),
        });
        let service = ExtractionService::new(carrier);
        let schema = serde_json::json!({"type": "object"});
        let html = "<html><body><p>crawlrs is a scraper</p></body></html>";

        let (result, _usage) = service
            .extract_with_schema(html, &schema)
            .await
            .expect("注入 carrier 后提取链路应成功");

        assert_eq!(result["name"], "crawlrs");
        assert_eq!(
            result["_provenance"]["name"]["start"], 0,
            "溯源映射应由 carrier 复用编排工具生成并挂载: {result}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// 溯源配置映射：`[llm]` 的 `provenance_*` 两键直接映射进编排配置
    /// （不依赖任何供应商栈）
    #[test]
    fn test_provenance_config_from_settings() {
        let mut llm = crate::config::settings::LLMSettings::default();
        llm.provenance_enabled = true;
        llm.provenance_min_coverage = 0.6;
        let config = provenance::ProvenanceConfig::from_settings(&llm);
        assert!(config.enabled);
        assert!((config.min_coverage - 0.6).abs() < f64::EPSILON);
    }

    /// 模板加载器默认构造可用（prompt_builder 保留为编排工具）
    #[test]
    fn test_in_memory_template_loader_available() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        assert!(loader.load_templates().unwrap().is_empty());
    }
}
