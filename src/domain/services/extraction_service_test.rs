use crate::domain::services::extraction_service::{ExtractionRule, ExtractionService};
use crate::domain::services::llm_service::MockLLMServiceTrait;
use serde_json::{json, Value};
use std::collections::HashMap;

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

    let result = tokio_test::block_on(ExtractionService::extract(html, &rules)).unwrap();

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

    let result = tokio_test::block_on(ExtractionService::extract(html, &rules)).unwrap();
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

    let result = tokio_test::block_on(ExtractionService::extract(html, &rules)).unwrap();
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

    let mut mock_llm = MockLLMServiceTrait::new();
    mock_llm
        .expect_extract_data()
        .times(1)
        .returning(|_, _| Ok(json!({"name": "Product X", "price": 100})));

    let service = ExtractionService::new(Box::new(mock_llm));
    let result = service.extract_data(html, &rules).await.unwrap();

    assert_eq!(result["product_info"]["name"], "Product X");
    assert_eq!(result["product_info"]["price"], 100);
}
