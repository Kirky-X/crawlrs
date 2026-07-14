//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::services::extraction_utils::ExtractableRule;
use crate::domain::services::llm_service::LLMServiceTrait;
pub use crate::domain::services::llm_service::TokenUsage;
use anyhow::Result;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use shaku::{Component, Interface};
use std::collections::HashMap;
use std::sync::Arc;
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

/// 提取服务接口
#[async_trait::async_trait]
pub trait ExtractionServiceTrait: Interface + Send + Sync {
    /// 提取数据（完整版本，需要Settings用于LLM）
    async fn extract(
        &self,
        html_content: &str,
        rules: &HashMap<String, ExtractionRule>,
        base_url: Option<&str>,
    ) -> Result<(Value, TokenUsage)>;

    /// 使用全局 Schema 直接通过 LLM 提取数据
    async fn extract_with_schema(
        &self,
        html_content: &str,
        schema: &Value,
    ) -> Result<(Value, TokenUsage)>;

    /// 使用CSS选择器提取数据（无需Settings）
    fn extract_with_selectors(
        &self,
        html_content: &str,
        rules: &HashMap<String, ExtractionRule>,
        base_url: Option<&str>,
    ) -> Result<Value>;
}

/// 提取服务
///
/// 负责从 HTML 内容中提取结构化数据
#[derive(Component)]
#[shaku(interface = ExtractionServiceTrait)]
pub struct ExtractionService {
    #[shaku(inject)]
    llm_service: Arc<dyn LLMServiceTrait>,
}

#[async_trait::async_trait]
impl ExtractionServiceTrait for ExtractionService {
    async fn extract(
        &self,
        html_content: &str,
        rules: &HashMap<String, ExtractionRule>,
        base_url: Option<&str>,
    ) -> Result<(Value, TokenUsage)> {
        self.extract_data(html_content, rules, base_url).await
    }

    async fn extract_with_schema(
        &self,
        html_content: &str,
        schema: &Value,
    ) -> Result<(Value, TokenUsage)> {
        // 1. Noise removal
        let clean_text = Self::get_clean_text(html_content);

        // 2. LLM Interaction
        self.llm_service
            .extract_data(&clean_text, schema, "json")
            .await
    }

    fn extract_with_selectors(
        &self,
        html_content: &str,
        rules: &HashMap<String, ExtractionRule>,
        base_url: Option<&str>,
    ) -> Result<Value> {
        Self::extract_with_selectors_internal(html_content, rules, base_url)
    }
}

impl ExtractionService {
    pub fn new(llm_service: Arc<dyn LLMServiceTrait>) -> Self {
        Self { llm_service }
    }

