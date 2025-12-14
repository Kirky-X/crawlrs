use anyhow::Result;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use crate::domain::services::llm_service::LLMService;

/// 提取规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionRule {
    pub selector: Option<String>, // Make selector optional for LLM extraction
    pub attr: Option<String>, // If None, extract text
    pub is_array: bool,
    pub use_llm: Option<bool>, // New field to enable LLM extraction
    pub llm_prompt: Option<String>, // Optional specific prompt for this rule
}

/// 提取服务
///
/// 负责从 HTML 内容中提取结构化数据
pub struct ExtractionService;

impl ExtractionService {
    /// 提取数据
    pub async fn extract(html_content: &str, rules: &HashMap<String, ExtractionRule>) -> Result<Value> {
        let mut result = HashMap::new();
        let llm_service = LLMService::new();

        for (key, rule) in rules {
            if rule.use_llm.unwrap_or(false) {
                // Use LLM for extraction
                let prompt = rule.llm_prompt.clone().unwrap_or_else(|| format!("Extract {} from the text", key));
                let schema = json!({ "type": if rule.is_array { "array" } else { "string" }, "description": prompt });
                
                // Pass the raw HTML or text content to LLM
                // Ideally, we might want to pass specific parts if a selector is provided
                // Note: parsing document is not Send safe if done outside async context or held across await points if not careful
                // So we parse inside the scope where needed or avoid holding Html across await
                let content_to_process = if let Some(sel) = &rule.selector {
                     // Parse document locally just for this scope to avoid Send issues? 
                     // Or just pass the raw html if selector parsing is complex?
                     // Let's parse just for extraction to be safe, though it might be less efficient
                     let document = Html::parse_document(html_content);
                     if let Ok(selector) = Selector::parse(sel) {
                        document.select(&selector)
                            .map(|e| e.text().collect::<Vec<_>>().join(" "))
                            .collect::<Vec<_>>()
                            .join("\n")
                     } else {
                         html_content.to_string()
                     }
                } else {
                    html_content.to_string()
                };

                match llm_service.extract_data(&content_to_process, &schema).await {
                     Ok(val) => { result.insert(key.clone(), val); },
                     Err(_) => { result.insert(key.clone(), Value::Null); }
                }
                continue;
            }

            // Traditional CSS Selector Extraction
            let selector_str = match &rule.selector {
                Some(s) => s,
                None => continue, 
            };
            
            let selector = match Selector::parse(selector_str) {
                Ok(s) => s,
                Err(_) => continue, // Skip invalid selectors
            };
            
            // Parse document for traditional extraction
            let document = Html::parse_document(html_content);

            if rule.is_array {
                let mut values = Vec::new();
                for element in document.select(&selector) {
                    let value = if let Some(attr) = &rule.attr {
                        element.value().attr(attr).map(|s| s.to_string())
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

                    if let Some(v) = value {
                        if !v.is_empty() {
                            values.push(Value::String(v));
                        }
                    }
                }
                result.insert(key.clone(), Value::Array(values));
            } else if let Some(element) = document.select(&selector).next() {
                let value = if let Some(attr) = &rule.attr {
                    element.value().attr(attr).map(|s| s.to_string())
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

                if let Some(v) = value {
                    result.insert(key.clone(), Value::String(v));
                } else {
                    result.insert(key.clone(), Value::Null);
                }
            } else {
                result.insert(key.clone(), Value::Null);
            }
        }

        Ok(json!(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title_and_links() {
        let html = r#"
            <html>
                <head><title>Test Page</title></head>
                <body>
                    <h1>Main Header</h1>
                    <div class="content">
                        <p>Paragraph 1</p>
                        <p>Paragraph 2</p>
                    </div>
                    <a href="https://example.com/1">Link 1</a>
                    <a href="https://example.com/2">Link 2</a>
                </body>
            </html>
        "#;

        let mut rules = HashMap::new();
        rules.insert(
            "title".to_string(),
            ExtractionRule {
                selector: Some("title".to_string()),
                attr: None,
                is_array: false,
                use_llm: None,
                llm_prompt: None,
            },
        );
        rules.insert(
            "header".to_string(),
            ExtractionRule {
                selector: Some("h1".to_string()),
                attr: None,
                is_array: false,
                use_llm: None,
                llm_prompt: None,
            },
        );
        rules.insert(
            "paragraphs".to_string(),
            ExtractionRule {
                selector: Some("div.content p".to_string()),
                attr: None,
                is_array: true,
                use_llm: None,
                llm_prompt: None,
            },
        );
        rules.insert(
            "links".to_string(),
            ExtractionRule {
                selector: Some("a".to_string()),
                attr: Some("href".to_string()),
                is_array: true,
                use_llm: None,
                llm_prompt: None,
            },
        );

        let result = tokio_test::block_on(ExtractionService::extract(html, &rules)).unwrap();

        assert_eq!(result["title"], "Test Page");
        assert_eq!(result["header"], "Main Header");

        let paragraphs = result["paragraphs"].as_array().unwrap();
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(paragraphs[0], "Paragraph 1");
        assert_eq!(paragraphs[1], "Paragraph 2");

        let links = result["links"].as_array().unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], "https://example.com/1");
        assert_eq!(links[1], "https://example.com/2");
    }
}
