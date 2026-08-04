// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Prompt Builder — 模板加载与 prompt 构造逻辑。
//!
//! 提供 `TemplateLoaderTrait` 及两种实现：
//! - `FileTemplateLoader`：从 TOML 文件加载模板（生产用）
//! - `InMemoryTemplateLoader`：内存模板（测试用）
//!
//! `build_prompt` 函数负责将模板与文本/schema 插值。

use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Trait for template loading - enables DI and testing
pub trait TemplateLoaderTrait: Send + Sync {
    /// Load all templates
    fn load_templates(&self) -> Result<HashMap<String, String>>;
}

/// Template loader wrapped in Arc for Clone support
pub type TemplateLoader = Arc<dyn TemplateLoaderTrait>;

/// File-based template loader (production implementation)
pub struct FileTemplateLoader {
    templates: HashMap<String, String>,
}

impl FileTemplateLoader {
    pub fn new(file_path: impl Into<String>) -> Self {
        let path = file_path.into();
        let templates = Self::read_templates(&path).unwrap_or_else(|e| {
            log::error!("Failed to load templates from {}: {}", path, e);
            HashMap::new()
        });

        Self { templates }
    }

    /// Create with default path
    pub fn default_path() -> Self {
        Self::new("config/prompts.toml")
    }

    fn read_templates(file_path: &str) -> Result<HashMap<String, String>> {
        let content = std::fs::read_to_string(file_path)?;
        let v: Value = toml::from_str(&content)?;

        let mut templates = HashMap::new();
        if let Some(extraction) = v.get("extraction") {
            if let Some(json_tpl) = extraction.get("json").and_then(|t| t.as_str()) {
                templates.insert("json".to_string(), json_tpl.to_string());
            }
            if let Some(md_tpl) = extraction.get("markdown").and_then(|t| t.as_str()) {
                templates.insert("markdown".to_string(), md_tpl.to_string());
            }
        }
        Ok(templates)
    }
}

impl Default for FileTemplateLoader {
    fn default() -> Self {
        Self::default_path()
    }
}

impl TemplateLoaderTrait for FileTemplateLoader {
    fn load_templates(&self) -> Result<HashMap<String, String>> {
        Ok(self.templates.clone())
    }
}

/// In-memory template loader (for testing)
#[derive(Debug, Default, Clone)]
pub struct InMemoryTemplateLoader {
    templates: HashMap<String, String>,
}

impl InMemoryTemplateLoader {
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    pub fn with_template(mut self, name: impl Into<String>, content: impl Into<String>) -> Self {
        self.templates.insert(name.into(), content.into());
        self
    }

    /// Add default extraction templates
    pub fn with_default_templates(mut self) -> Self {
        self.templates.insert(
            "json".to_string(),
            "Extract JSON from {{text}} using schema {{schema}}".to_string(),
        );
        self.templates.insert(
            "markdown".to_string(),
            "Extract Markdown from {{text}}".to_string(),
        );
        self
    }
}

impl TemplateLoaderTrait for InMemoryTemplateLoader {
    fn load_templates(&self) -> Result<HashMap<String, String>> {
        Ok(self.templates.clone())
    }
}

/// 从模板和输入构建 prompt 字符串。
///
/// # Arguments
///
/// * `template` - 包含 `{{text}}` 和 `{{schema}}` 占位符的模板字符串
/// * `text` - 待提取的文本内容
/// * `schema` - JSON schema（会被 pretty-print 后插值）
///
/// # Errors
///
/// 当 `schema` 序列化失败时返回错误
pub fn build_prompt(template: &str, text: &str, schema: &Value) -> Result<String> {
    Ok(template
        .replace("{{text}}", text)
        .replace("{{schema}}", &serde_json::to_string_pretty(schema)?))
}

