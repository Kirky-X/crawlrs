// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::{routing::post, Json, Router};
use crawlrs::config::settings::Settings;
use crawlrs::domain::services::extraction_service::{ExtractionRule, ExtractionService};
use crawlrs::domain::services::llm_service::LLMService;
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
            output_format: None,
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
            output_format: None,
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
            output_format: None,
        },
    );

    let settings = Settings::new().expect("Failed to load settings");
    let (result, _) = tokio_test::block_on(ExtractionService::extract(html, &rules, &settings, None))
        .expect("Failed to extract data");

    assert_eq!(result["title"], "Hello World");

    let paragraphs = result["paragraphs"]
        .as_array()
        .expect("Missing 'paragraphs' array in result");
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
            output_format: None,
        },
    );

    let settings = Settings::new().expect("Failed to load settings");
    let (result, _) = tokio_test::block_on(ExtractionService::extract(html, &rules, &settings, None))
        .expect("Failed to extract data");
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
            output_format: None,
        },
    );

    let settings = Settings::new().expect("Failed to load settings");
    let (result, _) = tokio_test::block_on(ExtractionService::extract(html, &rules, &settings, None))
        .expect("Failed to extract data");
    let list = result["missing_list"]
        .as_array()
        .expect("Missing 'missing_list' array in result");
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
            output_format: None,
        },
    );

    // Setup Local Server for LLM
    let content_json = json!({"name": "Product X", "price": 100}).to_string();
    let canned_response = json!({
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
            let resp = canned_response.clone();
            async move { Json(resp) }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to address");
    let addr = listener.local_addr().expect("Failed to get local address");
    let server_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("Failed to start server");
    });

    // Use Real LLMService with Local Server
    let llm_service = LLMService::new_with_config(
        "test-key".to_string(),
        "gpt-3.5-turbo".to_string(),
        server_url,
    );

    let service = ExtractionService::new(Box::new(llm_service));
    let (result, _) = service
        .extract_data(html, &rules, None)
        .await
        .expect("Failed to extract data");

    assert_eq!(result["product_info"]["name"], "Product X");
    assert_eq!(result["product_info"]["price"], 100);
}

