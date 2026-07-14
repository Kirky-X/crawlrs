// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 表单数据提取示例
//!
//! 演示如何从网页表单中提取数据，包括：
//! - 识别表单字段
//! - 提取输入值
//! - 处理动态表单
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example form_extraction
//! ```

use crawlrs::engines::engine_client::{EngineClient, ScrapeRequest};
use log::info;
use std::time::Duration;

/// 模拟的表单数据提取结果
#[derive(Debug)]
struct FormData {
    fields: Vec<FormField>,
}

#[derive(Debug)]
struct FormField {
    name: String,
    value: String,
    field_type: String,
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始表单数据提取示例");
    info!("=====================================\n");

    let client = EngineClient::new();

    // 示例表单页面
    let url = "https://httpbin.org/forms/post";

    info!("📝 目标页面: {}", url);
    info!("");

    // 爬取页面
    let request = ScrapeRequest::new(url).timeout(Duration::from_secs(30));

    match client.scrape(&request).await {
        Ok(response) => {
            let content = &response.content;
            info!("✅ 页面获取成功");
            info!("  内容长度: {} 字节", content.len());
            info!("");

            // 解析表单数据
            let form_data = extract_form_data(content);

            info!("📋 检测到的表单字段:");
            info!("-----------------------------");

            for field in &form_data.fields {
                let value_preview = if field.value.len() > 30 {
                    format!("{}...", &field.value[..30])
                } else {
                    field.value.clone()
                };
                info!(
                    "  [{}] {} = \"{}\"",
                    field.field_type, field.name, value_preview
                );
            }

            info!("");
            info!("📊 总计: {} 个字段", form_data.fields.len());

            // 模拟提交表单
            info!("");
            info!("🔄 模拟表单提交...");
            simulate_form_submission(&form_data).await;
        }
        Err(e) => {
            info!("❌ 页面获取失败: {:?}", e);
            info!("⚠️  演示离线解析逻辑...");

            // 演示离线解析
            let sample_html = r#"<html><body><form method="POST">
                <input type="text" name="username" value="testuser"/>
                <input type="email" name="email" value="test@example.com"/>
                <input type="password" name="password" value="secret123"/>
                <textarea name="bio">Hello World</textarea>
                <select name="country">
                    <option value="us">United States</option>
                    <option value="cn" selected>China</option>
                </select>
                <input type="checkbox" name="subscribe" value="yes" checked/>
            </form></body></html>"#;

            let form_data = extract_form_data(sample_html);

            info!("📋 从示例HTML解析的表单字段:");
            for field in &form_data.fields {
                info!(
                    "  [{}] {} = \"{}\"",
                    field.field_type, field.name, field.value
                );
            }
        }
    }

    info!("\n=====================================");
    info!("✨ 表单数据提取示例完成");
}

fn extract_form_data(html: &str) -> FormData {
    let mut fields = Vec::new();

    // 提取 input 字段
    let input_pattern =
        regex::Regex::new(r#"<input[^>]+type="([^"]+)"[^>]+name="([^"]+)"[^>]+value="([^"]*)""#)
            .unwrap();
    for cap in input_pattern.captures_iter(html) {
        fields.push(FormField {
            field_type: cap[1].to_string(),
            name: cap[2].to_string(),
            value: cap[3].to_string(),
        });
    }

    // 提取没有value的input字段
    let no_value_pattern =
        regex::Regex::new(r#"<input[^>]+type="([^"]+)"[^>]+name="([^"]+)"[^>]*>"#).unwrap();
    for cap in no_value_pattern.captures_iter(html) {
        if !fields.iter().any(|f| f.name == cap[2]) {
            fields.push(FormField {
                field_type: cap[1].to_string(),
                name: cap[2].to_string(),
                value: String::new(),
            });
        }
    }

    // 提取 textarea
    let textarea_pattern =
        regex::Regex::new(r#"<textarea[^>]+name="([^"]+)"[^>]*>([^<]*)</textarea>"#).unwrap();
    for cap in textarea_pattern.captures_iter(html) {
        fields.push(FormField {
            field_type: "textarea".to_string(),
            name: cap[1].to_string(),
            value: cap[2].to_string(),
        });
    }

    // 提取 select 字段
    let select_pattern =
        regex::Regex::new(r#"<select[^>]+name="([^"]+)"[^>]*>([\s\S]*?)</select>"#).unwrap();
    for cap in select_pattern.captures_iter(html) {
        let select_content = &cap[2];
        let selected = if select_content.contains("selected") || select_content.contains("checked")
        {
            // 提取选中的值
            let selected_pattern =
                regex::Regex::new(r#"<option[^>]+value="([^"]+)"[^>]+selected"#).unwrap();
            if let Some(selected_cap) = selected_pattern.captures(select_content) {
                selected_cap[1].to_string()
            } else {
                // 默认取第一个
                let first_option = regex::Regex::new(r#"<option[^>]+value="([^"]+)""#).unwrap();
                if let Some(first_cap) = first_option.captures(select_content) {
                    first_cap[1].to_string()
                } else {
                    String::new()
                }
            }
        } else {
            String::new()
        };

        fields.push(FormField {
            field_type: "select".to_string(),
            name: cap[1].to_string(),
            value: selected,
        });
    }

    FormData { fields }
}

async fn simulate_form_submission(form_data: &FormData) {
    // 准备表单数据
    let mut form_values = std::collections::HashMap::new();
    for field in &form_data.fields {
        if !field.value.is_empty() {
            form_values.insert(field.name.clone(), field.value.clone());
        }
    }

    info!("  准备提交的数据: {:?}", form_values);
    info!("✅ 表单数据准备完成");
    info!("  注意: 实际提交需要使用POST请求和正确的Content-Type");
}