/// 解析 LLM 返回的 content，根据 format 决定是否反序列化。
///
/// # Arguments
///
/// * `content` - LLM 返回的原始文本
/// * `format` - 目标格式（`"json"` 时会 trim markdown 代码块并解析）
///
/// # Errors
///
/// 当 `format == "json"` 且 content 不是有效 JSON 时返回错误
pub fn parse_llm_response(content: &str, format: &str) -> Result<Value> {
    if format == "json" {
        let clean_content = content
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let data = serde_json::from_str::<Value>(clean_content).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse LLM JSON response: {}: {}",
                clean_content,
                e
            )
        })?;
        Ok(data)
    } else {
        Ok(serde_json::json!({ "content": content }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_in_memory_template_loader_new_empty() {
        let loader = InMemoryTemplateLoader::new();
        let templates = loader.load_templates().unwrap();
        assert!(templates.is_empty());
    }

    #[test]
    fn test_in_memory_template_loader_with_template_adds_entry() {
        let loader = InMemoryTemplateLoader::new().with_template("test", "Hello {{name}}");
        let templates = loader.load_templates().unwrap();
        assert_eq!(templates.get("test").unwrap(), "Hello {{name}}");
    }

    #[test]
    fn test_in_memory_template_loader_with_default_templates_adds_two() {
        let loader = InMemoryTemplateLoader::new().with_default_templates();
        let templates = loader.load_templates().unwrap();
        assert_eq!(templates.len(), 2);
        assert!(templates.contains_key("json"));
        assert!(templates.contains_key("markdown"));
    }

    #[test]
    fn test_in_memory_template_loader_load_templates_returns_clone() {
        let loader = InMemoryTemplateLoader::new().with_template("a", "1");
        let mut t1 = loader.load_templates().unwrap();
        t1.insert("b".to_string(), "2".to_string());
        let t2 = loader.load_templates().unwrap();
        assert!(!t2.contains_key("b"));
    }

    #[test]
    fn test_in_memory_template_loader_default_empty() {
        let loader = InMemoryTemplateLoader::default();
        assert!(loader.load_templates().unwrap().is_empty());
    }

    #[test]
    fn test_in_memory_template_loader_builder_chain() {
        let loader = InMemoryTemplateLoader::new()
            .with_template("a", "1")
            .with_template("b", "2");
        let templates = loader.load_templates().unwrap();
        assert_eq!(templates.len(), 2);
        assert_eq!(templates.get("a").unwrap(), "1");
        assert_eq!(templates.get("b").unwrap(), "2");
    }

    #[test]
    fn test_file_template_loader_new_invalid_path_returns_empty() {
        let loader = FileTemplateLoader::new("/nonexistent/path.toml");
        assert!(loader.load_templates().unwrap().is_empty());
    }

    #[test]
    fn test_file_template_loader_default() {
        let loader = FileTemplateLoader::default();
        // 默认路径可能不存在，不应 panic
        let _ = loader.load_templates();
    }

    #[test]
    fn test_file_template_loader_default_path_does_not_panic() {
        let _ = FileTemplateLoader::default_path();
    }

    #[test]
    fn test_build_prompt_substitutes_placeholders() {
        let template = "Extract {{text}} with schema {{schema}}";
        let schema = json!({"type": "object"});
        let result = build_prompt(template, "hello", &schema).unwrap();
        assert!(result.contains("hello"));
        assert!(result.contains("\"type\": \"object\""));
        assert!(!result.contains("{{text}}"));
        assert!(!result.contains("{{schema}}"));
    }

    #[test]
    fn test_parse_llm_response_json_format() {
        let content = "```json\n{\"key\": \"value\"}\n```";
        let result = parse_llm_response(content, "json").unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_parse_llm_response_json_format_no_code_block() {
        let content = "{\"key\": \"value\"}";
        let result = parse_llm_response(content, "json").unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_parse_llm_response_markdown_format() {
        let content = "# Hello World";
        let result = parse_llm_response(content, "markdown").unwrap();
        assert_eq!(result["content"], "# Hello World");
    }

    #[test]
    fn test_parse_llm_response_json_invalid() {
        let content = "not json at all";
        let result = parse_llm_response(content, "json");
        assert!(result.is_err());
    }
}
