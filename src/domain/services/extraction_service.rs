//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::services::extraction_utils::ExtractableRule;
pub use crate::domain::services::llm_service::TokenUsage;
use crate::domain::services::llm_service::LLMServiceTrait;
use anyhow::Result;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use url::Url;

/// 提取规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionRule {
    pub selector: Option<String>, // Make selector optional for LLM extraction
    pub attr: Option<String>,     // If None, extract text
    pub is_array: bool,
    pub use_llm: Option<bool>,         // New field to enable LLM extraction
    pub llm_prompt: Option<String>,    // Optional specific prompt for this rule
    pub output_format: Option<String>, // "json" (default) or "plaintext"
}

impl ExtractableRule for ExtractionRule {
    fn selector(&self) -> &str {
        self.selector.as_deref().unwrap_or("")
    }

    fn is_array(&self) -> bool {
        self.is_array
    }

    fn attr(&self) -> Option<&str> {
        self.attr.as_deref()
    }

    fn description(&self) -> &str {
        self.llm_prompt.as_deref().unwrap_or("")
    }
}

/// 提取服务
///
/// 负责从 HTML 内容中提取结构化数据
pub struct ExtractionService {
    llm_service: Box<dyn LLMServiceTrait>,
}

impl ExtractionService {
    pub fn new(llm_service: Box<dyn LLMServiceTrait>) -> Self {
        Self { llm_service }
    }

    /// 使用CSS选择器提取数据（无需Settings）
    ///
    /// 仅使用CSS选择器进行提取，不需要数据库、Redis或LLM配置
    pub fn extract_with_selectors(
        html_content: &str,
        rules: &HashMap<String, ExtractionRule>,
        base_url: Option<&str>,
    ) -> Result<Value> {
        let mut result = HashMap::with_capacity(rules.len());
        let base = base_url.and_then(|u| url::Url::parse(u).ok());

        for (key, rule) in rules {
            // Skip LLM-only rules
            if rule.use_llm.unwrap_or(false) {
                continue;
            }

            if let Some(selector_str) = &rule.selector {
                if let Ok(selector) = Selector::parse(selector_str) {
                    let document = Html::parse_document(html_content);

                    if rule.is_array {
                        // Array extraction - 使用辅助方法优化
                        let mut values = Vec::new();
                        for element in document.select(&selector) {
                            if let Some(v) =
                                Self::extract_element_value(element, &rule.attr, base.as_ref())
                                    .filter(|v| !v.is_empty())
                            {
                                values.push(Value::String(v));
                            }
                        }
                        result.insert(key.clone(), Value::Array(values));
                    } else {
                        // Single element extraction - 使用辅助方法优化
                        if let Some(element) = document.select(&selector).next() {
                            let value =
                                Self::extract_element_value(element, &rule.attr, base.as_ref());
                            result.insert(
                                key.clone(),
                                value.map(Value::String).unwrap_or(Value::Null),
                            );
                        } else {
                            result.insert(key.clone(), Value::Null);
                        }
                    }
                }
            }
        }

        Ok(json!(result))
    }

    /// 提取单个元素的属性值 - 消除深层嵌套
    fn extract_element_value(
        element: ElementRef,
        attr: &Option<String>,
        base: Option<&Url>,
    ) -> Option<String> {
        // Guard: 如果有属性，尝试提取属性值
        if let Some(attr_name) = attr {
            let val = element.value().attr(attr_name).map(|s| s.to_string());

            // Guard: 如果有base URL且属性是链接类型，解析相对路径
            if let (Some(v), Some(b)) = (&val, base) {
                if attr_name == "href" || attr_name == "src" {
                    return b.join(v).map(|u| u.to_string()).ok().or(val);
                }
                return val;
            }

            return val;
        }

        // 默认：提取元素文本内容
        Some(
            element
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string(),
        )
    }

    /// 提取数据（完整版本，需要Settings用于LLM）- 通过依赖注入
    ///
    /// # Arguments
    ///
    /// * `html_content` - HTML 内容
    /// * `rules` - 提取规则
    /// * `llm_service` - LLM 服务（通过依赖注入）
    /// * `base_url` - 基础 URL
    pub async fn extract(
        html_content: &str,
        rules: &HashMap<String, ExtractionRule>,
        llm_service: Box<dyn LLMServiceTrait>,
        base_url: Option<&str>,
    ) -> Result<(Value, TokenUsage)> {
        let service = Self::new(llm_service);
        service.extract_data(html_content, rules, base_url).await
    }

    /// 使用全局 Schema 直接通过 LLM 提取数据 - 通过依赖注入
    ///
    /// # Arguments
    ///
    /// * `html_content` - HTML 内容
    /// * `schema` - 数据 schema
    /// * `llm_service` - LLM 服务（通过依赖注入）
    pub async fn extract_with_schema(
        html_content: &str,
        schema: &Value,
        llm_service: Box<dyn LLMServiceTrait>,
    ) -> Result<(Value, TokenUsage)> {
        // 1. Noise removal
        let clean_text = Self::get_clean_text(html_content);

        // 2. LLM Interaction
        llm_service.extract_data(&clean_text, schema, "json").await
    }

