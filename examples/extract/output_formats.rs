// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 输出格式示例
//!
//! 演示 crawlrs 提取结果的不同输出格式：
//! - JSON 格式（默认）— 结构化数据
//! - Plaintext 格式 — 纯文本内容
//! - Markdown 格式 — 通过 scrape 的 formats 参数获取
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example output_formats
//! ```

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 输出格式示例");
    info!("=====================================\n");

    let base_url = "http://localhost:8899";

    // 1. JSON 输出格式
    info!("1️⃣  JSON 输出格式（默认）");
    info!("-----------------------------");
    info!("  POST {}/v1/extract", base_url);
    info!("  {{");
    info!("    \"url\": \"https://example.com\",");
    info!("    \"extraction_rules\": {{");
    info!("      \"title\": {{ \"selector\": \"h1\", \"is_array\": false }},");
    info!("      \"paragraphs\": {{ \"selector\": \"p\", \"is_array\": true }}");
    info!("    }}");
    info!("  }}");
    info!("");
    info!("  输出:");
    info!("  {{");
    info!("    \"data\": {{");
    info!("      \"title\": \"Example Domain\",");
    info!("      \"paragraphs\": [\"This domain is...\", \"More text...\"]");
    info!("    }}");
    info!("  }}");
    info!("");

    // 2. Plaintext 输出格式
    info!("2️⃣  Plaintext 输出格式");
    info!("-----------------------------");
    info!("  在提取规则中设置 output_format:");
    info!("  {{");
    info!("    \"content\": {{");
    info!("      \"selector\": \"article\",");
    info!("      \"is_array\": false,");
    info!("      \"output_format\": \"plaintext\"");
    info!("    }}");
    info!("  }}");
    info!("");
    info!("  输出:");
    info!("  {{");
    info!("    \"data\": {{");
    info!("      \"content\": \"Article text without HTML tags...\"");
    info!("    }}");
    info!("  }}");
    info!("");

    // 3. Scrape 多格式输出
    info!("3️⃣  Scrape 多格式输出");
    info!("-----------------------------");
    info!("  POST {}/v1/scrape", base_url);
    info!("  {{");
    info!("    \"url\": \"https://example.com\",");
    info!("    \"formats\": [\"html\", \"markdown\", \"rawHtml\"]");
    info!("  }}");
    info!("");
    info!("  可用格式:");
    info!("    html      — 清理后的 HTML（移除脚本/样式等）");
    info!("    markdown  — 转换为 Markdown 格式");
    info!("    rawHtml   — 原始 HTML（未处理）");
    info!("");
    info!("  💡 提示:");
    info!("     - formats 为数组，可同时请求多种格式");
    info!("     - 不指定 formats 时默认返回 html");
    info!("     - markdown 格式适合内容分析和 LLM 处理");

    info!("\n=====================================");
    info!("✨ 输出格式示例完成");
}
