// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! fluxweir 承载的 LLM carrier（变更 `fluxweir-llm-gateway` design D4）
//!
//! [`FluxweirLlmCarrier`] 经 fluxweir SDK（嵌入式回环或外部实例）发出提取
//! 请求：`prompt_builder` 构造 prompt → SDK `chat` → JSON 解析 → usage 映射。
//! 分块提取复用 [`chunker::extract_chunked`]，字符级溯源复用 `provenance`
//! 编排——与宿主 carrier 契约（`LLMServiceTrait`）完全一致。

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::domain::services::llm::chunker::{self, ChunkingConfig};
use crate::domain::services::llm::prompt_builder::{
    build_prompt, parse_llm_response, FileTemplateLoader, TemplateLoaderTrait,
};
use crate::domain::services::llm::provenance;
use crate::domain::services::llm::{LLMServiceTrait, TokenUsage};

/// fluxweir 承载的 LLM 服务实现（cfg(feature = "gateway")）
pub struct FluxweirLlmCarrier {
    client: Arc<fluxweir::sdk::FluxweirClient>,
    model: String,
    templates: Arc<HashMap<String, String>>,
    provenance_enabled: bool,
    provenance_min_coverage: f64,
    chunking: Option<ChunkingConfig>,
}

impl FluxweirLlmCarrier {
    /// 从 crawlrs 设置装配 carrier（prompt 模板经 `config/prompts.toml` 装载，
    /// 缺失模板在调用期显性报错而非静默回退）
    pub fn new(client: fluxweir::sdk::FluxweirClient, settings: &crate::config::Settings) -> Self {
        let templates = FileTemplateLoader::default_path()
            .load_templates()
            .unwrap_or_default();
        let chunking = if settings.llm.chunking.enabled {
            Some(ChunkingConfig::from_settings(&settings.llm.chunking))
        } else {
            None
        };
        Self {
            client: Arc::new(client),
            model: settings
                .llm
                .model
                .clone()
                .unwrap_or_else(|| "auto".to_string()),
            templates: Arc::new(templates),
            provenance_enabled: settings.llm.provenance_enabled,
            provenance_min_coverage: settings.llm.provenance_min_coverage,
            chunking,
        }
    }

    /// 单次提取：prompt 构造 → SDK chat → JSON 解析 → usage 映射
    async fn chat_extract(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, TokenUsage)> {
        let template = self
            .templates
            .get(format)
            .ok_or_else(|| {
                anyhow!(
                    "no prompt template registered for format \"{format}\" \
                     (loaded from config/prompts.toml)"
                )
            })?
            .clone();
        let prompt = build_prompt(&template, text, schema)?;
        let request = fluxweir::sdk::ChatRequest::new(&self.model, &prompt);
        let response = self
            .client
            .chat(&request)
            .await
            .map_err(|e| anyhow!("fluxweir chat request failed: {e}"))?;
        parse_chat_payload(
            &response.content,
            format,
            response.usage.prompt_tokens,
            response.usage.completion_tokens,
            response.usage.total_tokens,
        )
    }

    /// 分块提取：对单个块发 SDK 请求并解析结果
    /// （供 `extract_data_chunked` 的 'static 闭包转发）。
    async fn extract_one_chunk(
        client: &fluxweir::sdk::FluxweirClient,
        model: &str,
        templates: &HashMap<String, String>,
        schema_json: &str,
        format: &str,
        chunk_text: &str,
    ) -> Result<(Value, TokenUsage)> {
        let template = templates
            .get(format)
            .ok_or_else(|| anyhow!("no prompt template registered for format \"{format}\""))?;
        let prompt = build_prompt(
            template,
            chunk_text,
            &serde_json::from_str::<Value>(schema_json)?,
        )?;
        let response = client
            .chat(&fluxweir::sdk::ChatRequest::new(model, &prompt))
            .await
            .map_err(|e| anyhow!("fluxweir chat request failed: {e}"))?;
        parse_chat_payload(
            &response.content,
            format,
            response.usage.prompt_tokens,
            response.usage.completion_tokens,
            response.usage.total_tokens,
        )
    }

    /// 分块提取：合并多块结果
    /// （供 `extract_data_chunked` 的 'static 闭包转发）。
    async fn merge_chunk_results(
        client: &fluxweir::sdk::FluxweirClient,
        model: &str,
        schema_json: &str,
        format: &str,
        merge_json: &str,
    ) -> Result<(Value, TokenUsage)> {
        let prompt = merge_prompt(merge_json, schema_json);
        let response = client
            .chat(&fluxweir::sdk::ChatRequest::new(model, &prompt))
            .await
            .map_err(|e| anyhow!("fluxweir merge request failed: {e}"))?;
        parse_chat_payload(
            &response.content,
            format,
            response.usage.prompt_tokens,
            response.usage.completion_tokens,
            response.usage.total_tokens,
        )
    }
}

/// SDK 响应 → 提取结果与 usage 的纯映射（独立函数承载单测：
/// 成功 / 非 JSON / usage 缺失零值三分支）
fn parse_chat_payload(
    content: &str,
    format: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
) -> Result<(Value, TokenUsage)> {
    let value = parse_llm_response(content, format).map_err(|e| {
        // 解析失败携带截断响应片段（与日志脱敏惯例一致：500 字符上限）
        let snippet: String = content.chars().take(500).collect();
        anyhow!("llm response is not valid {format}: {e} — response snippet: {snippet}")
    })?;
    let usage = TokenUsage {
        prompt_tokens: u32::try_from(prompt_tokens).unwrap_or(u32::MAX),
        completion_tokens: u32::try_from(completion_tokens).unwrap_or(u32::MAX),
        total_tokens: u32::try_from(total_tokens).unwrap_or(u32::MAX),
    };
    Ok((value, usage))
}

