use crawlrs::config::settings::Settings;
use crawlrs::domain::services::extraction_service::{ExtractionRule, ExtractionService};
use crawlrs::domain::services::llm_service::LLMService;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 使用环境变量设置 LLM 配置（安全方式）
    std::env::set_var("CRAWLRS__DATABASE__URL", "postgres://localhost/test");
    std::env::set_var("OLLAMA_HOST", "http://172.24.160.1:11434");
    std::env::set_var("CRAWLRS__LLM__PROVIDER", "openai");
    std::env::set_var("CRAWLRS__LLM__MODEL", "qwen3:8b");
    std::env::set_var("CRAWLRS__LLM__API_BASE_URL", "http://172.24.160.1:11434/v1");
    std::env::set_var("CRAWLRS__LLM__API_KEY", "ollama-is-ok");

    let settings = Settings::new().expect("Failed to load settings");

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let service = ExtractionService::new(Box::new(LLMService::new(&settings, http_client)));

    // 读取指定的真实 HTML 文件
    let html = std::fs::read_to_string("temp/extraction_test/raw.html")
        .expect("Failed to read temp/extraction_test/raw.html");

    // 测试 Markdown 提取
    println!("Starting markdown extraction...");
    let mut markdown_rules = HashMap::new();
    markdown_rules.insert(
        "content_md".to_string(),
        ExtractionRule {
            selector: Some("div.content_area".to_string()), // 使用 raw.html 中的实际选择器
            attr: None,
            is_array: false,
            use_llm: Some(true),
            llm_prompt: Some(
                "请将以下新闻正文内容整理成标准 Markdown 格式，保留标题层级、列表、引用等结构"
                    .to_string(),
            ),
            output_format: Some("markdown".to_string()),
        },
    );

    let (markdown_result, markdown_usage) = service
        .extract_data(&html, &markdown_rules, Some("https://news.cctv.com/"))
        .await?;

    println!("Markdown Result:\n{:#?}", markdown_result["content_md"]);
    println!("Markdown Token Usage: {:#?}\n", markdown_usage);

    // 测试 JSON 提取
    println!("Starting JSON extraction...");
    let mut json_rules = HashMap::new();
    json_rules.insert(
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

    let (json_result, json_usage) = service
        .extract_data(&html, &json_rules, Some("https://news.cctv.com/"))
        .await?;

    println!("JSON Result:\n{:#?}", json_result["summary_json"]);
    println!("JSON Token Usage: {:#?}", json_usage);

    Ok(())
}
