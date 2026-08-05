// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! LLM Prompt 提取示例
//!
//! 演示如何通过 `extraction_prompt` 字段在单次 scrape 请求中完成 LLM 提取。
//! 三种提取模式优先级：`extraction_rules` > `extraction_prompt` > `extraction_schema`。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example extraction_prompt
//! ```

use serde_json::json;
use std::time::Duration;

const API_URL: &str = "http://localhost:8899/v1/scrape";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    // === 示例 1：使用 extraction_prompt 提取 ===
    println!("=== 示例 1：extraction_prompt 提取 ===");
    let request_body = json!({
        "url": "https://example.com",
        "extraction_prompt": "Extract the page title, main heading, and all links with their URLs"
    });

    let response = client.post(API_URL).json(&request_body).send().await?;
    let result: serde_json::Value = response.json().await?;
    println!("响应: {}", serde_json::to_string_pretty(&result)?);

    // === 示例 2：使用 extraction_schema 提取 ===
    println!("\n=== 示例 2：extraction_schema 提取 ===");
    let request_body = json!({
        "url": "https://example.com",
        "extraction_schema": {
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "The page title"
                },
                "links": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": { "type": "string" },
                            "url": { "type": "string" }
                        }
                    }
                }
            },
            "required": ["title"]
        }
    });

    let response = client.post(API_URL).json(&request_body).send().await?;
    let result: serde_json::Value = response.json().await?;
    println!("响应: {}", serde_json::to_string_pretty(&result)?);

    // === 示例 3：extraction_rules + extraction_prompt 同时设置 ===
    // 注意：extraction_rules 优先级更高，extraction_prompt 会被忽略
    println!("\n=== 示例 3：优先级演示（rules > prompt > schema）===");
    let request_body = json!({
        "url": "https://example.com",
        "extraction_rules": {
            "title": {
                "selector": "h1",
                "attr": null,
                "is_array": false
            }
        },
        "extraction_prompt": "This will be ignored because extraction_rules takes priority"
    });

    let response = client.post(API_URL).json(&request_body).send().await?;
    let result: serde_json::Value = response.json().await?;
    println!("响应: {}", serde_json::to_string_pretty(&result)?);
    println!("注意：extraction_rules 优先级高于 extraction_prompt，prompt 被忽略");

    Ok(())
}