/// 网关 merge 调用 prompt（分块提取的块结果合并；与单发路径共用模板装载）
fn merge_prompt(merge_json: &str, schema_json: &str) -> String {
    format!(
        "You are merging partial JSON extraction results. \
         Merge the following partial results into a single JSON object that is \
         consistent with the schema. Resolve conflicts by preferring the more \
         complete value. Respond with ONLY the merged JSON.\n\n\
         Schema:\n{schema_json}\n\nPartial results:\n{merge_json}"
    )
}

#[async_trait]
impl LLMServiceTrait for FluxweirLlmCarrier {
    async fn extract_data(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, TokenUsage), anyhow::Error> {
        self.chat_extract(text, schema, format).await
    }

    /// 分块提取：复用 [`chunker::extract_chunked`]（'static 闭包约束 →
    /// client/模板/模型全部 owned 克隆进闭包；usage 累加由 chunker 承担）
    async fn extract_data_chunked(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, TokenUsage), anyhow::Error> {
        let Some(cfg) = self.chunking.clone() else {
            return self.extract_data(text, schema, format).await;
        };
        let client = self.client.clone();
        let model = self.model.clone();
        let templates = self.templates.clone();
        let schema_json = serde_json::to_string(schema)?;
        let format_owned = format.to_string();

        let extract_client = client.clone();
        let extract_model = model.clone();
        let extract_templates = templates.clone();
        let extract_schema = schema_json.clone();
        let extract_format = format_owned.clone();
        let extract_one = move |chunk_text: String| {
            let client = extract_client.clone();
            let model = extract_model.clone();
            let templates = extract_templates.clone();
            let schema_json = extract_schema.clone();
            let format = extract_format.clone();
            async move {
                Self::extract_one_chunk(
                    &client,
                    &model,
                    &templates,
                    &schema_json,
                    &format,
                    &chunk_text,
                )
                .await
            }
        };

        let merge_client = client;
        let merge_model = model;
        let merge_schema = schema_json;
        let merge_format = format_owned;
        let merge_call = move |merge_json: String| {
            let client = merge_client.clone();
            let model = merge_model.clone();
            let schema_json = merge_schema.clone();
            let format = merge_format.clone();
            async move {
                Self::merge_chunk_results(&client, &model, &schema_json, &format, &merge_json).await
            }
        };

        chunker::extract_chunked(text, schema, &cfg, extract_one, merge_call).await
    }

    /// 提取 + 字符级溯源：复用 `provenance` 编排（`_provenance` 并列挂载，
    /// `llm.provenance_enabled = false` 时为 None——mock/宿主零改动契约）
    async fn extract_data_with_provenance(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, Option<Value>, TokenUsage), anyhow::Error> {
        let (mut value, usage) = self.extract_data(text, schema, format).await?;
        if !self.provenance_enabled {
            return Ok((value, None, usage));
        }
        let prov = provenance::provenance_map(text, &value, self.provenance_min_coverage);
        provenance::attach_provenance(&mut value, &prov);
        Ok((value, Some(prov), usage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合法 JSON 响应 → 解析结果与 usage 三字段正确映射
    #[test]
    fn parse_chat_payload_success_maps_usage() {
        let (value, usage) =
            parse_chat_payload(r#"{"name":"x"}"#, "json", 11, 7, 18).expect("parse ok");
        assert_eq!(value["name"], "x");
        assert_eq!(usage.prompt_tokens, 11);
        assert_eq!(usage.completion_tokens, 7);
        assert_eq!(usage.total_tokens, 18);
    }

    /// 非 JSON 响应 → 显性错误并携带截断片段（不 panic、不返回空对象）
    #[test]
    fn parse_chat_payload_non_json_errors_with_snippet() {
        let body = "not json at all ".repeat(80);
        let err = parse_chat_payload(&body, "json", 1, 1, 2).expect_err("must error");
        let rendered = format!("{err:#}");
        assert!(rendered.contains("not valid json"), "{rendered}");
        assert!(rendered.contains("response snippet"), "{rendered}");
    }

    /// usage 缺失（上游省略）→ 零值 usage 正常返回，不报错
    #[test]
    fn parse_chat_payload_zero_usage_is_legal() {
        let (value, usage) =
            parse_chat_payload(r#"{"name":"x"}"#, "json", 0, 0, 0).expect("parse ok");
        assert_eq!(value["name"], "x");
        assert_eq!(usage, TokenUsage::default());
    }

    /// 合并 prompt 携带 schema 与块结果（分块提取 merge 路径的构造面）
    #[test]
    fn merge_prompt_carries_schema_and_results() {
        let prompt = merge_prompt(r#"[{"a":1}]"#, r#"{"type":"object"}"#);
        assert!(prompt.contains(r#"{"type":"object"}"#));
        assert!(prompt.contains(r#"[{"a":1}]"#));
    }

    /// 超大 usage 计数饱和映射（u64 → u32 截断防护，不 panic）
    #[test]
    fn parse_chat_payload_saturates_huge_usage() {
        let (_, usage) = parse_chat_payload(r#"{}"#, "json", u64::MAX, 0, 0).expect("parse ok");
        assert_eq!(usage.prompt_tokens, u32::MAX);
    }
}
