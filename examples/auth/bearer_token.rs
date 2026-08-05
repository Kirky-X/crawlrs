// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Bearer Token 认证示例
//!
//! 演示如何使用 `Authorization: Bearer <token>` 方式进行身份验证。
//! crawlrs 的 Bearer Token 实际就是 API Key，与 `x-api-key` 头等效。
//!
//! # 前提条件
//!
//! 需要先启动 crawlrs 服务（启用 auth feature）。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example bearer_token
//! ```

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 Bearer Token 认证示例");
    info!("=====================================\n");

    let base_url = "http://localhost:8899";
    let api_key =
        std::env::var("CRAWLRS_API_KEY").unwrap_or_else(|_| "your-api-key-here".to_string());

    // 1. Bearer Token 格式
    info!("1️⃣  Bearer Token 格式");
    info!("-----------------------------");
    info!(
        "  Authorization: Bearer {}",
        &api_key[..api_key.len().min(8)]
    );
    info!("  ...");
    info!("");
    info!("  crawlrs 支持两种等效的认证头:");
    info!("    - Authorization: Bearer <api-key>");
    info!("    - x-api-key: <api-key>");
    info!("");

    // 2. 使用 curl 示例
    info!("2️⃣  curl 调用示例");
    info!("-----------------------------");
    info!("  # 单页爬取");
    info!("  curl -X POST {}/v1/scrape \\", base_url);
    info!("    -H 'Authorization: Bearer <api-key>' \\");
    info!("    -H 'Content-Type: application/json' \\");
    info!("    -d '{{\"url\": \"https://example.com\"}}'");
    info!("");
    info!("  # 整站爬取");
    info!("  curl -X POST {}/v1/crawl \\", base_url);
    info!("    -H 'Authorization: Bearer <api-key>' \\");
    info!("    -H 'Content-Type: application/json' \\");
    info!("    -d '{{\"url\": \"https://example.com\", \"config\": {{\"max_depth\": 2}}}}'");
    info!("");
    info!("  # 搜索");
    info!("  curl -X POST {}/v1/search \\", base_url);
    info!("    -H 'Authorization: Bearer <api-key>' \\");
    info!("    -H 'Content-Type: application/json' \\");
    info!("    -d '{{\"query\": \"rust web scraping\", \"engines\": [\"google\"]}}'");
    info!("");

    // 3. 环境变量安全实践
    info!("3️⃣  安全实践");
    info!("-----------------------------");
    info!("  推荐通过环境变量传递 API Key，避免在命令行或代码中暴露:");
    info!("");
    info!("  export CRAWLRS_API_KEY='your-api-key-here'");
    info!("  curl -X POST {}/v1/scrape \\", base_url);
    info!("    -H \"Authorization: Bearer $CRAWLRS_API_KEY\" \\");
    info!("    -H 'Content-Type: application/json' \\");
    info!("    -d '{{\"url\": \"https://example.com\"}}'");
    info!("");
    info!("  💡 安全提示:");
    info!("     - 永远不要在代码中硬编码 API Key");
    info!("     - 使用 .env 文件 + .gitignore 管理本地密钥");
    info!("     - 生产环境使用密钥管理服务（如 Vault、AWS Secrets Manager）");
    info!("     - 定期轮换 API Key");

    info!("\n=====================================");
    info!("✨ Bearer Token 认证示例完成");
}