#[tokio::test]
async fn test_extraction_service_comprehensive_with_real_html_and_llm() {
    // 使用真实 HTML 文件作为输入数据源
    let html = std::fs::read_to_string("temp/extraction_test/raw.html")
        .expect("Failed to read temp/extraction_test/raw.html");

    // 构造综合提取规则，覆盖 ExtractionRule 的各种配置选项
    let mut rules = HashMap::new();

    // 1. CSS：页面 <title> 文本（selector + text，非数组）
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

    // 2. CSS：正文主标题 h1（selector + text，非数组）
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

    // 3. CSS：作者 meta[name="author"] 的 content 属性（selector + attr，非数组）
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

    // 4. CSS：来源 meta[name="source"] 的 content 属性（selector + attr，非数组）
    rules.insert(
        "source_meta".to_string(),
        ExtractionRule {
            selector: Some(r#"meta[name="source"]"#.to_string()),
            attr: Some("content".to_string()),
            is_array: false,
            use_llm: None,
            llm_prompt: None,
            output_format: None,
        },
    );

    // 5. CSS：正文段落数组（selector + text，数组）
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

    // 6. CSS：正文图片 URL 数组（selector + attr，数组）
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

    // 7. CSS：编辑信息（底部区域文本，非数组）
    rules.insert(
        "editor".to_string(),
        ExtractionRule {
            selector: Some("div.bottom_ind01 .zebian span".to_string()),
            attr: None,
            is_array: false,
            use_llm: None,
            llm_prompt: None,
            output_format: None,
        },
    );

    // 8. LLM：基于正文内容生成结构化摘要（use_llm = true，非数组）
    rules.insert(
        "llm_summary".to_string(),
        ExtractionRule {
            selector: Some("div.content_area".to_string()),
            attr: None,
            is_array: false,
            use_llm: Some(true),
            llm_prompt: Some(
                "请阅读新闻正文内容，并返回一个 JSON 对象，包含字段\
                 \"title\" 和 \"category\"，用中文简要概括新闻标题和主题分类。"
                    .to_string(),
            ),
            output_format: None,
        },
    );

    // ===== 搭建本地 LLM Mock 服务（参考现有 LLM 测试模式） =====
    let llm_content_json =
        json!({"title": "消费与外贸走势", "category": "宏观经济与对外开放"}).to_string();

    let canned_response = json!({
        "id": "chatcmpl-456",
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
        .expect("Failed to bind to address for LLM mock server");
    let addr = listener
        .local_addr()
        .expect("Failed to get local address for LLM mock server");
    let server_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("Failed to start LLM mock server");
    });

    // 使用真实的 LLMService + 本地 Mock Server 构造 ExtractionService
    let llm_service = LLMService::new_with_config(
        "test-key".to_string(),
        "gpt-3.5-turbo".to_string(),
        server_url,
    );
    let service = ExtractionService::new(Box::new(llm_service));

    // 调用真实的解析逻辑（CSS + LLM 综合）
    let (result, usage) = service
        .extract_data(&html, &rules, Some("https://news.cctv.com/"))
        .await
        .expect("Failed to extract data from real HTML");

    // ===== CSS 提取结果断言 =====

     // page_title：应包含新闻主标题和频道信息
    let page_title = result["page_title"]
        .as_str()
        .expect("page_title should be a string");
    assert!(
        page_title.contains("消费") && page_title.contains("新闻频道"),
        "unexpected page_title: {}",
        page_title
    );

    // article_title：正文主标题
    let article_title = result["article_title"]
        .as_str()
        .expect("article_title should be a string");
    assert!(
        article_title.contains("消费") && article_title.contains("外贸"),
        "unexpected article_title: {}",
        article_title
    );

    // 作者与来源 meta
    let author_meta = result["author_meta"]
        .as_str()
        .expect("author_meta should be a string");
    assert_eq!(author_meta, "刘珊");

    let source_meta = result["source_meta"]
        .as_str()
        .expect("source_meta should be a string");
    assert_eq!(source_meta, "央视网");

    // 段落数组 paragraphs：应为非空数组，且首段包含关键句
    let paragraphs = result["paragraphs"]
        .as_array()
        .expect("paragraphs should be an array");
    assert!(
        !paragraphs.is_empty(),
        "paragraphs should not be empty for real article"
    );
    let first_paragraph = paragraphs[0]
        .as_str()
        .expect("paragraphs[0] should be a string");
    assert!(
        first_paragraph.contains("大力提振消费") || first_paragraph.contains("消费"),
        "first paragraph content not as expected: {}",
        first_paragraph
    );

    // 图片 URL 数组 image_urls：应包含多张图片，且为字符串数组
    let image_urls = result["image_urls"]
        .as_array()
        .expect("image_urls should be an array");
    assert!(
        image_urls.len() >= 3,
        "expected at least 3 image urls, got {}",
        image_urls.len()
    );
    for url_val in image_urls {
        let url = url_val
            .as_str()
            .expect("each image_urls item should be a string");
        assert!(
            url.starts_with("https://"),
            "unexpected image url format (not absolute): {}",
            url
        );
    }

    // 编辑信息 editor：包含"编辑："字样
    let editor = result["editor"]
        .as_str()
        .expect("editor should be a string");
    assert!(
        editor.contains("编辑："),
        "unexpected editor text: {}",
        editor
    );

    // ===== LLM 提取结果断言 =====

    let llm_summary = &result["llm_summary"];
    assert!(
        llm_summary.is_object(),
        "llm_summary should be an object json, got: {}",
        llm_summary
    );
    assert_eq!(
        llm_summary["title"],
        "消费与外贸走势",
        "unexpected llm_summary.title: {}",
        llm_summary["title"]
    );
    assert_eq!(
        llm_summary["category"],
        "宏观经济与对外开放",
        "unexpected llm_summary.category: {}",
        llm_summary["category"]
    );

    // 验证 TokenUsage 已正确累加（至少非负，且 total >= prompt + completion）
    assert!(usage.total_tokens >= usage.prompt_tokens + usage.completion_tokens);
}
