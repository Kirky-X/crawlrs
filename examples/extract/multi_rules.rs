// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 多规则提取示例
//!
//! 演示如何通过 `/v1/extract` 端点同时使用多条提取规则，
//! 从同一页面提取多种结构化数据。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example multi_rules
//! ```

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 多规则提取示例");
    info!("=====================================\n");

    let base_url = "http://localhost:8899";

    // 1. 多规则提取请求
    info!("1️⃣  多规则提取请求");
    info!("-----------------------------");
    info!("  POST {}/v1/extract", base_url);
    info!("  {{");
    info!("    \"url\": \"https://example.com/article\",");
    info!("    \"extraction_rules\": {{");
    info!("      \"title\": {{");
    info!("        \"selector\": \"h1\",");
    info!("        \"is_array\": false");
    info!("      }},");
    info!("      \"links\": {{");
    info!("        \"selector\": \"a[href]\",");
    info!("        \"attr\": \"href\",");
    info!("        \"is_array\": true");
    info!("      }},");
    info!("      \"images\": {{");
    info!("        \"selector\": \"img[src]\",");
    info!("        \"attr\": \"src\",");
    info!("        \"is_array\": true");
    info!("      }},");
    info!("      \"meta_description\": {{");
    info!("        \"selector\": \"meta[name='description']\",");
    info!("        \"attr\": \"content\",");
    info!("        \"is_array\": false");
    info!("      }}");
    info!("    }}");
    info!("  }}");
    info!("");

    // 2. 提取规则字段说明
    info!("2️⃣  提取规则字段说明");
    info!("-----------------------------");
    info!("  每条规则包含以下字段:");
    info!("");
    info!("  selector     — CSS 选择器（如 \"h1\", \"div.content\", \"a.link\"）");
    info!("  attr         — 提取属性值（如 \"href\", \"src\", \"content\"）");
    info!("                 省略时提取文本内容");
    info!("  is_array     — true 返回数组，false 返回第一个匹配");
    info!("  use_llm      — true 启用 LLM 提取（需配置 LLM provider）");
    info!("  llm_prompt   — LLM 提取的自定义提示词");
    info!("  output_format — \"json\"（默认）或 \"plaintext\"");
    info!("");

    // 3. 预期响应
    info!("3️⃣  预期响应结构");
    info!("-----------------------------");
    info!("  {{");
    info!("    \"data\": {{");
    info!("      \"title\": \"Example Article\",");
    info!("      \"links\": [");
    info!("        \"https://example.com/page1\",");
    info!("        \"https://example.com/page2\"");
    info!("      ],");
    info!("      \"images\": [");
    info!("        \"https://example.com/img1.jpg\",");
    info!("        \"https://example.com/img2.png\"");
    info!("      ],");
    info!("      \"meta_description\": \"A brief article about...\"");
    info!("    }},");
    info!("    \"token_usage\": {{ \"total_tokens\": 0 }}");
    info!("  }}");
    info!("");
    info!("  💡 提示:");
    info!("     - 所有规则在同一次页面请求中并行执行");
    info!("     - 混合 CSS 选择器和 LLM 提取可以减少 API 调用");
    info!("     - is_array=true 的规则返回所有匹配项");

    info!("\n=====================================");
    info!("✨ 多规则提取示例完成");
}
