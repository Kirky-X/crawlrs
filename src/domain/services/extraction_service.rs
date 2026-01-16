// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::config::settings::Settings;
pub use crate::domain::services::llm_service::TokenUsage;
use crate::domain::services::llm_service::{LLMService, LLMServiceTrait};
use anyhow::Result;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

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
        let document = Html::parse_document(html_content);
        let base = base_url.and_then(|u| url::Url::parse(u).ok());

        for (key, rule) in rules {
            // 跳过LLM-only规则
            if rule.use_llm.unwrap_or(false) {
                continue;
            }

            // 获取CSS选择器
            let selector_str = match &rule.selector {
                Some(s) => s,
                None => continue,
            };

            let selector = match Selector::parse(selector_str) {
                Ok(s) => s,
                Err(_) => continue,
            };

            if rule.is_array {
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

                    if let Some(v) = value {
                        if !v.is_empty() {
                            values.push(Value::String(v));
                        }
                    }
                }
                result.insert(key.clone(), Value::Array(values));
            } else if let Some(element) = document.select(&selector).next() {
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

    /// 提取数据（完整版本，需要Settings用于LLM）
    pub async fn extract(
        html_content: &str,
        rules: &HashMap<String, ExtractionRule>,
        settings: &Settings,
        base_url: Option<&str>,
    ) -> Result<(Value, TokenUsage)> {
        let service = Self::new(Box::new(LLMService::new(settings)));
        service.extract_data(html_content, rules, base_url).await
    }

    /// 使用全局 Schema 直接通过 LLM 提取数据
    pub async fn extract_with_schema(
        html_content: &str,
        schema: &Value,
        settings: &Settings,
    ) -> Result<(Value, TokenUsage)> {
        let llm_service = LLMService::new(settings);

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
        // Pre-allocate based on rules count
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
                            .map(|e| Self::get_clean_text(&e.html())) // 关键修改：对选中的 HTML 再次进行去噪
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

                    if let Some(v) = value {
                        if !v.is_empty() {
                            values.push(Value::String(v));
                        }
                    }
                }
                result.insert(key.clone(), Value::Array(values));
            } else if let Some(element) = document.select(&selector).next() {
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

                if let Some(v) = value {
                    result.insert(key.clone(), Value::String(v));
                } else {
                    result.insert(key.clone(), Value::Null);
                }
            } else {
                result.insert(key.clone(), Value::Null);
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
                    // 额外检查：如果文本块内包含典型的代码模式，则丢弃（防止某些未被标签包裹的代码泄露）
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_with_real_html() {
        // 读取本地HTML文件
        let html = std::fs::read_to_string("temp/extraction_test/raw.html")
            .expect("Failed to read HTML file");

        let mut rules = HashMap::with_capacity(4);
        rules.insert(
            "title".to_string(),
            ExtractionRule {
                selector: Some("title".to_string()),
                attr: None,
                is_array: false,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );
        rules.insert(
            "paragraphs".to_string(),
            ExtractionRule {
                selector: Some("p".to_string()),
                attr: None,
                is_array: true,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );
        rules.insert(
            "links".to_string(),
            ExtractionRule {
                selector: Some("a[href]".to_string()),
                attr: Some("href".to_string()),
                is_array: true,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );
        rules.insert(
            "images".to_string(),
            ExtractionRule {
                selector: Some("img[src]".to_string()),
                attr: Some("src".to_string()),
                is_array: true,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );

        use crate::config::settings::Settings;

        let settings = Settings::new().expect("Failed to load settings");

        let (result, _) =
            ExtractionService::extract(&html, &rules, &settings, Some("https://news.cctv.com/"))
                .await
                .expect("Extraction failed");

        // 保存结果
        std::fs::create_dir_all("temp/extraction_test").ok();
        std::fs::write(
            "temp/extraction_test/result.json",
            serde_json::to_string_pretty(&result).unwrap(),
        )
        .ok();

        // 验证
        assert!(result.get("title").is_some());
        assert!(result.get("paragraphs").is_some());
        assert!(result.get("links").is_some());
        assert!(result.get("images").is_some());
    }

    #[tokio::test]
    async fn test_comprehensive_extraction_with_real_html_and_mock_llm() {
        use axum::{routing::post, Json, Router};
        use tokio::net::TcpListener;

        // 使用真实 HTML 文件
        let html = std::fs::read_to_string("temp/extraction_test/raw.html")
            .expect("Failed to read temp/extraction_test/raw.html");

        let mut rules = HashMap::new();

        // CSS 选择器提取规则
        rules.insert(
            "page_title".to_string(),
            ExtractionRule {
                selector: Some("title".to_string()),
                attr: None,
                is_array: false,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );

        rules.insert(
            "article_title".to_string(),
            ExtractionRule {
                selector: Some("div.title_area h1".to_string()),
                attr: None,
                is_array: false,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );

        rules.insert(
            "author_meta".to_string(),
            ExtractionRule {
                selector: Some(r#"meta[name="author"]"#.to_string()),
                attr: Some("content".to_string()),
                is_array: false,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );

        rules.insert(
            "paragraphs".to_string(),
            ExtractionRule {
                selector: Some(r#"div.content_area p[style*="text-indent"]"#.to_string()),
                attr: None,
                is_array: true,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );

        rules.insert(
            "image_urls".to_string(),
            ExtractionRule {
                selector: Some("div.content_area img[src]".to_string()),
                attr: Some("src".to_string()),
                is_array: true,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );

        // LLM 提取规则
        rules.insert(
            "llm_summary".to_string(),
            ExtractionRule {
                selector: Some("div.content_area".to_string()),
                attr: None,
                is_array: false,
                use_llm: Some(true),
                llm_prompt: Some("请返回 JSON 对象，包含 title 和 category 字段".to_string()),
                output_format: None,
            },
        );

        // 搭建本地 Mock LLM 服务器
        let llm_content_json =
            json!({"title": "消费与外贸走势", "category": "宏观经济"}).to_string();

        let canned_response = json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1677652289,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": llm_content_json
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 30,
                "total_tokens": 50
            }
        });

        let app = Router::new().route(
            "/chat/completions",
            post(move |Json(_): Json<Value>| {
                let resp = canned_response.clone();
                async move { Json(resp) }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind address for mock LLM");
        let addr = listener.local_addr().expect("Failed to get local address");
        let server_url = format!("http://{}", addr);

        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Failed to start mock LLM server");
        });

        // 使用 mock LLM 构造 ExtractionService
        let llm_service = LLMService::new_with_config(
            "test-key".to_string(),
            "gpt-3.5-turbo".to_string(),
            server_url,
        );
        let service = ExtractionService::new(Box::new(llm_service));

        // 执行提取
        let (result, usage) = service
            .extract_data(&html, &rules, Some("https://news.cctv.com/"))
            .await
            .expect("Failed to extract data");

        // 验证 CSS 提取结果
        let page_title = result["page_title"].as_str().expect("page_title missing");
        assert!(page_title.contains("消费") && page_title.contains("新闻频道"));

        let article_title = result["article_title"]
            .as_str()
            .expect("article_title missing");
        assert!(article_title.contains("消费") && article_title.contains("外贸"));

        let author_meta = result["author_meta"].as_str().expect("author_meta missing");
        assert_eq!(author_meta, "刘珊");

        let paragraphs = result["paragraphs"].as_array().expect("paragraphs missing");
        assert!(!paragraphs.is_empty());

        let image_urls = result["image_urls"].as_array().expect("image_urls missing");
        assert!(image_urls.len() >= 3);

        // 验证 LLM 提取结果
        let llm_summary = &result["llm_summary"];
        assert!(llm_summary.is_object());
        assert_eq!(llm_summary["title"], "消费与外贸走势");
        assert_eq!(llm_summary["category"], "宏观经济");

        // 验证 token 使用
        assert!(usage.total_tokens >= usage.prompt_tokens + usage.completion_tokens);

        // 保存结果
        std::fs::create_dir_all("temp/extraction_test").ok();
        std::fs::write(
            "temp/extraction_test/result_comprehensive.json",
            serde_json::to_string_pretty(&result).unwrap(),
        )
        .ok();
    }

    #[tokio::test]
    #[ignore] // 仅在本地有 LLM 时运行
    async fn test_real_llm_extraction() {
        use crate::config::settings::Settings;
        let mut settings = Settings::new().expect("Failed to load settings");

        // 配置本地 LLM
        settings.llm.provider = Some("ollama".to_string());
        settings.llm.model = Some("qwen3:8b".to_string());
        settings.llm.api_base_url = Some("http://172.24.160.1:11434".to_string());

        let service = ExtractionService::new(Box::new(LLMService::new(&settings)));

        let html = r#"
            <html>
                <head><title>Test Page</title><style>.noise { color: red; }</style></head>
                <body>
                    <nav>Menu Item 1</nav>
                    <div class="content">
                        <h1>真正的主题</h1>
                        <p>这是第一段有价值的内容。</p>
                        <p>这是第二段有价值的内容，包含一个<a href="https://example.com">链接</a>。</p>
                        <div class="noise">这是样式噪音</div>
                    </div>
                    <script>console.log('noise');</script>
                    <footer>Footer info</footer>
                </body>
            </html>
        "#;

        let mut rules = HashMap::new();
        rules.insert(
            "summary_json".to_string(),
            ExtractionRule {
                selector: None,
                attr: None,
                is_array: false,
                use_llm: Some(true),
                llm_prompt: Some("总结这篇文章的主题和主要内容".to_string()),
                output_format: Some("json".to_string()),
            },
        );
        rules.insert(
            "content_md".to_string(),
            ExtractionRule {
                selector: Some(".content".to_string()),
                attr: None,
                is_array: false,
                use_llm: Some(true),
                llm_prompt: Some("提取正文内容".to_string()),
                output_format: Some("markdown".to_string()),
            },
        );

        let (result, usage) = service
            .extract_data(html, &rules, None)
            .await
            .expect("Real LLM extraction failed");

        println!("Result: {:#?}", result);
        println!("Usage: {:#?}", usage);

        assert!(result.get("summary_json").is_some());
        assert!(result.get("content_md").is_some());
    }
}