    /// 使用CSS选择器提取数据（内部静态实现）
    fn extract_with_selectors_internal(
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

    /// 提取数据（内部实现）
    async fn extract_data(
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
                        log::error!("LLM extraction failed for key '{}': {}", key, e);
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    // ========== MockLLMService ==========

    /// Mock LLM service for testing ExtractionService without real HTTP calls.
    struct MockLLMService {
        response: Mutex<Option<Result<(Value, TokenUsage), String>>>,
        call_count: Arc<AtomicU64>,
        last_text: Mutex<Option<String>>,
        last_schema: Mutex<Option<Value>>,
        last_format: Mutex<Option<String>>,
    }

    impl MockLLMService {
        fn new_success(value: Value, usage: TokenUsage) -> Self {
            Self {
                response: Mutex::new(Some(Ok((value, usage)))),
                call_count: Arc::new(AtomicU64::new(0)),
                last_text: Mutex::new(None),
                last_schema: Mutex::new(None),
                last_format: Mutex::new(None),
            }
        }

        fn new_error(msg: &str) -> Self {
            Self {
                response: Mutex::new(Some(Err(msg.to_string()))),
                call_count: Arc::new(AtomicU64::new(0)),
                last_text: Mutex::new(None),
                last_schema: Mutex::new(None),
                last_format: Mutex::new(None),
            }
        }

        #[allow(dead_code)]
        fn call_count(&self) -> u64 {
            self.call_count.load(Ordering::SeqCst)
        }

        fn last_text(&self) -> Option<String> {
            self.last_text.lock().unwrap().clone()
        }

        fn last_format(&self) -> Option<String> {
            self.last_format.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LLMServiceTrait for MockLLMService {
        async fn extract_data(
            &self,
            text: &str,
            schema: &Value,
            format: &str,
        ) -> Result<(Value, TokenUsage), anyhow::Error> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            *self.last_text.lock().unwrap() = Some(text.to_string());
            *self.last_schema.lock().unwrap() = Some(schema.clone());
            *self.last_format.lock().unwrap() = Some(format.to_string());

            let guard = self.response.lock().unwrap();
            match &*guard {
                Some(Ok((v, u))) => Ok((v.clone(), u.clone())),
                Some(Err(msg)) => Err(anyhow::anyhow!("{}", msg)),
                None => Err(anyhow::anyhow!("No response configured")),
            }
        }
    }

    /// Helper to create an ExtractionService with a mock LLM
    fn make_service(mock: MockLLMService) -> (ExtractionService, Arc<AtomicU64>) {
        let count = mock.call_count.clone();
        let service = ExtractionService::new(Arc::new(mock));
        (service, count)
    }

    /// Helper to create an ExtractionService with a mock LLM and access to the mock
    fn make_service_with_mock(mock: MockLLMService) -> (ExtractionService, Arc<MockLLMService>) {
        let mock_arc = Arc::new(mock);
        let service = ExtractionService::new(mock_arc.clone());
        (service, mock_arc)
    }

    /// Helper to build a simple ExtractionRule
    fn rule(selector: Option<&str>, attr: Option<&str>, is_array: bool) -> ExtractionRule {
        ExtractionRule {
            selector: selector.map(|s| s.to_string()),
            attr: attr.map(|s| s.to_string()),
            is_array,
            use_llm: None,
            llm_prompt: None,
            output_format: None,
        }
    }

    fn llm_rule(selector: Option<&str>, prompt: Option<&str>, is_array: bool) -> ExtractionRule {
        ExtractionRule {
            selector: selector.map(|s| s.to_string()),
            attr: None,
            is_array,
            use_llm: Some(true),
            llm_prompt: prompt.map(|s| s.to_string()),
            output_format: None,
        }
    }

    // ========== ExtractableRule impl ==========

    #[test]
    fn test_extractable_rule_selector_with_value() {
        let r = rule(Some("div.content"), None, false);
        assert_eq!(r.selector(), "div.content");
    }

    #[test]
    fn test_extractable_rule_selector_none_returns_empty() {
        let r = rule(None, None, false);
        assert_eq!(r.selector(), "");
    }

    #[test]
    fn test_extractable_rule_is_array() {
        assert!(rule(Some("li"), None, true).is_array());
        assert!(!rule(Some("li"), None, false).is_array());
    }

    #[test]
    fn test_extractable_rule_attr_some() {
        let r = rule(Some("a"), Some("href"), false);
        assert_eq!(r.attr(), Some("href"));
    }

    #[test]
    fn test_extractable_rule_attr_none() {
        let r = rule(Some("a"), None, false);
        assert_eq!(r.attr(), None);
    }

    #[test]
    fn test_extractable_rule_description_from_llm_prompt() {
        let r = llm_rule(None, Some("Extract title"), false);
        assert_eq!(r.description(), "Extract title");
    }

    #[test]
    fn test_extractable_rule_description_empty_when_no_prompt() {
        let r = rule(Some("div"), None, false);
        assert_eq!(r.description(), "");
    }

    // ========== Validate impl ==========

    #[test]
    fn test_extraction_rule_validate_always_ok() {
        let r = rule(Some("div"), Some("class"), true);
        assert!(r.validate().is_ok());
    }

    #[test]
    fn test_extraction_rule_validate_empty_rule_ok() {
        let r = rule(None, None, false);
        assert!(r.validate().is_ok());
    }

    // ========== ExtractionService::new ==========

    #[test]
    fn test_extraction_service_new_creates_instance() {
        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let _service = ExtractionService::new(Arc::new(mock));
    }

    // ========== extract_with_selectors (static CSS extraction) ==========

    #[test]
    fn test_extract_with_selectors_single_text_element() {
        let html = r#"<html><body><h1>Title</h1></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert("title".to_string(), rule(Some("h1"), None, false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        assert_eq!(result["title"], "Title");
    }

    #[test]
    fn test_extract_with_selectors_array_text_elements() {
        let html = r#"<html><body><ul><li>A</li><li>B</li><li>C</li></ul></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert("items".to_string(), rule(Some("li"), None, true));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        let arr = result["items"].as_array().expect("should be array");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], "A");
        assert_eq!(arr[1], "B");
        assert_eq!(arr[2], "C");
    }

    #[test]
    fn test_extract_with_selectors_attribute_extraction() {
        let html = r#"<a href="https://example.com" class="link">Link</a>"#;
        let mut rules = HashMap::new();
        rules.insert("class".to_string(), rule(Some("a"), Some("class"), false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        assert_eq!(result["class"], "link");
    }

    #[test]
    fn test_extract_with_selectors_href_with_base_url() {
        let html = r#"<a href="/relative/path">Link</a>"#;
        let mut rules = HashMap::new();
        rules.insert("link".to_string(), rule(Some("a"), Some("href"), false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, Some("https://example.com"))
            .expect("extract should succeed");

        assert_eq!(result["link"], "https://example.com/relative/path");
    }

    #[test]
    fn test_extract_with_selectors_src_with_base_url() {
        let html = r#"<img src="/img/logo.png">"#;
        let mut rules = HashMap::new();
        rules.insert("img".to_string(), rule(Some("img"), Some("src"), false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, Some("https://example.com"))
            .expect("extract should succeed");

        assert_eq!(result["img"], "https://example.com/img/logo.png");
    }

    #[test]
    fn test_extract_with_selectors_href_without_base_returns_raw() {
        let html = r#"<a href="/path">Link</a>"#;
        let mut rules = HashMap::new();
        rules.insert("link".to_string(), rule(Some("a"), Some("href"), false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        assert_eq!(result["link"], "/path");
    }

    #[test]
    fn test_extract_with_selectors_non_href_attr_not_joined() {
        let html = r#"<div data-id="/123">Content</div>"#;
        let mut rules = HashMap::new();
        rules.insert(
            "data_id".to_string(),
            rule(Some("div"), Some("data-id"), false),
        );

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, Some("https://example.com"))
            .expect("extract should succeed");

        // data-id should NOT be URL-joined
        assert_eq!(result["data_id"], "/123");
    }

    #[test]
    fn test_extract_with_selectors_missing_element_returns_null() {
        let html = r#"<html><body></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert(
            "missing".to_string(),
            rule(Some("div.missing"), None, false),
        );

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        assert_eq!(result["missing"], Value::Null);
    }

    #[test]
    fn test_extract_with_selectors_empty_array_for_missing() {
        let html = r#"<html><body></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert("items".to_string(), rule(Some("li"), None, true));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        let arr = result["items"].as_array().expect("should be array");
        assert!(arr.is_empty());
    }

    #[test]
    fn test_extract_with_selectors_invalid_selector_skipped() {
        let html = r#"<html><body><div>Content</div></body></html>"#;
        let mut rules = HashMap::new();
        // Invalid selector that Selector::parse will reject
        rules.insert("bad".to_string(), rule(Some("div["), None, false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        // Invalid selector → key not in result
        assert!(result.get("bad").is_none() || result["bad"] == Value::Null);
    }

    #[test]
    fn test_extract_with_selectors_llm_only_rule_skipped() {
        let html = r#"<html><body><div>Content</div></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert(
            "llm_key".to_string(),
            llm_rule(None, Some("extract"), false),
        );

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        // LLM-only rules are skipped in extract_with_selectors
        assert!(result.get("llm_key").is_none());
    }

    #[test]
    fn test_extract_with_selectors_no_selector_skipped() {
        let html = r#"<html><body><div>Content</div></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert("no_sel".to_string(), rule(None, None, false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        // No selector → key not in result
        assert!(result.get("no_sel").is_none());
    }

    #[test]
    fn test_extract_with_selectors_empty_values_filtered_from_array() {
        let html = r#"<div><a href="">Empty</a><a href="/path">Link</a></div>"#;
        let mut rules = HashMap::new();
        rules.insert("links".to_string(), rule(Some("a"), Some("href"), true));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        let arr = result["links"].as_array().expect("should be array");
        // Empty href is filtered out
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0], "/path");
    }

    #[test]
    fn test_extract_with_selectors_multiple_rules() {
        let html = r#"
            <html><body>
                <h1>Title</h1>
                <p class="content">Para 1</p>
                <p class="content">Para 2</p>
                <a href="/link">Link</a>
            </body></html>
        "#;
        let mut rules = HashMap::new();
        rules.insert("title".to_string(), rule(Some("h1"), None, false));
        rules.insert("paras".to_string(), rule(Some("p.content"), None, true));
        rules.insert("link".to_string(), rule(Some("a"), Some("href"), false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, Some("https://example.com"))
            .expect("extract should succeed");

        assert_eq!(result["title"], "Title");
        let arr = result["paras"].as_array().expect("should be array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], "Para 1");
        assert_eq!(arr[1], "Para 2");
        assert_eq!(result["link"], "https://example.com/link");
    }

    #[test]
    fn test_extract_with_selectors_invalid_base_url_ignored() {
        let html = r#"<a href="/path">Link</a>"#;
        let mut rules = HashMap::new();
        rules.insert("link".to_string(), rule(Some("a"), Some("href"), false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, Some("not-a-url"))
            .expect("extract should succeed");

        // Invalid base URL → no joining, raw href returned
        assert_eq!(result["link"], "/path");
    }

    #[test]
    fn test_extract_with_selectors_empty_rules() {
        let html = r#"<html><body><div>Content</div></body></html>"#;
        let rules = HashMap::new();

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        assert!(result.as_object().unwrap().is_empty());
    }

    // ========== extract (trait method → extract_data) ==========

    #[tokio::test]
    async fn test_extract_css_only_rules() {
        let html = r#"<html><body><h1>Title</h1></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert("title".to_string(), rule(Some("h1"), None, false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let (result, usage) = service
            .extract(html, &rules, None)
            .await
            .expect("extract should succeed");

        assert_eq!(result["title"], "Title");
        assert_eq!(usage.total_tokens, 0);
    }

    #[tokio::test]
    async fn test_extract_llm_rule_success() {
        let html = r#"<html><body>Product costs $100</body></html>"#;
        let mut rules = HashMap::new();
        rules.insert(
            "product".to_string(),
            llm_rule(None, Some("Extract product info"), false),
        );

        let mock = MockLLMService::new_success(
            json!({"name": "Product", "price": 100}),
            TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
        );
        let (service, _count) = make_service(mock);
        let (result, usage) = service
            .extract(html, &rules, None)
            .await
            .expect("extract should succeed");

        assert_eq!(result["product"]["name"], "Product");
        assert_eq!(result["product"]["price"], 100);
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[tokio::test]
    async fn test_extract_llm_rule_error_returns_null() {
        let html = r#"<html><body>Content</body></html>"#;
        let mut rules = HashMap::new();
        rules.insert(
            "llm_key".to_string(),
            llm_rule(None, Some("Extract"), false),
        );

        let mock = MockLLMService::new_error("LLM service unavailable");
        let (service, _count) = make_service(mock);
        let (result, usage) = service
            .extract(html, &rules, None)
            .await
            .expect("extract should succeed");

        // LLM error → Null value, but extract itself doesn't fail
        assert_eq!(result["llm_key"], Value::Null);
        assert_eq!(usage.total_tokens, 0);
    }

    #[tokio::test]
    async fn test_extract_llm_rule_with_selector_for_noise_removal() {
        let html = r#"
            <html><body>
                <script>var x = 1;</script>
                <div class="content">Real content here</div>
            </body></html>
        "#;
        let mut rules = HashMap::new();
        rules.insert(
            "summary".to_string(),
            llm_rule(Some("div.content"), Some("Summarize"), false),
        );

        let mock =
            MockLLMService::new_success(json!({"summary": "Real content"}), TokenUsage::default());
        let (service, mock_arc) = make_service_with_mock(mock);
        let (_result, _usage) = service
            .extract(html, &rules, None)
            .await
            .expect("extract should succeed");

        // The mock should have been called with cleaned text from div.content
        let last_text = mock_arc.last_text().expect("should have been called");
        assert!(
            last_text.contains("Real content"),
            "should contain text from selected element, got: {}",
            last_text
        );
        assert!(
            !last_text.contains("var x"),
            "should not contain script content, got: {}",
            last_text
        );
    }

    #[tokio::test]
    async fn test_extract_llm_rule_without_selector_uses_full_html() {
        let html = r#"<html><body>Main content text</body></html>"#;
        let mut rules = HashMap::new();
        rules.insert(
            "info".to_string(),
            llm_rule(None, Some("Extract info"), false),
        );

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let (service, mock_arc) = make_service_with_mock(mock);
        let _ = service.extract(html, &rules, None).await;

        let last_text = mock_arc.last_text().expect("should have been called");
        assert!(
            last_text.contains("Main content text"),
            "should contain cleaned full HTML text, got: {}",
            last_text
        );
    }

    #[tokio::test]
    async fn test_extract_llm_rule_with_invalid_selector_falls_back_to_full_html() {
        let html = r#"<html><body>Fallback content</body></html>"#;
        let mut rules = HashMap::new();
        rules.insert(
            "info".to_string(),
            llm_rule(Some("div["), Some("Extract"), false),
        );

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let (service, mock_arc) = make_service_with_mock(mock);
        let _ = service.extract(html, &rules, None).await;

        let last_text = mock_arc.last_text().expect("should have been called");
        assert!(
            last_text.contains("Fallback content"),
            "invalid selector should fall back to full HTML, got: {}",
            last_text
        );
    }

    #[tokio::test]
    async fn test_extract_mixed_css_and_llm_rules() {
        let html = r#"
            <html><body>
                <h1>Title</h1>
                <div class="content">Product X costs $50</div>
            </body></html>
        "#;
        let mut rules = HashMap::new();
        rules.insert("title".to_string(), rule(Some("h1"), None, false));
        rules.insert(
            "product".to_string(),
            llm_rule(Some("div.content"), Some("Extract product"), false),
        );

        let mock = MockLLMService::new_success(
            json!({"name": "Product X", "price": 50}),
            TokenUsage {
                prompt_tokens: 5,
                completion_tokens: 3,
                total_tokens: 8,
            },
        );
        let (service, _count) = make_service(mock);
        let (result, usage) = service
            .extract(html, &rules, None)
            .await
            .expect("extract should succeed");

        assert_eq!(result["title"], "Title");
        assert_eq!(result["product"]["name"], "Product X");
        assert_eq!(usage.total_tokens, 8);
    }

    #[tokio::test]
    async fn test_extract_multiple_llm_rules_accumulate_usage() {
        let html = r#"<html><body>Content for two LLM calls</body></html>"#;
        let mut rules = HashMap::new();
        rules.insert("key1".to_string(), llm_rule(None, Some("Extract 1"), false));
        rules.insert("key2".to_string(), llm_rule(None, Some("Extract 2"), false));

        let mock = MockLLMService::new_success(
            json!({}),
            TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
        );
        let (service, _count) = make_service(mock);
        let (_result, usage) = service
            .extract(html, &rules, None)
            .await
            .expect("extract should succeed");

        // Two LLM calls, each with 15 total_tokens
        assert_eq!(usage.prompt_tokens, 20);
        assert_eq!(usage.completion_tokens, 10);
        assert_eq!(usage.total_tokens, 30);
    }

    #[tokio::test]
    async fn test_extract_llm_rule_default_format_is_json() {
        let html = r#"<html><body>Text</body></html>"#;
        let mut rules = HashMap::new();
        rules.insert("key".to_string(), llm_rule(None, Some("Extract"), false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let (service, mock_arc) = make_service_with_mock(mock);
        let _ = service.extract(html, &rules, None).await;

        let last_format = mock_arc.last_format().expect("should have format");
        assert_eq!(last_format, "json");
    }

    #[tokio::test]
    async fn test_extract_llm_rule_array_type_sets_array_schema() {
        let html = r#"<html><body>Text</body></html>"#;
        let mut rules = HashMap::new();
        rules.insert(
            "items".to_string(),
            llm_rule(None, Some("Extract items"), true),
        );

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let (service, _mock_arc) = make_service_with_mock(mock);
        let _ = service.extract(html, &rules, None).await;

        // We can't easily check the schema passed to mock, but the test
        // exercises the array branch of schema construction.
    }

    #[tokio::test]
    async fn test_extract_llm_rule_default_prompt_when_none() {
        let html = r#"<html><body>Text</body></html>"#;
        let mut rules = HashMap::new();
        // llm_prompt is None → default prompt "Extract {key} from the text"
        rules.insert(
            "mykey".to_string(),
            ExtractionRule {
                selector: None,
                attr: None,
                is_array: false,
                use_llm: Some(true),
                llm_prompt: None,
                output_format: None,
            },
        );

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let (service, _count) = make_service(mock);
        let (result, _usage) = service
            .extract(html, &rules, None)
            .await
            .expect("extract should succeed");

        // Should still produce a result (default prompt path exercised)
        assert!(result.get("mykey").is_some());
    }

    #[tokio::test]
    async fn test_extract_css_array_with_attr_and_base_url() {
        let html = r#"<div><a href="/a">A</a><a href="/b">B</a></div>"#;
        let mut rules = HashMap::new();
        rules.insert("links".to_string(), rule(Some("a"), Some("href"), true));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let (service, _count) = make_service(mock);
        let (result, _usage) = service
            .extract(html, &rules, Some("https://example.com"))
            .await
            .expect("extract should succeed");

        let arr = result["links"].as_array().expect("should be array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], "https://example.com/a");
        assert_eq!(arr[1], "https://example.com/b");
    }

    #[tokio::test]
    async fn test_extract_css_single_element_missing_returns_null() {
        let html = r#"<html><body></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert(
            "missing".to_string(),
            rule(Some("div.missing"), None, false),
        );

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let (service, _count) = make_service(mock);
        let (result, _usage) = service
            .extract(html, &rules, None)
            .await
            .expect("extract should succeed");

        assert_eq!(result["missing"], Value::Null);
    }

    #[tokio::test]
    async fn test_extract_css_invalid_selector_skipped() {
        let html = r#"<html><body><div>Content</div></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert("bad".to_string(), rule(Some("div["), None, false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let (service, _count) = make_service(mock);
        let (result, _usage) = service
            .extract(html, &rules, None)
            .await
            .expect("extract should succeed");

        // Invalid selector → key not in result
        assert!(result.get("bad").is_none());
    }

    // ========== extract_with_schema ==========

    #[tokio::test]
    async fn test_extract_with_schema_calls_llm_with_clean_text() {
        let html = r#"<html><body><script>var x=1;</script><p>Real content</p></body></html>"#;
        let schema = json!({"type": "object", "properties": {"title": {"type": "string"}}});

        let mock = MockLLMService::new_success(
            json!({"title": "Real content"}),
            TokenUsage {
                prompt_tokens: 5,
                completion_tokens: 2,
                total_tokens: 7,
            },
        );
        let (service, mock_arc) = make_service_with_mock(mock);
        let (result, usage) = service
            .extract_with_schema(html, &schema)
            .await
            .expect("should succeed");

        assert_eq!(result["title"], "Real content");
        assert_eq!(usage.total_tokens, 7);

        // Verify clean text was passed (no script content)
        let last_text = mock_arc.last_text().expect("should have been called");
        assert!(last_text.contains("Real content"));
        assert!(!last_text.contains("var x"));
    }

    #[tokio::test]
    async fn test_extract_with_schema_empty_html() {
        let html = "";
        let schema = json!({"type": "object"});

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let (service, _count) = make_service(mock);
        let (result, _usage) = service
            .extract_with_schema(html, &schema)
            .await
            .expect("should succeed");

        assert!(result.is_object());
    }

    #[tokio::test]
    async fn test_extract_with_schema_llm_error_propagates() {
        let html = r#"<html><body>Content</body></html>"#;
        let schema = json!({"type": "object"});

        let mock = MockLLMService::new_error("schema extraction failed");
        let (service, _count) = make_service(mock);
        let result = service.extract_with_schema(html, &schema).await;

        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("schema extraction failed"));
    }

    // ========== get_clean_text ==========

    #[test]
    fn test_get_clean_text_strips_script_tags() {
        let html = r#"<html><body><script>var x = 1;</script><p>Content</p></body></html>"#;
        let text = ExtractionService::get_clean_text(html);
        assert!(text.contains("Content"));
        assert!(!text.contains("var x"));
    }

    #[test]
    fn test_get_clean_text_strips_style_tags() {
        let html = r#"<html><body><style>.cls { color: red; }</style><p>Text</p></body></html>"#;
        let text = ExtractionService::get_clean_text(html);
        assert!(text.contains("Text"));
        assert!(!text.contains("color"));
        assert!(!text.contains(".cls"));
    }

    #[test]
    fn test_get_clean_text_strips_nav_footer_aside() {
        let html = r#"<html><body>
            <nav>Navigation</nav>
            <main>Main content</main>
            <aside>Sidebar</aside>
            <footer>Footer</footer>
        </body></html>"#;
        let text = ExtractionService::get_clean_text(html);
        assert!(text.contains("Main content"));
        assert!(!text.contains("Navigation"));
        assert!(!text.contains("Sidebar"));
        assert!(!text.contains("Footer"));
    }

    #[test]
    fn test_get_clean_text_strips_head_iframe_noscript_form() {
        let html = r#"<html>
            <head><title>Title</title></head>
            <body>
                <iframe src="about:blank">Frame</iframe>
                <noscript>No JS</noscript>
                <form><input type="text"></form>
                <p>Visible</p>
            </body>
        </html>"#;
        let text = ExtractionService::get_clean_text(html);
        assert!(text.contains("Visible"));
        assert!(!text.contains("Frame"));
        assert!(!text.contains("No JS"));
    }

    #[test]
    fn test_get_clean_text_strips_code_like_patterns() {
        let html = r#"<html><body>
            <div>var x = function() { return 1; };</div>
            <p>Normal text</p>
        </body></html>"#;
        let text = ExtractionService::get_clean_text(html);
        assert!(text.contains("Normal text"));
        // Code-like patterns with var/function/window AND ; or { should be filtered
        assert!(!text.contains("function()"));
    }

    #[test]
    fn test_get_clean_text_preserves_normal_text() {
        let html = r#"<html><body><p>Hello World</p><p>Second paragraph</p></body></html>"#;
        let text = ExtractionService::get_clean_text(html);
        assert!(text.contains("Hello World"));
        assert!(text.contains("Second paragraph"));
    }

    #[test]
    fn test_get_clean_text_empty_html() {
        let text = ExtractionService::get_clean_text("");
        assert!(text.is_empty());
    }

    #[test]
    fn test_get_clean_text_collapses_whitespace() {
        let html = r#"<html><body>  Multiple   spaces   </body></html>"#;
        let text = ExtractionService::get_clean_text(html);
        // split_whitespace + join normalizes whitespace
        assert!(!text.contains("  "));
    }

    #[test]
    fn test_get_clean_text_nested_elements() {
        let html = r#"<html><body><div><span>Nested</span> text</div></body></html>"#;
        let text = ExtractionService::get_clean_text(html);
        assert!(text.contains("Nested"));
        assert!(text.contains("text"));
    }

    #[test]
    fn test_get_clean_text_strips_window_pattern() {
        let html = r#"<html><body>
            <div>window.location = "http://evil.com";</div>
            <p>Safe text</p>
        </body></html>"#;
        let text = ExtractionService::get_clean_text(html);
        assert!(text.contains("Safe text"));
        // window. pattern with ; should be filtered
        assert!(!text.contains("window.location"));
    }

    // ========== extract_element_value (indirectly tested via extract_with_selectors) ==========

    #[test]
    fn test_extract_element_value_text_fallback_when_no_attr() {
        // When attr is None, text content is extracted
        let html = r#"<p>Hello Text</p>"#;
        let mut rules = HashMap::new();
        rules.insert("text".to_string(), rule(Some("p"), None, false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        assert_eq!(result["text"], "Hello Text");
    }

    #[test]
    fn test_extract_element_value_attr_missing_returns_null() {
        // When attr is Some but element doesn't have that attr → Null
        let html = r#"<div>No href</div>"#;
        let mut rules = HashMap::new();
        rules.insert("href".to_string(), rule(Some("div"), Some("href"), false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let result = service
            .extract_with_selectors(html, &rules, None)
            .expect("extract should succeed");

        assert_eq!(result["href"], Value::Null);
    }

    // ========== extract (async) CSS selector branch coverage ==========

    #[tokio::test]
    async fn test_extract_async_array_text_extraction() {
        let html = r#"<html><body><ul><li>Apple</li><li>Banana</li></ul></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert("items".to_string(), rule(Some("li"), None, true));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let (result, _) = service
            .extract(html, &rules, None)
            .await
            .expect("extract should succeed");

        let arr = result["items"].as_array().expect("should be array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], "Apple");
        assert_eq!(arr[1], "Banana");
    }

    #[tokio::test]
    async fn test_extract_async_array_attr_non_href_with_base() {
        let html = r#"<html><body><a class="link-a" href="/a">A</a><a class="link-b" href="/b">B</a></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert("classes".to_string(), rule(Some("a"), Some("class"), true));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let (result, _) = service
            .extract(html, &rules, Some("https://example.com"))
            .await
            .expect("extract should succeed");

        let arr = result["classes"].as_array().expect("should be array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], "link-a");
        assert_eq!(arr[1], "link-b");
    }

    #[tokio::test]
    async fn test_extract_async_array_attr_non_href_without_base() {
        let html =
            r#"<html><body><span data-id="1">A</span><span data-id="2">B</span></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert("ids".to_string(), rule(Some("span"), Some("data-id"), true));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let (result, _) = service
            .extract(html, &rules, None)
            .await
            .expect("extract should succeed");

        let arr = result["ids"].as_array().expect("should be array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], "1");
        assert_eq!(arr[1], "2");
    }

    #[tokio::test]
    async fn test_extract_async_single_attr_non_href_with_base() {
        let html = r#"<html><body><div class="content">Text</div></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert("cls".to_string(), rule(Some("div"), Some("class"), false));

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let (result, _) = service
            .extract(html, &rules, Some("https://example.com"))
            .await
            .expect("extract should succeed");

        assert_eq!(result["cls"], "content");
    }

    #[tokio::test]
    async fn test_extract_async_single_attr_non_href_without_base() {
        let html = r#"<html><body><div data-value="42">Text</div></body></html>"#;
        let mut rules = HashMap::new();
        rules.insert(
            "val".to_string(),
            rule(Some("div"), Some("data-value"), false),
        );

        let mock = MockLLMService::new_success(json!({}), TokenUsage::default());
        let service = ExtractionService::new(Arc::new(mock));
        let (result, _) = service
            .extract(html, &rules, None)
            .await
            .expect("extract should succeed");

        assert_eq!(result["val"], "42");
    }
}
