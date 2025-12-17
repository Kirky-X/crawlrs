// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::config::settings::Settings;
use crate::domain::services::extraction_service::{ExtractionRule, ExtractionService};
use crate::domain::services::llm_service::LLMService;
use axum::{routing::post, Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::net::TcpListener;

#[test]
fn test_extraction_service_basic_selectors() {
    let html = r#"
        <html>
            <body>
                <h1 id="main-title">Hello World</h1>
                <div class="content">
                    <p>Paragraph 1</p>
                    <p>Paragraph 2</p>
                </div>
                <a href="https://example.com">Link</a>
            </body>
        </html>
    "#;

    let mut rules = HashMap::new();
    rules.insert(
        "title".to_string(),
        ExtractionRule {
            selector: Some("h1#main-title".to_string()),
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
        "link_href".to_string(),
        ExtractionRule {
            selector: Some("a".to_string()),
            attr: Some("href".to_string()),
            is_array: false,
            use_llm: None,
            llm_prompt: None,
        },
    );

    let settings = Settings::new().unwrap();
    let (result, _) = tokio_test::block_on(ExtractionService::extract(html, &rules, &settings)).unwrap();

    assert_eq!(result["title"], "Hello World");

    let paragraphs = result["paragraphs"].as_array().unwrap();
    assert_eq!(paragraphs.len(), 2);
    assert_eq!(paragraphs[0], "Paragraph 1");
    assert_eq!(paragraphs[1], "Paragraph 2");

    assert_eq!(result["link_href"], "https://example.com");
}

#[test]
fn test_extraction_service_missing_elements() {
    let html = "<html><body></body></html>";
    let mut rules = HashMap::new();
    rules.insert(
        "missing".to_string(),
        ExtractionRule {
            selector: Some("div.missing".to_string()),
            attr: None,
            is_array: false,
            use_llm: None,
            llm_prompt: None,
        },
    );

    let settings = Settings::new().unwrap();
    let (result, _) = tokio_test::block_on(ExtractionService::extract(html, &rules, &settings)).unwrap();
    assert_eq!(result["missing"], Value::Null);
}

#[test]
fn test_extraction_service_empty_array() {
    let html = "<html><body></body></html>";
    let mut rules = HashMap::new();
    rules.insert(
        "missing_list".to_string(),
        ExtractionRule {
            selector: Some("li".to_string()),
            attr: None,
            is_array: true,
            use_llm: None,
            llm_prompt: None,
        },
    );

    let settings = Settings::new().unwrap();
    let (result, _) = tokio_test::block_on(ExtractionService::extract(html, &rules, &settings)).unwrap();
    let list = result["missing_list"].as_array().unwrap();
    assert!(list.is_empty());
}

#[tokio::test]
async fn test_extraction_service_with_llm() {
    let html = "<html><body>Some unstructured text about Product X costing $100</body></html>";

    let mut rules = HashMap::new();
    rules.insert(
        "product_info".to_string(),
        ExtractionRule {
            selector: None,
            attr: None,
            is_array: false,
            use_llm: Some(true),
            llm_prompt: Some("Extract product name and price".to_string()),
        },
    );

    // Setup Local Server for LLM
    let content_json = json!({"name": "Product X", "price": 100}).to_string();
    let fake_response = json!({
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "created": 1677652288,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content_json
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 10,
            "total_tokens": 20
        }
    });

    let app = Router::new().route(
        "/chat/completions",
        post(move |Json(_): Json<Value>| {
            let resp = fake_response.clone();
            async move { Json(resp) }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Use Real LLMService with Local Server
    let llm_service = LLMService::new_with_config(
        "test-key".to_string(),
        "gpt-3.5-turbo".to_string(),
        server_url,
    );

    let service = ExtractionService::new(Box::new(llm_service));
    let (result, _) = service.extract_data(html, &rules).await.unwrap();

    assert_eq!(result["product_info"]["name"], "Product X");
    assert_eq!(result["product_info"]["price"], 100);
}