    pub async fn extract_data(
        &self,
        html_content: &str,
        rules: &HashMap<String, ExtractionRule>,
        base_url: Option<&str>,
    ) -> Result<(Value, TokenUsage)> {
        let mut result = HashMap::with_capacity(rules.len());
        let mut total_usage = TokenUsage::default();
        let base = base_url.and_then(|u| url::Url::parse(u).ok());

        for (key, rule) in rules {
            if rule.use_llm.unwrap_or(false) {
                // LLM Extraction Flow
                let format = rule.output_format.as_deref().unwrap_or("json");

                let prompt = rule
                    .llm_prompt
                    .clone()
                    .unwrap_or_else(|| format!("Extract {} from the text", key));
                let schema = json!({ "type": if rule.is_array { "array" } else { "string" }, "description": prompt });

                // 1. Noise removal via CSS or selector
                let content_to_process = if let Some(sel) = &rule.selector {
                    let document = Html::parse_document(html_content);
                    if let Ok(selector) = Selector::parse(sel) {
                        document
                            .select(&selector)
                            .map(|e| Self::get_clean_text(&e.html()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        Self::get_clean_text(html_content)
                    }
                } else {
                    Self::get_clean_text(html_content)
                };

                // 2. LLM Interaction
                match self
                    .llm_service
                    .extract_data(&content_to_process, &schema, format)
                    .await
                {
                    Ok((val, usage)) => {
                        result.insert(key.clone(), val);
                        total_usage.prompt_tokens += usage.prompt_tokens;
                        total_usage.completion_tokens += usage.completion_tokens;
                        total_usage.total_tokens += usage.total_tokens;
                    }
                    Err(e) => {
                        println!("LLM extraction failed for key '{}': {}", key, e);
                        tracing::error!("LLM extraction failed for key '{}': {}", key, e);
                        result.insert(key.clone(), Value::Null);
                    }
                }
                continue;
            }

            // Traditional CSS Selector Extraction - CONSOLIDATED using ExtractionUtils
            if let Some(selector_str) = &rule.selector {
                if let Ok(selector) = Selector::parse(selector_str) {
                    let document = Html::parse_document(html_content);

                    if rule.is_array {
                        // Array extraction - CONSOLIDATED (previously 31 duplicate lines)
                        let mut values = Vec::new();
                        for element in document.select(&selector) {
                            let value = if let Some(attr) = &rule.attr {
                                let val = element.value().attr(attr).map(|s| s.to_string());
                                if let (Some(v), Some(b)) = (&val, &base) {
                                    if attr == "href" || attr == "src" {
                                        b.join(v).map(|u| u.to_string()).ok().or(val)
                                    } else {
                                        val
                                    }
                                } else {
                                    val
                                }
                            } else {
                                Some(
                                    element
                                        .text()
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                        .trim()
                                        .to_string(),
                                )
                            };

                            if let Some(v) = value.filter(|v| !v.is_empty()) {
                                values.push(Value::String(v));
                            }
                        }
                        result.insert(key.clone(), Value::Array(values));
                    } else {
                        // Single element extraction - CONSOLIDATED (previously 30 duplicate lines)
                        if let Some(element) = document.select(&selector).next() {
                            let value = if let Some(attr) = &rule.attr {
                                let val = element.value().attr(attr).map(|s| s.to_string());
                                if let (Some(v), Some(b)) = (&val, &base) {
                                    if attr == "href" || attr == "src" {
                                        b.join(v).map(|u| u.to_string()).ok().or(val)
                                    } else {
                                        val
                                    }
                                } else {
                                    val
                                }
                            } else {
                                Some(
                                    element
                                        .text()
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                        .trim()
                                        .to_string(),
                                )
                            };

                            result.insert(
                                key.clone(),
                                value.map(Value::String).unwrap_or(Value::Null),
                            );
                        } else {
                            result.insert(key.clone(), Value::Null);
                        }
                    }
                }
            }
        }

        Ok((json!(result), total_usage))
    }

    /// 获取干净的文本内容（去除 script, style 等噪音）
    pub fn get_clean_text(html: &str) -> String {
        let document = Html::parse_document(html);
        let mut text_parts = Vec::new();

        // 递归获取文本，从根元素开始
        Self::collect_clean_text(document.root_element(), &mut text_parts);

        text_parts
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn collect_clean_text(node: scraper::ElementRef, text_parts: &mut Vec<String>) {
        let name = node.value().name();
        // 过滤常见的噪音标签
        if name == "script"
            || name == "style"
            || name == "nav"
            || name == "footer"
            || name == "head"
            || name == "iframe"
            || name == "noscript"
            || name == "aside"
            || name == "form"
        {
            return;
        }

        for child in node.children() {
            if let Some(text) = child.value().as_text() {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    // 额外检查：如果文本块内包含典型的代码模式，则丢弃
                    let lower = trimmed.to_lowercase();
                    if !((lower.contains("var ")
                        || lower.contains("function(")
                        || lower.contains("window."))
                        && (trimmed.contains(';') || trimmed.contains('{')))
                    {
                        text_parts.push(trimmed.to_string());
                    }
                }
            } else if let Some(element) = scraper::ElementRef::wrap(child) {
                Self::collect_clean_text(element, text_parts);
            }
        }
    }
}

// Implement Validate for ExtractionRule to enable DTO validation
use validator::Validate;

impl Validate for ExtractionRule {
    fn validate(&self) -> Result<(), validator::ValidationErrors> {
        // No custom validation needed - all fields are optional or have defaults
        // The struct is valid by default
        Ok(())
    }
}
